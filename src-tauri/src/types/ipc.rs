use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelType {
    Llm,
    Embedding,
    Image,
    Tts,
    Asr,
}

impl ModelType {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "llm" | "text" | "chat" | "language" => Some(Self::Llm),
            "embedding" | "embeddings" => Some(Self::Embedding),
            "image" | "diffusion" | "image-generation" => Some(Self::Image),
            "tts" | "speech" | "audio" | "text-to-speech" | "voice" => Some(Self::Tts),
            "asr" | "whisper" | "transcription" => Some(Self::Asr),
            _ => None,
        }
    }

    pub fn default_vram_budget(&self) -> u64 {
        match self {
            Self::Llm => 1_900_000_000,
            Self::Image => 1_600_000_000,
            Self::Tts => 0, // TTS runs on CPU, charges zero VRAM
            Self::Asr => 512 * 1024 * 1024,
            Self::Embedding => 256 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Residency {
    Gpu,
    Cpu,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSpec {
    pub id: String,
    #[serde(rename = "model_type")]
    pub model_type: ModelType,
    pub vram_bytes: u64,
}

impl ModelSpec {
    pub fn new(id: impl Into<String>, model_type: ModelType, vram_bytes: Option<u64>) -> Self {
        let id_str = id.into();
        let lower = id_str.to_ascii_lowercase();
        let is_real = lower.contains("qwen")
            || lower.contains("sd-")
            || lower.contains("sd1")
            || lower.contains("sd2")
            || lower.contains("turbo")
            || lower.contains("stable-diffusion")
            || lower.contains("parler")
            || lower.ends_with(".gguf")
            || lower.ends_with(".safetensors");

        let default_bytes = if is_real {
            model_type.default_vram_budget()
        } else {
            0
        };

        Self {
            id: id_str,
            model_type,
            vram_bytes: vram_bytes.unwrap_or(default_bytes),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelStatus {
    pub model_id: String,
    pub model_type: ModelType,
    pub residency: Residency,
    pub vram_bytes: u64,
    pub loaded_at_unix: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelRuntimeInfo {
    pub id: String,
    pub model_type: ModelType,
    pub residency: Residency,
    pub vram_bytes: u64,
    pub idle_secs: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TelemetrySnapshot {
    pub gpu_name: String,
    pub gpu_util_percent: f32,
    pub vram_total_bytes: u64,
    pub vram_used_bytes: u64,
    pub vram_high_watermark: f64,
    /// Engine-tracked VRAM (model weights only)
    pub engine_resident_bytes: u64,
    /// Untracked VRAM (KV cache, activations, fragmentation)
    pub vram_untracked_bytes: u64,
    /// Peak observed untracked memory
    pub vram_peak_untracked_bytes: u64,
    pub memory_bus_percent: f64,
    pub bandwidth_pressure: f64,
    pub ram_total_bytes: u64,
    pub ram_used_bytes: u64,
    pub cpu_usage_percent: f32,
    pub loaded_models: Vec<ModelRuntimeInfo>,
}

pub fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default()
}
