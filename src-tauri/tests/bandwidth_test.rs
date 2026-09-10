use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use axum::http::{Request, StatusCode};
use gabriel_lib::api::router::build_router;
use gabriel_lib::core::bandwidth::{BandwidthGovernor, GovernorConfig};
use gabriel_lib::core::engine::EngineState;
use gabriel_lib::core::EngineConfig;
use gabriel_lib::inference::{stub::StubImageBackend, ImageBackend};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

fn engine_with(config: EngineConfig) -> EngineState {
    EngineState::new(config)
}

async fn loaded_default() -> EngineState {
    let e = engine_with(EngineConfig::default());
    for (id, t) in [
        ("m-llm", gabriel_lib::types::ModelType::Llm),
        ("m-image", gabriel_lib::types::ModelType::Image),
    ] {
        e.load_model(id, t, Some(32 * 1024 * 1024))
            .await
            .unwrap();
    }
    e
}

#[tokio::test]
async fn governor_reports_pressure_through_telemetry() {
    let e = loaded_default().await;

    let snap = e.telemetry_snapshot();
    assert!(
        snap.memory_bus_percent >= 0.0 && snap.memory_bus_percent <= 100.0,
        "bus percent out of range: {}",
        snap.memory_bus_percent
    );
    assert!(
        snap.bandwidth_pressure >= 0.0 && snap.bandwidth_pressure <= 1.0,
        "pressure out of range: {}",
        snap.bandwidth_pressure
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn interactive_lane_is_immune_to_bandwidth_pressure() {
    let e = loaded_default().await;

    for _ in 0..30 {
        e.governor().update(100.0);
    }
    assert!(
        e.governor().pressure() > 0.99,
        "saturated bus must register as full pressure"
    );

    let app = build_router(e);
    let req = Request::builder()
        .method("POST")
        .uri("/v1/chat/completions")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "model": "m-llm",
                "messages": [{"role": "user", "content": "interactive under pressure"}],
                "stream": false,
                "max_tokens": 8
            })
            .to_string(),
        ))
        .unwrap();

    let t0 = Instant::now();
    let res = app.oneshot(req).await.unwrap();
    let elapsed = t0.elapsed();

    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(!v["choices"][0]["message"]["content"].as_str().unwrap().is_empty());

    assert!(
        elapsed < Duration::from_millis(900),
        "interactive traffic must not absorb bus backoff (took {:?})",
        elapsed
    );
}

#[tokio::test]
async fn diffusion_backs_off_under_bus_pressure() {
    let hot = Arc::new(BandwidthGovernor::new(GovernorConfig {
        ceiling_percent: 10.0,
        max_yield: Duration::from_millis(50),
    }));
    let cool = Arc::new(BandwidthGovernor::new(GovernorConfig {
        ceiling_percent: 99.0,
        max_yield: Duration::from_millis(50),
    }));

    for _ in 0..40 {
        hot.update(100.0);
    }

    let base = Duration::from_millis(60) * 20;

    let cool_img = StubImageBackend::new("cool".into(), cool.clone())
        .generate("x", 16, 16)
        .await
        .unwrap();
    let t0 = Instant::now();
    let hot_img = StubImageBackend::new("hot".into(), hot.clone())
        .generate("y", 16, 16)
        .await
        .unwrap();
    let hot_elapsed = t0.elapsed();
    assert!(!cool_img.is_empty());

    assert_eq!(cool.standard_yield(), Duration::ZERO);
    assert!(hot.standard_yield() > Duration::ZERO);
    assert!(
        hot_elapsed >= base + Duration::from_millis(400),
        "pressured diffusion ({:?}) must take visibly longer than baseline {:?}",
        hot_elapsed,
        base
    );
    assert!(!hot_img.is_empty());
}

#[tokio::test]
async fn slot_pool_rejects_deterministically_instead_of_thrashing() {
    let e = engine_with(EngineConfig {
        max_loaded_models: 2,
        idle_offload_after: Duration::from_secs(600),
        ..EngineConfig::default()
    });

    e.load_model("a", gabriel_lib::types::ModelType::Llm, None)
        .await
        .unwrap();
    e.load_model("b", gabriel_lib::types::ModelType::Tts, None)
        .await
        .unwrap();

    let err = e
        .load_model("c", gabriel_lib::types::ModelType::Image, None)
        .await
        .expect_err("pool is saturated and nothing is idle-evictable");
    assert_eq!(err.code_slug(), "slot_pool_exhausted");
    assert_eq!(e.list_models().len(), 2, "rejection must not disturb residents");

    e.unload_model("b").await.unwrap();
    e.load_model("c", gabriel_lib::types::ModelType::Image, None)
        .await
        .expect("slot freed, admission succeeds");
    assert_eq!(e.list_models().len(), 2);
}

#[tokio::test]
async fn slot_pool_evicts_lru_idle_resident_when_saturated() {
    let e = engine_with(EngineConfig {
        max_loaded_models: 2,
        idle_offload_after: Duration::from_millis(80),
        ..EngineConfig::default()
    });

    e.load_model("old", gabriel_lib::types::ModelType::Tts, None)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    e.load_model("fresh", gabriel_lib::types::ModelType::Llm, None)
        .await
        .unwrap();

    e.load_model("newcomer", gabriel_lib::types::ModelType::Image, None)
        .await
        .expect("idle LRU 'old' should be evicted to make room");

    let ids: Vec<String> = e.list_models().into_iter().map(|m| m.id).collect();
    assert!(!ids.contains(&"old".to_string()));
    assert!(ids.contains(&"fresh".to_string()));
    assert!(ids.contains(&"newcomer".to_string()));
}
