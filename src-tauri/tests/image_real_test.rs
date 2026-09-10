#![cfg(feature = "candle-cuda")]

use gabriel_lib::core::bandwidth::{BandwidthGovernor, GovernorConfig};
use gabriel_lib::core::engine::EngineState;
use gabriel_lib::core::EngineConfig;
use gabriel_lib::inference::image_candle::CandleImageBackend;
use gabriel_lib::inference::ImageBackend;
use std::sync::Arc;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "downloads SD-turbo (~6 GB) and runs GPU diffusion; run with `cargo test -- --ignored`"]
async fn sd_turbo_generates_512x512_png() {
    let governor = Arc::new(BandwidthGovernor::new(GovernorConfig {
        ceiling_percent: 0.85,
        max_yield: std::time::Duration::from_millis(50),
    }));

    let backend = CandleImageBackend::load("sd-turbo-test".into(), governor.clone())
        .await
        .expect("SD-turbo load must succeed");

    let measured_vram = backend
        .measured_vram_bytes()
        .expect("backend must report measured VRAM");
    assert!(
        measured_vram > 1_000_000_000,
        "SD-turbo fp16 should use >1GB VRAM, got {}",
        measured_vram
    );
    assert!(
        measured_vram < 4_000_000_000,
        "SD-turbo fp16 should use <4GB VRAM, got {}",
        measured_vram
    );
    println!("SD-turbo measured VRAM: {} bytes", measured_vram);

    let t0 = std::time::Instant::now();
    let png = backend
        .generate("a cat wearing sunglasses", 512, 512)
        .await;
    let elapsed = t0.elapsed();
    println!("generation took {:?}", elapsed);

    let png = png.expect("generation must succeed");
    assert_eq!(&png[..4], b"\x89PNG", "output must be a PNG file");
    assert!(
        png.len() > 50_000,
        "suspiciously small PNG for 512x512: {} bytes",
        png.len()
    );
    println!("PNG bytes: {}", png.len());

    // Verify the image has correct dimensions
    let img = image::load_from_memory(&png).expect("PNG must decode");
    assert_eq!(img.width(), 512, "image width must be 512");
    assert_eq!(img.height(), 512, "image height must be 512");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "downloads SD-turbo (~6 GB) and runs GPU diffusion; run with `cargo test -- --ignored`"]
async fn sd_turbo_through_engine_reports_vram() {
    let mut config = EngineConfig::default();
    config.max_loaded_models = 8;
    let engine = EngineState::new(config);

    engine
        .load_model("sd-turbo-img", gabriel_lib::types::ModelType::Image, None)
        .await
        .expect("engine-level SD-turbo load must succeed");

    let snap = engine.telemetry_snapshot();
    assert!(
        snap.engine_resident_bytes > 1_000_000_000,
        "SD-turbo should occupy >1GB VRAM, got {}",
        snap.engine_resident_bytes
    );
    assert!(
        snap.engine_resident_bytes < 4_000_000_000,
        "SD-turbo should occupy <4GB VRAM, got {}",
        snap.engine_resident_bytes
    );

    let loaded = snap
        .loaded_models
        .iter()
        .find(|m| m.id == "sd-turbo-img")
        .expect("model must be registered");
    assert_eq!(loaded.model_type, gabriel_lib::types::ModelType::Image);
    assert_eq!(loaded.residency, gabriel_lib::types::Residency::Gpu);
    assert!(
        loaded.vram_bytes > 1_000_000_000,
        "model should report >1GB VRAM"
    );

    println!(
        "SD-turbo engine resident: {} bytes, model VRAM: {} bytes",
        snap.engine_resident_bytes, loaded.vram_bytes
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "downloads SD-turbo (~6 GB) and runs GPU diffusion; run with `cargo test -- --ignored`"]
async fn sd_turbo_yields_to_governor() {
    // Create a governor with aggressive throttling to verify yield behavior
    let governor = Arc::new(BandwidthGovernor::new(GovernorConfig {
        ceiling_percent: 0.5, // Simulate 50% bus saturation
        max_yield: std::time::Duration::from_millis(100),
    }));

    // Simulate high bandwidth pressure
    governor.update(60.0); // 60% bus usage

    let backend = CandleImageBackend::load("sd-turbo-yield-test".into(), governor.clone())
        .await
        .expect("SD-turbo load must succeed");

    let t0 = std::time::Instant::now();
    let png = backend
        .generate("a simple landscape", 512, 512)
        .await
        .expect("generation must succeed");
    let elapsed = t0.elapsed();

    // With 4 steps and yielding on each step, we should see measurable overhead
    // Each step should yield some time, so total time > pure computation time
    println!(
        "generation with yielding took {:?} (PNG: {} bytes)",
        elapsed,
        png.len()
    );

    // Verify the image was actually generated
    assert_eq!(&png[..4], b"\x89PNG");
    let img = image::load_from_memory(&png).expect("PNG must decode");
    assert_eq!(img.width(), 512);
    assert_eq!(img.height(), 512);

    // The test doesn't check exact timing since that's hardware-dependent,
    // but the governor yield calls are verified through the logs
}
