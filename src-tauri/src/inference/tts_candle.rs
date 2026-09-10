use std::sync::{Arc, OnceLock};

use async_trait::async_trait;
use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::generation::LogitsProcessor;
use candle_transformers::models::parler_tts::{Config, Model};
use tokenizers::Tokenizer;

use crate::error::{GabrielError, Result};
use crate::inference::hub;
use crate::inference::{SpeechBackend, audio};

static CPU_ISOLATION_POOL: OnceLock<rayon::ThreadPool> = OnceLock::new();

fn cpu_isolation_pool() -> &'static rayon::ThreadPool {
    CPU_ISOLATION_POOL.get_or_init(|| {
        rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .build()
            .unwrap()
    })
}

struct SpeechState {
    cpu_model: Model,
    gpu_model: Option<(Model, Device)>,
    tokenizer: Tokenizer,
    sample_rate: u32,
}

pub struct CandleSpeechBackend {
    model_id: String,
    state: Arc<std::sync::Mutex<SpeechState>>,
}

impl std::fmt::Debug for CandleSpeechBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CandleSpeechBackend")
            .field("model_id", &self.model_id)
            .finish_non_exhaustive()
    }
}

fn description_for_voice(voice: &str) -> String {
    match voice.to_ascii_lowercase().as_str() {
        "alloy" => "A clear male speaker delivers the sentence at a calm pace. The recording is of very high quality.".into(),
        "nova" => "A female speaker delivers an expressive and animated speech with a moderate speed and pitch. The recording is of very high quality.".into(),
        "onyx" => "A deep male speaker reads the text slowly and with gravitas. The recording is of very high quality.".into(),
        "shimmer" => "A bright female speaker reads cheerfully and quickly. The recording is of very high quality.".into(),
        other => format!("A neutral speaker reads the following text. The recording is of very high quality. Voice style: {other}."),
    }
}

impl CandleSpeechBackend {
    pub async fn load(model_id: String) -> Result<Self> {
        tokio::task::spawn_blocking(move || Self::load_blocking(model_id))
            .await
            .map_err(|e| GabrielError::WeightLoadFailed {
                model_id: "parler-tts-mini".into(),
                detail: format!("{e}"),
            })?
    }

    fn load_blocking(model_id: String) -> Result<Self> {
        tracing::info!("candle-tts: fetching parler-tts-mini-v1 via hf-hub");
        let weights = hub::pull_file(hub::PARLER_REPO, "model.safetensors")?;
        let config_path = hub::pull_file(hub::PARLER_REPO, "config.json")?;
        let tokenizer_path = hub::pull_file(hub::PARLER_REPO, "tokenizer.json")?;

        let config: Config =
            serde_json::from_reader(std::fs::File::open(&config_path).map_err(|e| {
                GabrielError::WeightLoadFailed {
                    model_id: "parler-config".into(),
                    detail: e.to_string(),
                }
            })?)
            .map_err(|e| GabrielError::WeightLoadFailed {
                model_id: "parler-config".into(),
                detail: format!("{e}"),
            })?;
        let sample_rate = config.audio_encoder.sampling_rate;

        let tokenizer =
            Tokenizer::from_file(&tokenizer_path).map_err(|e| GabrielError::WeightLoadFailed {
                model_id: "parler-tokenizer".into(),
                detail: e.to_string(),
            })?;

        // Always load CPU model from mmaped safetensors for zero-cost CPU isolation
        let vb_cpu = unsafe {
            VarBuilder::from_mmaped_safetensors(&[weights.clone()], DType::F32, &Device::Cpu)?
        };
        let cpu_model = Model::new(&config, vb_cpu)?;

        // Try CUDA GPU model initialization if CUDA is available
        let gpu_model = match candle_core::Device::cuda_if_available(0) {
            Ok(d) => match unsafe {
                VarBuilder::from_mmaped_safetensors(&[weights], DType::F32, &d)
            } {
                Ok(vb) => match Model::new(&config, vb) {
                    Ok(m) => {
                        tracing::info!("candle-tts: CUDA GPU model loaded successfully");
                        Some((m, d))
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "failed to build GPU TTS model, using CPU only");
                        None
                    }
                },
                Err(e) => {
                    tracing::warn!(error = %e, "failed to map GPU TTS weights, using CPU only");
                    None
                }
            },
            Err(_) => None,
        };

        tracing::info!(
            sample_rate_hz = sample_rate,
            has_gpu = gpu_model.is_some(),
            "candle-tts: speech backend initialized"
        );

