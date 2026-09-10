use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use candle_core::{DType, Device, IndexOp, Tensor};
use candle_nn::Module;
use candle_transformers::models::stable_diffusion::{self, StableDiffusionConfig};
use tokenizers::Tokenizer;

use crate::core::bandwidth::BandwidthGovernor;
use crate::error::{GabrielError, Result};
use crate::inference::hub;
use crate::inference::ImageBackend;

const STEPS_DEFAULT: usize = 12;
const STEPS_TURBO: usize = 4;
const GUIDANCE_SCALE_DEFAULT: f64 = 7.5;
const GUIDANCE_SCALE_TURBO: f64 = 1.0;
const VAE_SCALE: f64 = 0.18215;
const SIZE: usize = 512;

struct Pipeline {
    clip: stable_diffusion::clip::ClipTextTransformer,
    unet: stable_diffusion::unet_2d::UNet2DConditionModel,
    vae: stable_diffusion::vae::AutoEncoderKL,
    config: StableDiffusionConfig,
    tokenizer: Tokenizer,
    cpu_device: Device,
    gpu_device: Device,
    dtype: DType,
    steps: usize,
    guidance_scale: f64,
}

pub struct CandleImageBackend {
    model_id: String,
    pipeline: Arc<Mutex<Pipeline>>,
    governor: Arc<BandwidthGovernor>,
    resident_bytes: u64,
}

impl std::fmt::Debug for CandleImageBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CandleImageBackend")
            .field("model_id", &self.model_id)
            .field("resident_bytes", &self.resident_bytes)
            .finish_non_exhaustive()
        }
}

impl CandleImageBackend {
    pub async fn load(model_id: String, governor: Arc<BandwidthGovernor>) -> Result<Self> {
        let tag = model_id.clone();
        tokio::task::spawn_blocking(move || Self::load_blocking(model_id, governor))
            .await
            .map_err(|e| GabrielError::WeightLoadFailed {
                model_id: tag,
                detail: format!("{e}"),
            })?
    }

    fn load_blocking(model_id: String, governor: Arc<BandwidthGovernor>) -> Result<Self> {
        let is_turbo = model_id.contains("turbo");
        let (steps, guidance_scale) = if is_turbo {
            (STEPS_TURBO, GUIDANCE_SCALE_TURBO)
        } else {
            (STEPS_DEFAULT, GUIDANCE_SCALE_DEFAULT)
        };

        tracing::info!(
            model_id = %model_id,
            is_turbo,
            "candle-image: loading diffusion components (CLIP+VAE on CPU, UNet on GPU)"
        );

        let (files_unet, files_vae, files_clip, files_tokenizer) = if is_turbo {
            match hub::SdTurboFiles::pull() {
                Ok(f) => (f.unet, f.vae, f.clip, f.tokenizer),
                Err(e) => {
                    tracing::warn!(error = %e, "failed to pull SD-Turbo files, falling back to SD1.5");
                    let f = hub::SdV15Files::pull()?;
                    (f.unet, f.vae, f.clip, f.tokenizer)
                }
            }
        } else {
            let f = hub::SdV15Files::pull()?;
            (f.unet, f.vae, f.clip, f.tokenizer)
        };

        let before = hub::gpu_used_bytes().unwrap_or(0);

        let gpu_device = Device::cuda_if_available(0).map_err(|_| GabrielError::GpuQueryFailed)?;
        let cpu_device = Device::Cpu;
        let dtype = DType::F16;

        // SD-Turbo is built on SD 2.1 (CLIP ViT-L/14, 1024-dim cross-attention)
        // SD 1.5 uses CLIP ViT-B/32 or ViT-L/14-patch14 with 768-dim projection
        let config = if is_turbo {
            StableDiffusionConfig::v2_1(None, Some(SIZE), Some(SIZE))
        } else {
            StableDiffusionConfig::v1_5(None, Some(SIZE), Some(SIZE))
        };

        let tokenizer =
            Tokenizer::from_file(&files_tokenizer).map_err(|e| {
                GabrielError::WeightLoadFailed {
                    model_id: "sd-clip-tokenizer".into(),
                    detail: e.to_string(),
                }
            })?;

        // CLIP Text Encoder: CPU (F32)
        tracing::info!("candle-image: loading CLIP text encoder on CPU");
        let clip = build_clip(&config, &files_clip, &cpu_device)?;

        // VAE Decoder: CPU (F32 for fast native SIMD CPU decode)
        tracing::info!("candle-image: loading VAE decoder on CPU (F32)");
        let vae = config
            .build_vae(&files_vae, &cpu_device, DType::F32)
            .map_err(|e| GabrielError::WeightLoadFailed {
                model_id: "sd-vae".into(),
                detail: format!("{e}"),
            })?;

        // UNet Denoising Model: GPU (only GPU-resident component)
        tracing::info!("candle-image: loading UNet denoising model on GPU");
        let unet = config
            .build_unet(&files_unet, &gpu_device, 4, false, dtype)
            .map_err(|e| GabrielError::WeightLoadFailed {
                model_id: "sd-unet".into(),
                detail: format!("{e}"),
            })?;
        let after = hub::gpu_used_bytes().unwrap_or(before);

        let measured = after.saturating_sub(before);
        let resident_bytes = if measured >= 1_000_000_000 && measured <= 4_000_000_000 {
            measured
        } else {
            // UNet SD1.5/Turbo fp16 weights are ~1.6 GB
            1_600_000_000
        };

        tracing::info!(
            measured_vram_bytes = resident_bytes,
            "candle-image: SD UNet resident on GPU (CLIP+VAE on CPU)"
        );

        Ok(Self {
            model_id,
            pipeline: Arc::new(Mutex::new(Pipeline {
                clip,
                unet,
                vae,
                config,
                tokenizer,
                cpu_device,
                gpu_device,
                dtype,
                steps,
                guidance_scale,
            })),
            governor,
            resident_bytes,
        })
    }

