use std::path::PathBuf;

use crate::error::{GabrielError, Result};

pub const QWEN_GGUF_REPO: &str = "Qwen/Qwen2.5-3B-Instruct-GGUF";
pub const QWEN_GGUF_FILE: &str = "qwen2.5-3b-instruct-q4_k_m.gguf";
pub const QWEN_TOKENIZER_REPO: &str = "Qwen/Qwen2.5-3B-Instruct";
pub const SD15_REPO: &str = "stable-diffusion-v1-5/stable-diffusion-v1-5";
pub const SD15_CLIP_TOKENIZER_REPO: &str = "openai/clip-vit-base-patch32";
/// SD 2.x / SD-Turbo ships with CLIP ViT-L/14 tokenizer (49408 vocab)
pub const SD2_CLIP_TOKENIZER_REPO: &str = "openai/clip-vit-large-patch14";
pub const SD_TURBO_REPO: &str = "stabilityai/sd-turbo";
#[cfg(feature = "tts-parler")]
pub const PARLER_REPO: &str = "parler-tts/parler-tts-mini-v1";

pub fn pull_file(repo: &str, filename: &str) -> Result<PathBuf> {
    let api = hf_hub::api::sync::Api::new().map_err(|_| GabrielError::DownloadFailed {
        repo: repo.to_string(),
        filename: filename.to_string(),
    })?;
    api.model(repo.to_string())
        .get(filename)
        .map_err(|_| GabrielError::DownloadFailed {
            repo: repo.to_string(),
            filename: filename.to_string(),
        })
}

#[derive(Debug, Clone)]
pub struct SdV15Files {
    pub unet: PathBuf,
    pub vae: PathBuf,
    pub clip: PathBuf,
    pub tokenizer: PathBuf,
}

impl SdV15Files {
    pub fn pull() -> Result<Self> {
        Ok(Self {
            unet: pull_file(SD15_REPO, "unet/diffusion_pytorch_model.fp16.safetensors")?,
            vae: pull_file(SD15_REPO, "vae/diffusion_pytorch_model.fp16.safetensors")?,
            clip: pull_file(SD15_REPO, "text_encoder/model.fp16.safetensors")?,
            tokenizer: pull_file(SD15_CLIP_TOKENIZER_REPO, "tokenizer.json")?,
        })
    }
}

#[derive(Debug, Clone)]
pub struct SdTurboFiles {
    pub unet: PathBuf,
    pub vae: PathBuf,
    pub clip: PathBuf,
    pub tokenizer: PathBuf,
}

impl SdTurboFiles {
    pub fn pull() -> Result<Self> {
        tracing::info!("fetching SD-turbo fp16 components from HuggingFace Hub");
        Ok(Self {
            unet: pull_file(
                SD_TURBO_REPO,
                "unet/diffusion_pytorch_model.fp16.safetensors",
            )?,
            vae: pull_file(
                SD_TURBO_REPO,
                "vae/diffusion_pytorch_model.fp16.safetensors",
            )?,
            clip: pull_file(SD_TURBO_REPO, "text_encoder/model.fp16.safetensors")?,
            // SD-Turbo is based on SD 2.1; use the ViT-L/14 tokenizer (matches the CLIP weights)
            tokenizer: pull_file(SD2_CLIP_TOKENIZER_REPO, "tokenizer.json")?,
        })
    }
}

/// Query GPU used bytes via NVML (nvml-wrapper).
///
/// This avoids pulling in cudarc directly (which would conflict with
/// candle-core's pinned cudarc feature set) and gives us device-level
/// memory information regardless of CUDA context state.
#[cfg(feature = "nvml")]
pub fn gpu_used_bytes() -> Result<u64> {
    let nvml = nvml_wrapper::Nvml::init().map_err(|_| GabrielError::GpuQueryFailed)?;
    let device = nvml
        .device_by_index(0)
        .map_err(|_| GabrielError::GpuQueryFailed)?;
    let mem = device
        .memory_info()
        .map_err(|_| GabrielError::GpuQueryFailed)?;
    Ok(mem.used)
}

#[cfg(not(feature = "nvml"))]
pub fn gpu_used_bytes() -> Result<u64> {
    tracing::warn!("nvml feature disabled; gpu_used_bytes returning 0");
    Ok(0)
}