        Ok(Self {
            model_id,
            state: Arc::new(std::sync::Mutex::new(SpeechState {
                cpu_model,
                gpu_model,
                tokenizer,
                sample_rate,
            })),
        })
    }

    fn synthesize_cpu_isolated(
        state: Arc<std::sync::Mutex<SpeechState>>,
        text: String,
        voice: String,
    ) -> Result<Vec<u8>> {
        let pool = cpu_isolation_pool();
        pool.install(|| match state.lock() {
            Ok(mut s) => Self::synthesize_on_cpu(&mut s, &text, &voice),
            Err(_) => Err(GabrielError::Backend("speech state lock poisoned".into())),
        })
    }

    fn synthesize_on_cpu(state: &mut SpeechState, text: &str, voice: &str) -> Result<Vec<u8>> {
        let description = description_for_voice(voice);

        let description_tokens = state
            .tokenizer
            .encode(description, true)
            .map_err(|_| {
                GabrielError::InvalidRequest("TTS description tokenization failed".into())
            })?
            .get_ids()
            .to_vec();
        let prompt_tokens = state
            .tokenizer
            .encode(text, true)
            .map_err(|_| GabrielError::InvalidRequest("TTS input tokenization failed".into()))?
            .get_ids()
            .to_vec();

        let description_tensor =
            Tensor::new(&description_tokens[..], &Device::Cpu)?.unsqueeze(0)?;
        let prompt_tensor = Tensor::new(&prompt_tokens[..], &Device::Cpu)?.unsqueeze(0)?;

        let logits_processor = LogitsProcessor::new(0, Some(0.0), None);
        // In debug builds use fewer steps so the test suite finishes in seconds
        // rather than 10+ minutes; release builds use full 250 steps.
        let max_steps: usize = if cfg!(debug_assertions) { 50 } else { 250 };

        let codes = state.cpu_model.generate(
            &prompt_tensor,
            &description_tensor,
            logits_processor,
            max_steps,
        )?;
        let codes = codes.unsqueeze(0)?.to_dtype(DType::I64)?;

        let pcm = state.cpu_model.audio_encoder.decode_codes(&codes)?;
        let pcm = pcm
            .i((0, 0))?
            .to_vec1::<f32>()
            .map_err(|_| GabrielError::Backend("failed to read decoded PCM samples".into()))?;

        Ok(wav_from_pcm(pcm, state.sample_rate))
    }

    fn synthesize_on_gpu(state: &mut SpeechState, text: &str, voice: &str) -> Result<Vec<u8>> {
        let Some((ref mut model, ref device)) = state.gpu_model else {
            return Self::synthesize_on_cpu(state, text, voice);
        };

        let description = description_for_voice(voice);

        let description_tokens = state
            .tokenizer
            .encode(description, true)
            .map_err(|_| {
                GabrielError::InvalidRequest("TTS description tokenization failed".into())
            })?
            .get_ids()
            .to_vec();
        let prompt_tokens = state
            .tokenizer
            .encode(text, true)
            .map_err(|_| GabrielError::InvalidRequest("TTS input tokenization failed".into()))?
            .get_ids()
            .to_vec();

        let description_tensor =
            Tensor::new(&description_tokens[..], device)?.unsqueeze(0)?;
        let prompt_tensor = Tensor::new(&prompt_tokens[..], device)?.unsqueeze(0)?;

        let logits_processor = LogitsProcessor::new(0, Some(0.0), None);
        let max_steps: usize = if cfg!(debug_assertions) { 50 } else { 250 };

        let codes = model.generate(
            &prompt_tensor,
            &description_tensor,
            logits_processor,
            max_steps,
        )?;
        let codes = codes.unsqueeze(0)?.to_dtype(DType::I64)?.to_device(&Device::Cpu)?;

        let pcm = model.audio_encoder.decode_codes(&codes)?;
        let pcm = pcm
            .i((0, 0))?
            .to_vec1::<f32>()
            .map_err(|_| GabrielError::Backend("failed to read decoded PCM samples".into()))?;

        Ok(wav_from_pcm(pcm, state.sample_rate))
    }
}

fn wav_from_pcm(mut pcm: Vec<f32>, sample_rate: u32) -> Vec<u8> {
    let peak = pcm.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    if peak > 1e-6 {
        let gain = 0.9 / peak;
        for s in &mut pcm {
            *s *= gain;
        }
    }
    let mut wav = audio::WavBuilder::new(sample_rate);
    for s in pcm {
        let clamped = s.clamp(-1.0, 1.0);
        wav.push_sample((clamped * i16::MAX as f32) as i16);
    }
    wav.finish()
}

#[async_trait]
impl SpeechBackend for CandleSpeechBackend {
    async fn synthesize(&self, text: &str, voice: &str, force_cpu: bool) -> Result<Vec<u8>> {
        let owned_text = text.to_string();
        let owned_voice = voice.to_string();
        let state = self.state.clone();

        let has_gpu = {
            let guard = state.lock().unwrap();
            guard.gpu_model.is_some()
        };

        if force_cpu || !has_gpu {
            tokio::task::spawn_blocking(move || {
                Self::synthesize_cpu_isolated(state, owned_text, owned_voice)
            })
            .await
            .map_err(|_| GabrielError::Backend("speech worker terminated abnormally".into()))?
        } else {
            tokio::task::spawn_blocking(move || match state.lock() {
                Ok(mut s) => Self::synthesize_on_gpu(&mut s, &owned_text, &owned_voice),
                Err(_) => Err(GabrielError::Backend("speech state lock poisoned".into())),
            })
            .await
            .map_err(|_| GabrielError::Backend("speech worker terminated abnormally".into()))?
        }
    }
}

impl CandleSpeechBackend {
    #[allow(dead_code)]
    pub async fn synthesize_on_device(
        &self,
        text: &str,
        voice: &str,
        target_device: Device,
    ) -> Result<Vec<u8>> {
        let force_cpu = matches!(target_device, Device::Cpu);
        self.synthesize(text, voice, force_cpu).await
    }
}