    fn generate_blocking(
        pipeline: &Mutex<Pipeline>,
        governor: &BandwidthGovernor,
        prompt: &str,
    ) -> Result<Vec<u8>> {
        let pipe = pipeline
            .lock()
            .map_err(|_| GabrielError::Backend("image pipeline lock poisoned".into()))?;

        let pad_id = *pipe
            .tokenizer
            .get_vocab(true)
            .get("<|endoftext|>")
            .ok_or(GabrielError::Backend("CLIP tokenizer missing pad token".into()))?;
        let max_pos = pipe.config.clip.max_position_embeddings;

        let mut tokens = pipe
            .tokenizer
            .encode(prompt, true)
            .map_err(|_| GabrielError::InvalidRequest("prompt tokenization failed".into()))?
            .get_ids()
            .to_vec();
        tokens.truncate(max_pos);
        while tokens.len() < max_pos {
            tokens.push(pad_id);
        }

        // Step 1: Text Encoding on CPU
        tracing::debug!("SD: encoding prompt on CPU");
        let cond_tokens =
            Tensor::new(tokens.as_slice(), &pipe.cpu_device)?.unsqueeze(0)?;
        let cond = pipe.clip.forward(&cond_tokens)?;
        let cond_embeddings_cpu = cond.to_dtype(pipe.dtype)?;
        let cond_embeddings = cond_embeddings_cpu.to_device(&pipe.gpu_device)?;

        let text_embeddings = if (pipe.guidance_scale - 1.0).abs() < f64::EPSILON {
            cond_embeddings
        } else {
            let uncond_tokens =
                Tensor::new(&vec![pad_id; max_pos][..], &pipe.cpu_device)?.unsqueeze(0)?;
            let uncond = pipe.clip.forward(&uncond_tokens)?;
            let text_embeddings_cpu = Tensor::cat(&[&uncond, &cond], 0)?.to_dtype(pipe.dtype)?;
            text_embeddings_cpu.to_device(&pipe.gpu_device)?
        };

        let mut scheduler = pipe.config.build_scheduler(pipe.steps)?;
        let timesteps = scheduler.timesteps().to_vec();

        // Step 2: Denoising Loop on GPU
        tracing::debug!("SD: initializing latents on GPU");
        let latents_shape = (1usize, 4usize, SIZE / 8, SIZE / 8);
        let seed = seed_for(prompt);
        let latents = Tensor::randn(0f32, 1f32, latents_shape, &pipe.gpu_device)?
            .affine(scheduler.init_noise_sigma(), 0.0)?
            .to_dtype(pipe.dtype)?;

        let _ = seed;
        let mut latents = latents;
        for (step_index, &timestep) in timesteps.iter().enumerate() {
            let yield_time = governor.standard_yield();
            if !yield_time.is_zero() {
                tracing::debug!(
                    step = step_index + 1,
                    yield_ms = yield_time.as_millis() as u64,
                    "diffusion yielding memory bus to interactive traffic"
                );
                std::thread::sleep(yield_time);
            }

            let latent_model_input =
                scheduler.scale_model_input(latents.clone(), timestep)?;

            let noise_pred = if (pipe.guidance_scale - 1.0).abs() < f64::EPSILON {
                pipe.unet
                    .forward(&latent_model_input, timestep as f64, &text_embeddings)?
            } else {
                let input = Tensor::cat(&[&latent_model_input, &latent_model_input], 0)?;
                let pred = pipe.unet
                    .forward(&input, timestep as f64, &text_embeddings)?;
                let chunks = pred.chunk(2, 0)?;
                (&chunks[0] + ((&chunks[1] - &chunks[0])? * pipe.guidance_scale)?)?
            };

            latents = scheduler.step(&noise_pred, timestep, &latents)?;
            tracing::debug!(
                step = step_index + 1,
                total = pipe.steps,
                "diffusion step complete"
            );
        }

        // Step 3: VAE Decode on CPU
        // Transfer final latents back to system memory in F32
        tracing::debug!("SD: transferring latents to CPU for VAE decode");
        let latents_cpu = latents.to_device(&pipe.cpu_device)?.to_dtype(DType::F32)?;
        let images = pipe.vae.decode(&(latents_cpu.affine(VAE_SCALE, 0.0))?)?;
        let pixels = ((images.affine(0.5, 0.0))? + 0.5)?
            .clamp(0f32, 1f32)?
            .affine(255., 0.)?
            .to_dtype(DType::U8)?;
        encode_png(pixels.i(0)?)
    }
}

