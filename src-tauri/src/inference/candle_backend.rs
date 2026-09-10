use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::error::{GabrielError, Result};
use crate::inference::hub;
use crate::inference::TextBackend;
use crate::types::{ChatEvent, GenParams};

use candle_core::quantized::gguf_file;
use candle_core::{Device, Tensor};
use candle_transformers::generation::{LogitsProcessor, Sampling};
use candle_transformers::models::quantized_qwen2::ModelWeights;
use tokenizers::Tokenizer;

const IM_END: &str = "<|im_end|>";
const REPEAT_PENALTY: f32 = 1.1;
const REPEAT_LAST_N: usize = 64;
const TOP_P: f64 = 0.8;

struct TextState {
    model: ModelWeights,
    tokenizer: Tokenizer,
    device: Device,
    eos_token: u32,
}

/// Streaming decoder ported from candle's TokenOutputStream: decodes the full
/// suffix window each step so multi-token BPE pieces never duplicate.
struct StreamDecoder {
    tokenizer: Tokenizer,
    tokens: Vec<u32>,
    prev_index: usize,
    current_index: usize,
}

impl StreamDecoder {
    fn new(tokenizer: &Tokenizer) -> Self {
        Self {
            tokenizer: tokenizer.clone(),
            tokens: Vec::new(),
            prev_index: 0,
            current_index: 0,
        }
    }

    fn decode(&self, tokens: &[u32]) -> Option<String> {
        self.tokenizer.decode(tokens, true).ok()
    }

    fn next_token(&mut self, token: u32) -> Option<String> {
        let prev_text = if self.tokens.is_empty() {
            String::new()
        } else {
            self.decode(&self.tokens[self.prev_index..self.current_index])?
        };
        self.tokens.push(token);
        let text = self.decode(&self.tokens[self.prev_index..])?;
        if text.len() > prev_text.len() && text.chars().last()?.is_alphanumeric() {
            let piece = text.split_at(prev_text.len()).1.to_string();
            self.prev_index = self.current_index;
            self.current_index = self.tokens.len();
            Some(piece)
        } else {
            None
        }
    }

    fn decode_rest(&self) -> Option<String> {
        let prev_text = if self.tokens.is_empty() {
            String::new()
        } else {
            self.decode(&self.tokens[self.prev_index..self.current_index])?
        };
        let text = self.decode(&self.tokens[self.prev_index..])?;
        if text.len() > prev_text.len() {
            Some(text.split_at(prev_text.len()).1.to_string())
        } else {
            None
        }
    }
}

pub struct CandleTextBackend {
    model_id: String,
    state: Arc<Mutex<TextState>>,
    resident_bytes: u64,
}

impl std::fmt::Debug for CandleTextBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CandleTextBackend")
            .field("model_id", &self.model_id)
            .field("resident_bytes", &self.resident_bytes)
            .finish_non_exhaustive()
    }
}

impl CandleTextBackend {
    pub async fn load(model_id: String) -> Result<Self> {
        tokio::task::spawn_blocking(move || Self::load_blocking(model_id))
            .await
            
            .map_err(|e| GabrielError::WeightLoadFailed {
                model_id: "qwen2.5-3b".into(),
                detail: e.to_string(),
            })?
    }

