use gabriel_lib::core::engine::EngineState;
use gabriel_lib::core::EngineConfig;
use gabriel_lib::types::{ChatEvent, GenParams, ModelType};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use tokio::time::timeout;

#[tokio::main(flavor = "multi_thread", worker_threads = 8)]
async fn main() {
    let mut config = EngineConfig::default();
    config.max_loaded_models = 8;
    config.vram_high_watermark = 0.85;
    let engine = EngineState::new(config);

    println!("=== Three-Way Concurrent Multimodal Residency Test (Stub Verify) ===");

    // 0. Pre-load with updated footprints
    println!("[0/4] Pre-loading models...");
    engine.load_model("qwen-2.5-3b-instruct-q4_k_m.gguf", ModelType::Llm, Some(1_900_000_000)).await.unwrap();
    engine.load_model("sd-turbo", ModelType::Image, Some(1_600_000_000)).await.unwrap();
    engine.load_model("parler-tts", ModelType::Tts, Some(450_000_000)).await.unwrap();

    let snap_pre = engine.telemetry_snapshot();
    println!("Pre-load ledger: {} bytes (models: {})", snap_pre.engine_resident_bytes, snap_pre.loaded_models.len());
    assert_eq!(snap_pre.engine_resident_bytes, 1_900_000_000 + 1_600_000_000 + 450_000_000, "Total static load should be 3.95GB");
    println!("✅ Step 0 passed - footprints correct");

    // 1. Baseline LLM
    println!("[1/4] Measuring baseline LLM token latency...");
    let t0_baseline = Instant::now();
    let mut baseline_rx = engine.submit_chat("qwen-2.5-3b-instruct-q4_k_m.gguf", "Count to 5 slowly.".to_string(), GenParams::default()).await.unwrap();
    let mut baseline_tokens = 0;
    while let Some(event) = baseline_rx.recv().await {
        if let ChatEvent::Token(_) = event { baseline_tokens += 1; }
    }
    let baseline_duration = t0_baseline.elapsed();
    println!("Baseline: {} tokens in {:?}", baseline_tokens, baseline_duration);

    // 2. Concurrent workload
    println!("\n[2/4] Launching Concurrent Workloads (LLM + Image + TTS)...");
    let token_count = Arc::new(AtomicUsize::new(0));
    let token_count_clone = Arc::clone(&token_count);
    let engine_clone = engine.clone();
    let chat_handle = tokio::spawn(async move {
        let t0_concurrent = Instant::now();
        let mut chat_rx = engine_clone.submit_chat("qwen-2.5-3b-instruct-q4_k_m.gguf", "Explain physics in two sentences.".to_string(), GenParams::default()).await.unwrap();
        while let Some(event) = chat_rx.recv().await {
            if let ChatEvent::Token(_) = event { token_count_clone.fetch_add(1, Ordering::Relaxed); }
        }
        let elapsed = t0_concurrent.elapsed();
        (elapsed, token_count_clone.load(Ordering::Relaxed))
    });

    let engine_image = engine.clone();
    let image_handle = tokio::spawn(async move {
        let img_rx = engine_image.submit_image("sd-turbo", "a serene mountain landscape".to_string(), 512, 512).await.unwrap();
        timeout(Duration::from_secs(120), img_rx).await.unwrap().unwrap().unwrap()
    });

    let engine_tts = engine.clone();
    let tts_handle = tokio::task::spawn_blocking(move || {
        tokio::runtime::Handle::current().block_on(async {
            let speech_rx = engine_tts.submit_speech("parler-tts", "Gabriel local AI engine active.".to_string(), "nova".to_string()).await.unwrap();
            timeout(Duration::from_secs(180), speech_rx).await.unwrap().unwrap().unwrap()
        })
    });

    let (chat_elapsed, tokens_gen) = chat_handle.await.unwrap();
    let png_bytes = image_handle.await.unwrap();
    let wav_bytes = tts_handle.await.unwrap();

    println!("\n[3/4] Validating Modal Outputs & Performance...");
    assert!(tokens_gen != 0, "LLM must generate output tokens");
    let is_png = png_bytes.len() >= 4 && &png_bytes[..4] == b"\x89PNG";
    let is_bmp = png_bytes.len() >= 2 && &png_bytes[..2] == b"BM";
    assert!(is_png || is_bmp, "Image must be PNG or BMP");
    assert_eq!(&wav_bytes[..4], b"RIFF", "Speech must be WAV/RIFF");
    println!("✅ Output streams valid: LLM ({} tokens), Image ({} bytes), Speech ({} bytes)", tokens_gen, png_bytes.len(), wav_bytes.len());

    let concurrent_ms_per_token = chat_elapsed.as_millis() as f64 / tokens_gen.max(1) as f64;
    println!("LLM Token Speed under contention: {:.2} ms/token", concurrent_ms_per_token);

    // 4. Telemetry checks
    println!("\n[4/4] Telemetry & Memory Ledger Checks...");
    let snap = engine.telemetry_snapshot();
    let usage_ratio = if snap.vram_total_bytes == 0 { 0.0 } else { snap.engine_resident_bytes as f64 / snap.vram_total_bytes as f64 };
    println!("Total VRAM Allocation: {} / {} bytes ({:.2}%)", snap.engine_resident_bytes, snap.vram_total_bytes, usage_ratio*100.0);
    if snap.vram_total_bytes != 0 {
        assert!(usage_ratio <= 0.85, "VRAM exceeded high watermark");
    } else {
        const SIMULATED_8GB: u64 = 8 * 1024 * 1024 * 1024;
        const HIGH_WATERMARK_BUDGET: u64 = (SIMULATED_8GB as f64 * 0.85) as u64;
        assert!(snap.engine_resident_bytes <= HIGH_WATERMARK_BUDGET);
        println!("Ledger check (degraded mode) passed: {} <= {}", snap.engine_resident_bytes, HIGH_WATERMARK_BUDGET);
    }
    let tts_model = snap.loaded_models.iter().find(|m| m.id == "parler-tts").unwrap();
    assert!(tts_model.vram_bytes == 450_000_000 || tts_model.vram_bytes == 0, "TTS must be 450 MB or 0, got {}", tts_model.vram_bytes);
    if tts_model.vram_bytes == 450_000_000 {
        println!("✅ TTS VRAM: 450 MB (GPU path)");
    } else {
        println!("✅ TTS VRAM: 0 Bytes (CPU fallback)");
    }
    assert!(snap.engine_resident_bytes <= 6_500_000_000);
    println!("\n✅ All Step 4 Multimodal Concurrency Invariants PASSED!");
}
