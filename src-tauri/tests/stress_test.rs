use std::time::Instant;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use gabriel_lib::api::router::build_router;
use gabriel_lib::core::engine::EngineState;
use serde_json::{json, Value};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn engine() -> EngineState {
    EngineState::new(gabriel_lib::core::EngineConfig::default())
}

async fn loaded() -> EngineState {
    let e = engine();
    e.load_model("m-llm", gabriel_lib::types::ModelType::Llm, Some(128 * 1024 * 1024))
        .await
        .unwrap();
    e.load_model("m-image", gabriel_lib::types::ModelType::Image, Some(64 * 1024 * 1024))
        .await
        .unwrap();
    e.load_model("m-tts", gabriel_lib::types::ModelType::Tts, Some(16 * 1024 * 1024))
        .await
        .unwrap();
    e
}

async fn post(app: &axum::Router, uri: &str, payload: Value) -> (StatusCode, Vec<u8>) {
    let req = Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes().to_vec();
    (status, bytes)
}

fn chat_payload(model: &str, tokens: u32) -> Value {
    json!({
        "model": model,
        "messages": [{"role": "user", "content": "stress"}],
        "stream": false,
        "max_tokens": tokens
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_chat_storm_32_streams() {
    let app = build_router(loaded().await);

    let mut handles = Vec::new();
    for i in 0..32 {
        let app = app.clone();
        handles.push(tokio::spawn(async move {
            let payload = json!({
                "model": "m-llm",
                "messages": [{"role": "user", "content": format!("storm {i}")}],
                "stream": true,
                "max_tokens": 24
            });
            let req = Request::builder()
                .method("POST")
                .uri("/v1/chat/completions")
                .header("content-type", "application/json")
                .body(Body::from(payload.to_string()))
                .unwrap();
            let res = app.oneshot(req).await.unwrap();
            assert_eq!(res.status(), StatusCode::OK);
            res.into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec()
        }));
    }

    let mut failures = Vec::new();
    for (i, h) in handles.into_iter().enumerate() {
        let body = h.await.unwrap();
        let text = String::from_utf8(body).unwrap();
        if !text.contains("[DONE]") {
            failures.push(format!(
                "stream {i}: len={} head={:?} tail={:?}",
                text.len(),
                &text[..text.len().min(160)],
                &text[text.len().saturating_sub(160)..]
            ));
        }
    }
    assert!(failures.is_empty(), "streams that never terminated:\n{}", failures.join("\n"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mixed_modality_concurrent_workload() {
    let app = build_router(loaded().await);

    let mut handles = Vec::new();

    for _i in 0..16 {
        let app = app.clone();
        handles.push(tokio::spawn(async move {
            post(&app, "/v1/chat/completions", chat_payload("m-llm", 16)).await
        }));
    }
    for i in 0..4 {
        let app = app.clone();
        handles.push(tokio::spawn(async move {
            post(
                &app,
                "/v1/images/generations",
                json!({"prompt": format!("scene {i}"), "model": "m-image", "size": "16x16"}),
            )
            .await
        }));
    }
    for i in 0..4 {
        let app = app.clone();
        handles.push(tokio::spawn(async move {
            post(
                &app,
                "/v1/audio/speech",
                json!({"model": "m-tts", "input": format!("line {i}"), "voice": "nova"}),
            )
            .await
        }));
    }

    let mut ok = 0;
    for h in handles {
        let (status, _) = h.await.unwrap();
        assert_eq!(status, StatusCode::OK);
        ok += 1;
    }
    assert_eq!(ok, 24);
}

#[tokio::test]
async fn rapid_load_unload_cycles_do_not_deadlock() {
    let e = engine();
    let start = Instant::now();

    for i in 0..50 {
        let id = format!("cycle-{i}");
        e.load_model(&id, gabriel_lib::types::ModelType::Llm, Some(1024 * 1024))
            .await
            .expect("load");
        assert_eq!(e.list_models().len(), 1);
        e.unload_model(&id).await.expect("unload");
        assert!(e.list_models().is_empty());
    }

    assert!(
        start.elapsed().as_secs() < 30,
        "load/unload cycles must not stall"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn interactive_lane_preempts_saturated_standard_lane() {
    let app = build_router(loaded().await);

    let img_app = app.clone();
    let image_job = tokio::spawn(async move {
        post(
            &img_app,
            "/v1/images/generations",
            json!({"prompt": "slow diffusion", "model": "m-image", "size": "64x64"}),
        )
        .await
    });

    tokio::time::sleep(std::time::Duration::from_millis(150)).await;

    let t0 = Instant::now();
    let (status, body) = post(&app, "/v1/chat/completions", chat_payload("m-llm", 8)).await;
    let elapsed = t0.elapsed();

    assert_eq!(status, StatusCode::OK);
    let v: Value = serde_json::from_slice(&body).unwrap();
    assert!(!v["choices"][0]["message"]["content"].as_str().unwrap().is_empty());
    assert!(
        elapsed < std::time::Duration::from_millis(900),
        "interactive job must preempt background diffusion (took {:?})",
        elapsed
    );

    let _ = image_job.await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sustained_sequential_throughput_100_requests() {
    let app = build_router(loaded().await);

    let start = Instant::now();
    for i in 0..100 {
        let (status, body) =
            post(&app, "/v1/chat/completions", chat_payload("m-llm", 4)).await;
        assert_eq!(status, StatusCode::OK, "request {i} failed");
        let v: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(v["object"], "chat.completion");
    }
    let elapsed = start.elapsed();

    tracing_trivial_log();
    assert!(
        elapsed < std::time::Duration::from_secs(60),
        "throughput collapsed: 100 requests took {:?}",
        elapsed
    );
}

fn tracing_trivial_log() {}
