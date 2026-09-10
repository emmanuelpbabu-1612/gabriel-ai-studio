use axum::body::Body;
use axum::http::{Request, StatusCode};
use gabriel_lib::api::router::build_router;
use gabriel_lib::core::engine::EngineState;
use gabriel_lib::core::EngineConfig;
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

async fn engine() -> EngineState {
    let e = EngineState::new(EngineConfig::default());
    e.load_model("m-llm", gabriel_lib::types::ModelType::Llm, Some(64 * 1024 * 1024))
        .await
        .unwrap();
    e.load_model("m-image", gabriel_lib::types::ModelType::Image, Some(32 * 1024 * 1024))
        .await
        .unwrap();
    e.load_model("m-tts", gabriel_lib::types::ModelType::Tts, Some(16 * 1024 * 1024))
        .await
        .unwrap();
    e
}

async fn send(
    app: axum::Router,
    method: &str,
    uri: &str,
    content_type: Option<&str>,
    body: String,
) -> (StatusCode, Vec<u8>) {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(ct) = content_type {
        builder = builder.header("content-type", ct);
    }
    let res = app
        .oneshot(builder.body(Body::from(body)).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes().to_vec();
    (status, bytes)
}

#[tokio::test]
async fn malformed_json_is_rejected_with_4xx() {
    let app = build_router(engine().await);
    let (status, _) = send(
        app,
        "POST",
        "/v1/chat/completions",
        Some("application/json"),
        "{ not valid json !!!".into(),
    )
    .await;

    assert!(
        status == StatusCode::BAD_REQUEST || status == StatusCode::UNPROCESSABLE_ENTITY,
        "malformed JSON must be a client error, got {status}"
    );
}

#[tokio::test]
async fn missing_content_type_is_rejected() {
    let app = build_router(engine().await);
    let (status, _) = send(
        app,
        "POST",
        "/v1/chat/completions",
        None,
        json!({"model": "m-llm", "messages": []}).to_string(),
    )
    .await;

    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
}

#[tokio::test]
async fn oversized_body_is_rejected() {
    let app = build_router(engine().await);

    let huge = "x".repeat(3 * 1024 * 1024);
    let payload = json!({
        "model": "m-llm",
        "messages": [{"role": "user", "content": huge}],
        "stream": false
    });

    let (status, _) = send(
        app,
        "POST",
        "/v1/chat/completions",
        Some("application/json"),
        payload.to_string(),
    )
    .await;

    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn empty_messages_array_is_400() {
    let app = build_router(engine().await);
    let (status, _) = send(
        app,
        "POST",
        "/v1/chat/completions",
        Some("application/json"),
        json!({"model": "m-llm", "messages": [], "stream": false}).to_string(),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn oversized_chat_prompt_is_400() {
    let app = build_router(engine().await);
    let (status, body) = send(
        app,
        "POST",
        "/v1/chat/completions",
        Some("application/json"),
        json!({
            "model": "m-llm",
            "messages": [{"role": "user", "content": "a".repeat(70_000)}],
            "stream": false
        })
        .to_string(),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(String::from_utf8_lossy(&body).contains("character limit"));
}

#[tokio::test]
async fn oversized_image_prompt_is_400() {
    let app = build_router(engine().await);
    let (status, _) = send(
        app,
        "POST",
        "/v1/images/generations",
        Some("application/json"),
        json!({"prompt": "b".repeat(5_000), "model": "m-image"}).to_string(),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn oversized_speech_input_is_400() {
    let app = build_router(engine().await);
    let (status, _) = send(
        app,
        "POST",
        "/v1/audio/speech",
        Some("application/json"),
        json!({"model": "m-tts", "input": "c".repeat(20_000), "voice": "alloy"}).to_string(),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn absurd_generation_params_are_clamped_not_fatal() {
    let app = build_router(engine().await);

    let (status, body) = send(
        app,
        "POST",
        "/v1/chat/completions",
        Some("application/json"),
        json!({
            "model": "m-llm",
            "messages": [{"role": "user", "content": "clamp me"}],
            "stream": false,
            "max_tokens": 4_000_000_000u64,
            "temperature": 100.0
        })
        .to_string(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(!v["choices"][0]["message"]["content"].as_str().unwrap().is_empty());
}

#[tokio::test]
async fn invalid_image_dimensions_are_400() {
    let app = build_router(engine().await);

    for bad in ["0x0", "9999x9999", "axb", "100"] {
        let (status, _) = send(
            app.clone(),
            "POST",
            "/v1/images/generations",
            Some("application/json"),
            json!({"prompt": "test", "model": "m-image", "size": bad}).to_string(),
        )
        .await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "size '{bad}' must be rejected");
    }
}

#[tokio::test]
async fn sse_payloads_never_leak_raw_control_characters() {
    let app = build_router(engine().await);

    let (status, body) = send(
        app,
        "POST",
        "/v1/chat/completions",
        Some("application/json"),
        json!({
            "model": "m-llm",
            "messages": [{"role": "user", "content": "newline\r\ntest"}],
            "stream": true,
            "max_tokens": 64
        })
        .to_string(),
    )
    .await;

    assert_eq!(status, StatusCode::OK);
    let text = String::from_utf8(body).unwrap();

    for line in text.lines() {
        if let Some(data) = line.strip_prefix("data: ") {
            if data == "[DONE]" {
                continue;
            }
            assert!(
                serde_json::from_str::<serde_json::Value>(data).is_ok(),
                "every data: line must be valid JSON, got: {data}"
            );
        }
    }
}

#[tokio::test]
async fn unknown_routes_return_404_not_panic() {
    let app = build_router(engine().await);
    let res = app
        .oneshot(
            Request::builder()
                .uri("/v1/secret/admin")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}
