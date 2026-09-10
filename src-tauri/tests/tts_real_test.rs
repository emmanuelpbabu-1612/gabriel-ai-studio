#![cfg(feature = "tts-parler")]

use gabriel_lib::core::engine::EngineState;
use gabriel_lib::core::EngineConfig;
use gabriel_lib::inference::tts_candle::CandleSpeechBackend;
use gabriel_lib::inference::SpeechBackend;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "downloads parler-tts-mini-v1 (~1 GB) and runs slow CPU inference; run with `cargo test --features tts-parler -- --ignored`"]
async fn parler_synthesizes_wav_without_touching_vram() {
    let backend = CandleSpeechBackend::load("parler-mini-test".into())
        .await
        .expect("parler load must succeed");

    let t0 = std::time::Instant::now();
    let wav = backend.synthesize("Hello from Gabriel.", "nova", true).await;
    let elapsed = t0.elapsed();
    println!("synthesis took {elapsed:?}");

    let wav = wav.expect("synthesis must succeed");
    assert_eq!(&wav[..4], b"RIFF", "output must be a RIFF/WAV container");
    assert_eq!(&wav[8..12], b"WAVE");
    assert!(
        wav.len() > 8_000,
        "suspiciously tiny audio for a full sentence: {} bytes",
        wav.len()
    );
    println!("wav bytes: {}", wav.len());

    // The critical property: TTS must never charge the GPU ledger.
    let mut config = EngineConfig::default();
    config.max_loaded_models = 8;
    let engine = EngineState::new(config);
    engine
        .load_model("voice", gabriel_lib::types::ModelType::Tts, None)
        .await
        .expect("engine-level TTS load must succeed");

    let snap = engine.telemetry_snapshot();
    assert_eq!(
        snap.engine_resident_bytes, 0,
        "TTS must not occupy any VRAM"
    );
    assert!(
        snap.loaded_models
            .iter()
            .all(|m| m.id != "voice" || m.vram_bytes == 0),
        "registered TTS model must report zero VRAM"
    );
}
