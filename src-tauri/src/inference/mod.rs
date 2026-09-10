pub mod audio;
pub mod image_codec;
pub mod stub;

use std::sync::Arc;

use async_trait::async_trait;

use crate::error::Result;
use crate::types::{GenParams, ModelSpec};

#[cfg(feature = "candle-cuda")]
pub mod candle_backend;
#[cfg(any(feature = "candle-cuda", feature = "tts-parler"))]
pub(crate) mod hub;
#[cfg(feature = "candle-cuda")]
pub mod image_candle;
#[cfg(feature = "tts-parler")]
pub mod tts_candle;
pub mod tts_parler;

#[async_trait]
pub trait TextBackend: Send + Sync + std::fmt::Debug {
    async fn stream_tokens(
        &self,
        prompt: &str,
        params: GenParams,
        tx: tokio::sync::mpsc::Sender<crate::types::ChatEvent>,
    ) -> Result<u32>;

    fn measured_vram_bytes(&self) -> Option<u64> {
        None
    }
}

#[async_trait]
pub trait ImageBackend: Send + Sync + std::fmt::Debug {
    async fn generate(&self, prompt: &str, width: u32, height: u32) -> Result<Vec<u8>>;

    fn measured_vram_bytes(&self) -> Option<u64> {
        None
    }
}

    #[async_trait]
    pub trait SpeechBackend: Send + Sync + std::fmt::Debug {
        async fn synthesize(&self, text: &str, voice: &str, force_cpu: bool) -> Result<Vec<u8>>;
    }

#[derive(Debug, Clone)]
pub struct BackendFactory {
    governor: Arc<crate::core::bandwidth::BandwidthGovernor>,
}

impl BackendFactory {
    pub fn new(governor: Arc<crate::core::bandwidth::BandwidthGovernor>) -> Self {
        Self { governor }
    }

    pub async fn create(&self, spec: &ModelSpec) -> Result<crate::core::engine::ModelHandle> {
        use crate::core::engine::ModelHandle;

        let lower = spec.id.to_ascii_lowercase();
        let is_real = lower.contains("qwen")
            || lower.contains("sd-")
            || lower.contains("sd1")
            || lower.contains("sd2")
            || lower.contains("turbo")
            || lower.contains("stable-diffusion")
            || lower.contains("parler")
            || lower.ends_with(".gguf")
            || lower.ends_with(".safetensors");

        #[cfg(feature = "candle-cuda")]
        if is_real {
            match spec.model_type {
                crate::types::ModelType::Llm | crate::types::ModelType::Embedding => {
                    let backend = candle_backend::CandleTextBackend::load(spec.id.clone()).await?;
                    return Ok(ModelHandle::Text(Arc::new(backend)));
                }
                crate::types::ModelType::Image => {
                    let backend = image_candle::CandleImageBackend::load(
                        spec.id.clone(),
                        self.governor.clone(),
                    )
                    .await?;
                    return Ok(ModelHandle::Image(Arc::new(backend)));
                }
                _ => {}
            }
        }

        #[cfg(feature = "tts-parler")]
        if is_real && spec.model_type == crate::types::ModelType::Tts {
            let backend = tts_candle::CandleSpeechBackend::load(spec.id.clone()).await?;
            return Ok(ModelHandle::Speech(Arc::new(backend)));
        }

        match spec.model_type {
            crate::types::ModelType::Llm | crate::types::ModelType::Embedding => {
                Ok(ModelHandle::Text(Arc::new(stub::StubTextBackend::new(
                    spec.id.clone(),
                ))))
            }
            crate::types::ModelType::Image => {
                Ok(ModelHandle::Image(Arc::new(stub::StubImageBackend::new(
                    spec.id.clone(),
                    self.governor.clone(),
                ))))
            }
            crate::types::ModelType::Tts => Ok(ModelHandle::Speech(Arc::new(
                stub::StubSpeechBackend::new(spec.id.clone()),
            ))),
            crate::types::ModelType::Asr => Err(crate::error::GabrielError::Backend(
                "ASR backends are not wired yet".into(),
            )),
        }
    }
}