fn build_clip(
    config: &StableDiffusionConfig,
    weights: &PathBuf,
    device: &Device,
) -> Result<stable_diffusion::clip::ClipTextTransformer> {
    stable_diffusion::build_clip_transformer(&config.clip, weights, device, DType::F32)
        .map_err(|e| GabrielError::WeightLoadFailed {
            model_id: "sd-clip".into(),
            detail: e.to_string(),
        })
}

fn seed_for(prompt: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    prompt.hash(&mut h);
    h.finish()
        ^ std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(7)
}

fn encode_png(image_chw_u8: Tensor) -> Result<Vec<u8>> {
    let (channels, height, width) = match image_chw_u8.dims3() {
        Ok(d) => d,
        Err(_) => return Err(GabrielError::Backend("unexpected VAE output shape".into())),
    };
    if channels != 3 {
        return Err(GabrielError::Backend("VAE output must be RGB".into()));
    }
    let data = image_chw_u8
        .flatten_all()?
        .to_vec1::<u8>()
        .map_err(|_| GabrielError::Backend("failed to read decoded pixels".into()))?;

    let mut imgbuf = image::RgbImage::new(width as u32, height as u32);
    for y in 0..height {
        for x in 0..width {
            let r = data[y * width + x];
            let g = data[(height + y) * width + x];
            let b = data[(2 * height + y) * width + x];
            imgbuf.put_pixel(x as u32, y as u32, image::Rgb([r, g, b]));
        }
    }

    let mut out = Vec::new();
    let cursor = std::io::Cursor::new(&mut out);
    image::DynamicImage::ImageRgb8(imgbuf)
        .write_to(cursor, image::ImageFormat::Png)
        .map_err(|_| GabrielError::Backend("PNG encoding failed".into()))?;
    Ok(out)
}

#[async_trait]
impl ImageBackend for CandleImageBackend {
    async fn generate(&self, prompt: &str, _width: u32, _height: u32) -> Result<Vec<u8>> {
        let owned_prompt = prompt.to_string();
        let pipeline = self.pipeline.clone();
        let governor = self.governor.clone();

        tokio::task::spawn_blocking(move || {
            Self::generate_blocking(&pipeline, &governor, &owned_prompt)
        })
        .await
        .map_err(|_| GabrielError::Backend("diffusion worker terminated abnormally".into()))?
    }

    fn measured_vram_bytes(&self) -> Option<u64> {
        Some(self.resident_bytes)
    }
}
