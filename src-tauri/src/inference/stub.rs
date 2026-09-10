use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::mpsc;
use tracing::debug;

use crate::error::Result;
use crate::inference::audio::{SAMPLE_RATE_HZ, WavBuilder};
use crate::inference::image_codec::{encode_bmp_rgba, hash_string};
use crate::types::{ChatEvent, GenParams};

use super::{ImageBackend, SpeechBackend, TextBackend};

const TOKEN_DELAY_MS: u64 = 14;

#[derive(Debug)]
pub struct StubTextBackend {
    model_id: String,
}

impl StubTextBackend {
    pub fn new(model_id: String) -> Self {
        Self { model_id }
    }

    fn compose(prompt: &str) -> String {
        let topic = prompt
            .lines()
            .last()
            .unwrap_or("your request")
            .trim_start_matches("user: ")
            .chars()
            .take(160)
            .collect::<String>();
        format!(
            "Gabriel local engine online. Processing \"{topic}\". \
This response is produced by the deterministic stub backend; swap in a Candle-backed \
TextBackend via the `candle-cuda` feature for real weights. The scheduler, VRAM pager and \
streaming pipeline you are seeing are fully live."
        )
    }
}

#[async_trait]
impl TextBackend for StubTextBackend {
    async fn stream_tokens(
        &self,
        prompt: &str,
        params: GenParams,
        tx: mpsc::Sender<ChatEvent>,
    ) -> Result<u32> {
        let reply = Self::compose(prompt);
        let mut count = 0u32;
        for token in tokenize(&reply) {
            if count >= params.max_tokens {
                break;
            }
            if tx.send(ChatEvent::Token(token.to_string())).await.is_err() {
                break;
            }
            count += 1;
            tokio::time::sleep(std::time::Duration::from_millis(TOKEN_DELAY_MS)).await;
        }
        debug!(model = %self.model_id, tokens = count, "stub text stream done");
        Ok(count)
    }
}

fn tokenize(text: &str) -> impl Iterator<Item = &str> {
    text.split_inclusive(' ')
}

#[derive(Debug)]
pub struct StubImageBackend {
    model_id: String,
    governor: Arc<crate::core::bandwidth::BandwidthGovernor>,
}

impl StubImageBackend {
    pub fn new(model_id: String, governor: Arc<crate::core::bandwidth::BandwidthGovernor>) -> Self {
        Self { model_id, governor }
    }
}

#[async_trait]
impl ImageBackend for StubImageBackend {
    async fn generate(&self, prompt: &str, width: u32, height: u32) -> Result<Vec<u8>> {
        let steps = 20;
        for step in 0..steps {
            let yield_time = self.governor.standard_yield();
            if !yield_time.is_zero() {
                debug!(
                    model = %self.model_id,
                    step = step + 1,
                    yield_ms = yield_time.as_millis() as u64,
                    "diffusion yielding memory bus to interactive traffic"
                );
                tokio::time::sleep(yield_time).await;
            }
            tokio::time::sleep(std::time::Duration::from_millis(60)).await;
        }
        let seed = hash_string(prompt) ^ hash_string(&self.model_id);
        Ok(encode_bmp_rgba(width, height, seed))
    }
}

#[derive(Debug)]
pub struct StubSpeechBackend {
    model_id: String,
}

impl StubSpeechBackend {
    pub fn new(model_id: String) -> Self {
        Self { model_id }
    }
}

/// CPU Thread Isolation helper for stub TTS:
/// Constrains Rayon/OpenMP worker pool to 2 threads to avoid starving Tokio
/// async workers or stalling CUDA host driver calls.
fn synthesize_cpu_isolated_stub<F, R>(f: F) -> R
where
    F: FnOnce() -> R + Send,
    R: Send,
{
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(2) // Constrain CPU tensor math to 2 cores
        .build()
        .unwrap();
    pool.install(f)
}

#[async_trait]
impl SpeechBackend for StubSpeechBackend {
    async fn synthesize(&self, text: &str, voice: &str, _force_cpu: bool) -> Result<Vec<u8>> {
        let base_hz = match voice.to_ascii_lowercase().as_str() {
            "alloy" | "echo" => 150.0,
            "fable" | "onyx" => 120.0,
            "nova" | "shimmer" => 210.0,
            _ => 175.0,
        };

        let duration_secs = (text.len() as f32 * 0.055).clamp(0.4, 30.0);
        let total_samples = (duration_secs * SAMPLE_RATE_HZ as f32) as usize;
        // Use isolated pool to avoid hijacking all system threads
        let model_id = self.model_id.clone();
        let wav_bytes = tokio::task::spawn_blocking(move || {
            synthesize_cpu_isolated_stub(|| {
                let mut wav = WavBuilder::new(SAMPLE_RATE_HZ);
                for i in 0..total_samples {
                    let t = i as f32 / SAMPLE_RATE_HZ as f32;
                    let envelope = (t / 0.05).min(1.0) * ((duration_secs - t) / 0.08).min(1.0);
                    let syllable = 2.2 + 1.6 * (t * 3.7).sin();
                    let vibrato = 1.0 + 0.02 * (t * 9.0).sin();
                    let sample =
                        (2.0 * std::f32::consts::PI * base_hz * syllable * vibrato * t).sin()
                            * 0.35
                            * envelope.clamp(0.0, 1.0);
                    wav.push_sample((sample * i16::MAX as f32) as i16);
                }
                debug!(model = %model_id, secs = duration_secs, "stub speech synthesized (cpu-isolated)");
                wav.finish()
            })
        })
        .await
        .map_err(|_| crate::error::GabrielError::Backend("speech worker terminated abnormally".into()))?;

        Ok(wav_bytes)
    }
}
