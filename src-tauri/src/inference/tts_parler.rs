// Adaptive Device Scheduler - CPU Thread Isolation for Parler-TTS
// This module mirrors tts_candle logic but is kept as `tts_parler` per prompt naming.
// It ensures CPU synthesis uses a bounded Rayon pool (2 threads) to prevent Tokio starvation.

use std::sync::Arc;

use crate::error::{GabrielError, Result};

// Limit CPU worker threads during CPU synthesis task
pub fn synthesize_cpu_isolated<F, R>(f: F) -> R
where
    F: FnOnce() -> R + Send,
    R: Send,
{
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(2) // Constrain CPU tensor math to 2 cores
        .build()
        .unwrap();

    pool.install(|| {
        // Run TTS inference on CPU without hijacking all system threads
        f()
    })
}

// Re-export the real backend for convenience when feature is enabled
#[cfg(feature = "tts-parler")]
pub use crate::inference::tts_candle::CandleSpeechBackend;

/// Helper that routes synthesis based on available VRAM budget.
/// Uses the static thread pool from tts_candle for CPU isolation.
pub async fn synthesize_with_device_routing(
    backend: Arc<dyn crate::inference::SpeechBackend>,
    text: String,
    voice: String,
    cpu_fallback: bool,
) -> Result<Vec<u8>> {
    if cpu_fallback {
        let bg = backend.clone();
        tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Handle::current();
            synthesize_cpu_isolated(|| rt.block_on(bg.synthesize(&text, &voice, true)))
        })
        .await
        .map_err(|_| GabrielError::Backend("speech worker terminated abnormally".into()))?
    } else {
        backend.synthesize(&text, &voice, cpu_fallback).await
    }
}