    fn load_blocking(model_id: String) -> Result<Self> {
        let load_tag = model_id.clone();
        tracing::info!("candle-llm: fetching Qwen2.5-3B-Instruct GGUF (Q4_K_M) via hf-hub");
        let gguf_path = hub::pull_file(hub::QWEN_GGUF_REPO, hub::QWEN_GGUF_FILE)?;
        let tokenizer_path = hub::pull_file(hub::QWEN_TOKENIZER_REPO, "tokenizer.json")?;

        let before = hub::gpu_used_bytes()?;
        let device = Device::cuda_if_available(0)
            .map_err(|_| GabrielError::GpuQueryFailed)?;

        let mut file = std::fs::File::open(&gguf_path)
            
            .map_err(|e| GabrielError::WeightLoadFailed { model_id: load_tag, detail: format!("{e}") })?;
        let content = gguf_file::Content::read(&mut file).map_err(|e| {
            tracing::error!("gguf content read failed: {e}");
            GabrielError::WeightLoadFailed { model_id: model_id.clone(), detail: format!("{e}") }
        })?;
        let tensor_count = content.tensor_infos.len();
        let mut model = ModelWeights::from_gguf(content, &mut file, &device).map_err(|e| {
            tracing::error!("weight build failed: {e}");
            GabrielError::WeightLoadFailed { model_id: model_id.clone(), detail: format!("{e}") }
        })?;
        let after = hub::gpu_used_bytes()?;

        let tokenizer = Tokenizer::from_file(&tokenizer_path)
            
            .map_err(|e| GabrielError::WeightLoadFailed { model_id: "qwen-tokenizer".into(), detail: format!("{e}") })?;
        let eos_token = *tokenizer
            .get_vocab(true)
            .get(IM_END)
            .ok_or_else(|| GabrielError::WeightLoadFailed {
                model_id: "qwen-tokenizer".into(),
                detail: "missing <|im_end|> in vocab".into(),
            })?;

        let resident_bytes = after.saturating_sub(before);
        tracing::info!(
            tensors = tensor_count,
            measured_vram_bytes = resident_bytes,
            "candle-llm: weights resident on GPU (measured via cudarc mem_get_info delta)"
        );

        model.clear_kv_cache();
        Ok(Self {
            model_id,
            state: Arc::new(Mutex::new(TextState {
                model,
                tokenizer,
                device,
                eos_token,
            })),
            resident_bytes,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn generate_blocking(
        state: &mut TextState,
        prompt: &str,
        params: GenParams,
        bridge: mpsc::UnboundedSender<ChatEvent>,
        cancel: Arc<AtomicBool>,
    ) -> std::result::Result<u32, String> {
        let formatted = format!("<|im_start|>user\n{prompt}{IM_END}\n<|im_start|>assistant\n");
        let prompt_tokens = state
            .tokenizer
            .encode(formatted, true)
            .map_err(|_| "prompt tokenization failed".to_string())?
            .get_ids()
            .to_vec();

        state.model.clear_kv_cache();

        let temperature = params.temperature as f64;
        let sampling = if temperature <= 0.01 {
            Sampling::ArgMax
        } else {
            Sampling::TopP {
                p: TOP_P,
                temperature,
            }
        };
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos() as u64 ^ d.as_secs())
            .unwrap_or(42);
        let mut logits_processor = LogitsProcessor::from_sampling(seed, sampling);

        let mut all_tokens: Vec<u32> = Vec::with_capacity(params.max_tokens as usize);
        let mut decoder = StreamDecoder::new(&state.tokenizer);
        let prompt_len = prompt_tokens.len();
        let mut generated = 0u32;
        let max_tokens = params.max_tokens.max(1);

        let sample_result = (|| -> candle_core::Result<u32> {
            let input = Tensor::new(&prompt_tokens[..], &state.device)?.unsqueeze(0)?;
            let logits = state.model.forward(&input, 0)?.squeeze(0)?;
            logits_processor.sample(&logits)
        })();

        let mut next_token =
            sample_result.map_err(|e| format!("prompt forward failed: {e}"))?;

        let mut error: Option<String> = None;
        loop {
            if cancel.load(Ordering::Relaxed) || generated >= max_tokens {
                break;
            }

            if next_token == state.eos_token {
                break;
            }

            all_tokens.push(next_token);
            generated += 1;

            if let Some(piece) = decoder.next_token(next_token) {
                if !piece.is_empty() && bridge.send(ChatEvent::Token(piece)).is_err() {
                    break;
                }
            }

            let input = match Tensor::new(&[next_token], &state.device).and_then(|t| t.unsqueeze(0))
            {
                Ok(t) => t,
                Err(e) => {
                    error = Some(format!("decode step failed: {e}"));
                    break;
                }
            };
            let position = prompt_len + all_tokens.len() - 1;
            let logits = state.model.forward(&input, position).and_then(|l| l.squeeze(0));
            let logits = match logits.and_then(|l| {
                let start_at = all_tokens.len().saturating_sub(REPEAT_LAST_N);
                candle_transformers::utils::apply_repeat_penalty(
                    &l,
                    REPEAT_PENALTY,
                    &all_tokens[start_at..],
                )
            }) {
                Ok(l) => l,
                Err(e) => {
                    error = Some(format!("forward failed: {e}"));
                    break;
                }
            };

            match logits_processor.sample(&logits) {
                Ok(t) => next_token = t,
                Err(e) => {
                    error = Some(format!("sampling failed: {e}"));
                    break;
                }
            }
        }

        if error.is_some() {
            return Err(error.unwrap());
        }
        if let Some(rest) = decoder.decode_rest().filter(|r| !r.trim().is_empty()) {
            if bridge.send(ChatEvent::Token(rest)).is_err() {
                return Ok(generated);
            }
        }
        Ok(generated)
    }
}


#[async_trait]
impl TextBackend for CandleTextBackend {
    async fn stream_tokens(
        &self,
        prompt: &str,
        params: GenParams,
        events: mpsc::Sender<ChatEvent>,
    ) -> Result<u32> {
        let cancel = Arc::new(AtomicBool::new(false));
        let (bridge_tx, mut bridge_rx) = mpsc::unbounded_channel::<ChatEvent>();

        let state_arc = self.state.clone();
        let cancel_for_worker = cancel.clone();
        let owned_prompt = prompt.to_string();
        let worker = tokio::task::spawn_blocking(move || {
            let outcome = state_arc
                .lock()
                .map_err(|_| "inference state lock poisoned".to_string())
                .and_then(|mut state| {
                    Self::generate_blocking(&mut state, &owned_prompt, params, bridge_tx.clone(), cancel_for_worker)
                });
            drop(bridge_tx);
            outcome
        });

        while let Some(ev) = bridge_rx.recv().await {
            if events.send(ev).await.is_err() {
                cancel.store(true, Ordering::Relaxed);
                break;
            }
        }
        let generated = worker
            .await
            .map_err(|_| GabrielError::Backend("generation worker terminated abnormally".into()))?
            .map_err(GabrielError::Backend)?;

        tracing::debug!(model = %self.model_id, tokens = generated, "candle-llm stream complete");
        Ok(generated)
    }

    fn measured_vram_bytes(&self) -> Option<u64> {
        Some(self.resident_bytes)
    }
}
