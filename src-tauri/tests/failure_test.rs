use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use gabriel_lib::api::router::build_router;
use gabriel_lib::core::{scheduler::Scheduler, EngineConfig};
use gabriel_lib::core::engine::EngineState;
use gabriel_lib::types::{Job, JobKind, ModelType, Priority};
use serde_json::json;
use tower::ServiceExt;

fn engine() -> EngineState {
    EngineState::new(EngineConfig::default())
}

#[tokio::test]
async fn oversized_vram_budget_is_rejected_not_crashed() {
    let e = engine();

    let err = e
        .load_model("colossus", ModelType::Llm, Some(512 * 1024 * 1024 * 1024))
        .await
        .expect_err("must reject absurd budget");

    assert_eq!(err.code_slug(), "vram_exhausted");
    assert!(e.list_models().is_empty(), "failed load must not linger");
    assert!(
        e.telemetry_snapshot().engine_resident_bytes == 0,
        "failed load must not charge the VRAM ledger"
    );
}

#[tokio::test]
async fn failed_load_frees_capacity_for_next_model() {
    let e = engine();

    let _ = e
        .load_model("too-big", ModelType::Image, Some(1024 * 1024 * 1024 * 1024))
        .await;

    e.load_model("just-right", ModelType::Tts, Some(32 * 1024 * 1024))
        .await
        .expect("engine must recover after rejected admission");
}

#[tokio::test]
async fn client_disconnect_mid_stream_is_graceful() {
    let e = engine();
    e.load_model("chat", ModelType::Llm, None).await.unwrap();

    let rx = e
        .submit_chat("chat", "long running".into(), Default::default())
        .await
        .unwrap();
    drop(rx);

    tokio::time::sleep(Duration::from_millis(300)).await;

    let rx2 = e
        .submit_chat("chat", "recovery probe".into(), Default::default())
        .await
        .expect("engine healthy after dropped consumer");

    let mut events = 0;
    let mut rx2 = rx2;
    while let Some(ev) = rx2.recv().await {
        events += 1;
        if matches!(ev, gabriel_lib::types::ChatEvent::Done { .. }) {
            break;
        }
    }
    assert!(events > 0);
}

#[tokio::test]
async fn unload_during_active_stream_does_not_panic() {
    let e = engine();
    e.load_model("chat", ModelType::Llm, None).await.unwrap();

    let mut rx = e
        .submit_chat("chat", "unload race".into(), Default::default())
        .await
        .unwrap();

    e.unload_model("chat").await.unwrap();

    let mut saw_terminal = false;
    while let Some(ev) = rx.recv().await {
        match ev {
            gabriel_lib::types::ChatEvent::Done { .. } => {
                saw_terminal = true;
                break;
            }
            gabriel_lib::types::ChatEvent::Failed(_) => {
                saw_terminal = true;
                break;
            }
            _ => {}
        }
    }
    assert!(saw_terminal, "in-flight stream must reach a terminal event");
}

#[tokio::test]
async fn pager_offloaded_model_returns_clean_error() {
    use gabriel_lib::types::Residency;

    let e = engine();
    e.load_model("victim", ModelType::Llm, None).await.unwrap();

    let status = e.offload_model("victim").await.unwrap();
    assert_eq!(status.residency, Residency::Cpu);
    assert_eq!(e.telemetry_snapshot().engine_resident_bytes, 0);
    assert_eq!(e.list_models().len(), 1, "offloaded model stays registered");

    let app = build_router(e);
    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({"model": "victim", "messages": [{"role": "user", "content": "hi"}], "stream": false})
                .to_string(),
        ))
        .unwrap();

    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn scheduler_rejects_jobs_when_lane_saturated() {
    let (sched, mut rxs) = Scheduler::new(1);

    let make_job = |n: u8| Job {
        id: gabriel_lib::types::JobId::new_v4(),
        priority: Priority::Interactive,
        model_id: "m".into(),
        kind: JobKind::Speech {
            text: format!("job{n}"),
            voice: "alloy".into(),
            reply: tokio::sync::oneshot::channel().0,
            cpu_fallback: false,
        },
    };

    sched.try_submit(make_job(1)).expect("first fits");
    let second = sched.try_submit(make_job(2));
    assert!(second.is_err(), "full lane must reject backpressure-style");

    let drained = rxs.high.recv().await;
    assert!(drained.is_some());
}

#[tokio::test]
async fn unknown_model_type_is_rejected() {
    assert!(ModelType::parse("warp-drive").is_none());
    assert_eq!(ModelType::parse("LLM"), Some(ModelType::Llm));
    assert_eq!(ModelType::parse("Text-To-Speech"), Some(ModelType::Tts));
}

#[tokio::test]
async fn double_unload_and_missing_model_are_typed_errors() {
    let e = engine();
    e.load_model("x", ModelType::Tts, None).await.unwrap();
    e.unload_model("x").await.unwrap();

    let err = e.unload_model("x").await.expect_err("second unload fails");
    assert_eq!(err.code_slug(), "model_not_loaded");

    let err = e
        .submit_chat("never-loaded", "hi".into(), Default::default())
        .await
        .expect_err("unknown model");
    assert_eq!(err.code_slug(), "model_not_loaded");
}
