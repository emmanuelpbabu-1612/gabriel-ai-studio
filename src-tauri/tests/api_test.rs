use axum::body::Body;
use axum::http::{Request, StatusCode};
use futures::StreamExt;
use gabriel_lib::api::router::build_router;
use gabriel_lib::core::engine::EngineState;
use http_body_util::BodyExt;
use serde_json::{json, Value};
use tower::ServiceExt;

fn test_engine() -> EngineState {
    let config = gabriel_lib::core::EngineConfig::default();
    EngineState::new(config)
}

async fn body_bytes(body: Body) -> Vec<u8> {
    body.collect()
        .await
        .expect("collectable body")
        .to_bytes()
        .to_vec()
}

#[tokio::test]
async fn models_endpoint_lists_loaded_models() {
    let engine = test_engine();
    engine
        .load_model("test-llm", gabriel_lib::types::ModelType::Llm, Some(64 * 1024 * 1024))
        .await
        .expect("load succeeds");
    let app = build_router(engine);

    let res = app
        .oneshot(
            Request::builder()
                .uri("/v1/models")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(res.status(), StatusCode::OK);
    let bytes = body_bytes(res.into_body()).await;
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(v["object"], "list");
    assert_eq!(v["data"][0]["id"], "test-llm");
    assert_eq!(v["data"][0]["gabriel"]["residency"], "vram");
}

#[tokio::test]
async fn chat_completion_non_stream_returns_content() {
    let engine = test_engine();
    engine
        .load_model("stub-chat", gabriel_lib::types::ModelType::Llm, None)
        .await
        .unwrap();
    let app = build_router(engine);

    let payload = json!({
        "model": "stub-chat",
        "messages": [
            {"role": "system", "content": "You are Gabriel."},
            {"role": "user", "content": "hello"}
        ],
        "stream": false,
        "max_tokens": 32
    });

    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = body_bytes(res.into_body()).await;
    let v: Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(v["object"], "chat.completion");
    assert!(
        v["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| !s.is_empty())
            .unwrap_or(false)
    );
    assert!(v["usage"]["total_tokens"].as_u64().unwrap() > 0);
}

#[tokio::test]
async fn chat_completion_stream_emits_sse_chunks() {
    let engine = test_engine();
    engine
        .load_model("stub-stream", gabriel_lib::types::ModelType::Llm, None)
        .await
        .unwrap();
    let app = build_router(engine);

    let payload = json!({
        "model": "stub-stream",
        "messages": [{"role": "user", "content": "stream please"}],
        "stream": true,
        "max_tokens": 16
    });

    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let mut stream = res.into_body().into_data_stream();
    let mut collected = String::new();
    while let Some(chunk) = stream.next().await {
        collected.push_str(&String::from_utf8_lossy(
            &chunk.expect("valid chunk"),
        ));
        if collected.contains("[DONE]") {
            break;
        }
    }

    assert!(collected.contains("chat.completion.chunk"));
    assert!(collected.contains("stop"));
}

#[tokio::test]
async fn image_generation_returns_b64_payload() {
    let engine = test_engine();
    engine
        .load_model("stub-image", gabriel_lib::types::ModelType::Image, None)
        .await
        .unwrap();
    let app = build_router(engine);

    let payload = json!({
        "prompt": "a neon skyline at dusk",
        "model": "stub-image",
        "size": "64x64"
    });

    let req = Request::builder()
        .method("POST")
        .uri("/v1/images/generations")
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = body_bytes(res.into_body()).await;
    let v: Value = serde_json::from_slice(&bytes).unwrap();

    let b64 = v["data"][0]["b64_json"].as_str().expect("b64 present");
    assert!(!b64.is_empty());
    assert_eq!(&b64[..2], "Qk");
}

#[tokio::test]
async fn speech_synthesis_returns_wav_audio() {
    let engine = test_engine();
    engine
        .load_model("stub-tts", gabriel_lib::types::ModelType::Tts, None)
        .await
        .unwrap();
    let app = build_router(engine);

    let payload = json!({
        "model": "stub-tts",
        "input": "Gabriel online.",
        "voice": "nova"
    });

    let req = Request::builder()
        .method("POST")
        .uri("/v1/audio/speech")
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers()["content-type"],
        "audio/wav"
    );

    let bytes = body_bytes(res.into_body()).await;
    assert_eq!(&bytes[..4], b"RIFF");
}

#[tokio::test]
async fn request_to_unloaded_model_is_404() {
    let engine = test_engine();
    let app = build_router(engine);

    let payload = json!({
        "model": "missing-model",
        "messages": [{"role": "user", "content": "hi"}],
        "stream": false
    });

    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn unload_removes_model_and_subsequent_requests_fail() {
    let engine = test_engine();
    engine
        .load_model("temp", gabriel_lib::types::ModelType::Tts, None)
        .await
        .unwrap();
    engine.unload_model("temp").await.unwrap();
    assert!(engine.list_models().is_empty());

    let app = build_router(engine);
    let payload = json!({
        "model": "temp",
        "input": "gone?",
        "voice": "alloy"
    });
    let req = Request::builder()
        .method("POST")
        .uri("/v1/audio/speech")
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}
