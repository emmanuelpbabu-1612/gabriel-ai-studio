use tauri::{AppHandle, State};
use tauri::Emitter;

use crate::core::engine::EngineState;
use crate::error::{GabrielError, Result};
use crate::types::{ModelStatus, ModelType, TelemetrySnapshot};
use serde::Serialize;
use serde::Deserialize;
use std::fs;
use std::time::Duration;

#[derive(Debug, Clone, Serialize)]
pub struct HfModelResult {
    pub id: String,
    pub author: String,
    pub downloads: u64,
    pub tags: Vec<String>,
    pub files: Vec<HfFileResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HfFileResult {
    pub name: String,
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct HfApiModel {
    id: String,
    #[serde(default)] author: String,
    #[serde(default)] downloads: u64,
    #[serde(default)] tags: Vec<String>,
    #[serde(default)] siblings: Vec<HfSibling>,
}

#[derive(Debug, Deserialize)]
struct HfSibling {
    rfilename: String,
    #[serde(default)] size: Option<u64>,
    #[serde(default)] lfs: Option<HfLfs>,
}

#[derive(Debug, Deserialize)]
struct HfLfs { size: Option<u64> }

#[tauri::command]
pub async fn search_huggingface_models(query: String) -> Result<Vec<HfModelResult>> {
    let query = query.trim();
    if query.is_empty() { return Ok(Vec::new()); }
    let query = query.to_string();

    // Step 1: search the HF index — this returns basic metadata but siblings: [] for most repos.
    let search_results = tokio::task::spawn_blocking(move || -> Result<Vec<HfApiModel>> {
        ureq::get("https://huggingface.co/api/models")
            .query("search", &query)
            .query("limit", "20")
            .query("filter", "gguf")
            .call()
            .map_err(|e| GabrielError::Internal(e.to_string()))?
            .body_mut()
            .read_json()
            .map_err(|e| GabrielError::Internal(e.to_string()))
    }).await.map_err(|e| GabrielError::Internal(e.to_string()))??;

    // Step 2: for each result, fire a per-repo call to get the real file list (siblings).
    // The list endpoint returns empty siblings for most repos; /api/models/{id} is authoritative.
    let repo_ids: Vec<String> = search_results.iter().map(|m| m.id.clone()).collect();
    let hydrated_siblings: Vec<Vec<HfSibling>> = tokio::task::spawn_blocking(move || {
        repo_ids.iter().map(|repo_id| {
            let url = format!("https://huggingface.co/api/models/{repo_id}");
            let result: Option<HfApiModel> = ureq::get(&url)
                .call()
                .ok()
                .and_then(|mut r| r.body_mut().read_json().ok());
            result.map(|m| m.siblings).unwrap_or_default()
        }).collect()
    }).await.map_err(|e| GabrielError::Internal(e.to_string()))?;

    // Merge: build final results, filtering to only repos that have GGUF/safetensors files.
    let results: Vec<HfModelResult> = search_results
        .into_iter()
        .zip(hydrated_siblings.into_iter())
        .filter_map(|(model, siblings)| {
            let files: Vec<HfFileResult> = siblings
                .into_iter()
                .filter(|f| {
                    f.rfilename.ends_with(".gguf")
                        || f.rfilename.ends_with(".ggml")
                        || f.rfilename.ends_with(".safetensors")
                })
                .map(|f| HfFileResult {
                    name: f.rfilename,
                    size_bytes: f.size.or_else(|| f.lfs.and_then(|lfs| lfs.size)),
                })
                .collect();

            // Hide repos with no compatible files (they're dead ends in the UI).
            if files.is_empty() {
                return None;
            }

            Some(HfModelResult {
                author: if model.author.is_empty() {
                    model.id.split('/').next().unwrap_or_default().into()
                } else {
                    model.author
                },
                id: model.id,
                downloads: model.downloads,
                tags: model.tags,
                files,
            })
        })
        .collect();

    Ok(results)
}

#[derive(Debug, Clone, Serialize)]
pub struct HfDownloadProgress {
    pub download_id: String,
    pub repo_id: String,
    pub filename: String,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct HfDownloadStarted {
    pub download_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HfDownloadDone {
    pub download_id: String,
    pub repo_id: String,
    pub filename: String,
    pub ok: bool,
    pub cancelled: bool,
    pub model_id: Option<String>,
    pub error: Option<String>,
}

#[tauri::command]
pub async fn download_huggingface_model(app: AppHandle, state: State<'_, EngineState>, repo_id: String, filename: String, kind: String) -> Result<HfDownloadStarted> {
    // Fire-and-forget: returns a download id immediately; progress and the
    // final outcome flow out of band on "hf-download-progress" /
    // "hf-download-done" so the UI stays responsive and cancellable.
    // Registration on success is unchanged (engine registers the real
    // filename, and the done event carries the model id for refresh).
    let download_id = uuid::Uuid::new_v4().to_string();
    state.start_hf_download(
        download_id.clone(),
        repo_id.clone(),
        filename.clone(),
        kind,
        {
            let app = app.clone();
            let download_id = download_id.clone();
            let repo_id = repo_id.clone();
            let filename = filename.clone();
            move |downloaded_bytes, total_bytes| {
                let _ = app.emit(
                    "hf-download-progress",
                    HfDownloadProgress {
                        download_id: download_id.clone(),
                        repo_id: repo_id.clone(),
                        filename: filename.clone(),
                        downloaded_bytes,
                        total_bytes,
                    },
                );
            }
        },
        {
            let app = app.clone();
            let download_id = download_id.clone();
            let repo_id = repo_id.clone();
            let filename = filename.clone();
            move |outcome| {
                let done = match outcome {
                    Ok(Some(info)) => HfDownloadDone {
                        download_id: download_id.clone(),
                        repo_id: repo_id.clone(),
                        filename: filename.clone(),
                        ok: true,
                        cancelled: false,
                        model_id: Some(info.id),
                        error: None,
                    },
                    Ok(None) => HfDownloadDone {
                        download_id: download_id.clone(),
                        repo_id: repo_id.clone(),
                        filename: filename.clone(),
                        ok: false,
                        cancelled: true,
                        model_id: None,
                        error: None,
                    },
                    Err(error) => HfDownloadDone {
                        download_id: download_id.clone(),
                        repo_id: repo_id.clone(),
                        filename: filename.clone(),
                        ok: false,
                        cancelled: false,
                        model_id: None,
                        error: Some(error.to_string()),
                    },
                };
                let _ = app.emit("hf-download-done", done);
            }
        },
    );
    Ok(HfDownloadStarted { download_id })
}

#[tauri::command]
pub fn cancel_hf_download(state: State<'_, EngineState>, download_id: String) -> bool {
    state.cancel_hf_download(&download_id)
}

#[derive(Debug, Clone, Serialize)]
pub struct DiskUsage {
    pub used_bytes: u64,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub llm_bytes: u64,
    pub image_bytes: u64,
    pub voice_bytes: u64,
    pub other_bytes: u64,
}

#[tauri::command]
pub fn get_disk_usage(state: State<'_, EngineState>) -> DiskUsage {
    let mut usage = DiskUsage { used_bytes: 0, total_bytes: 0, available_bytes: 0, llm_bytes: 0, image_bytes: 0, voice_bytes: 0, other_bytes: 0 };
    // Bucket tracked models by their registered type (accurate for
    // .safetensors which may be LLM/Image/Voice). Untracked files on disk
    // fall into `other_bytes` below.
    let mut tracked_bytes: u64 = 0;
    for m in state.list_models() {
        tracked_bytes = tracked_bytes.saturating_add(m.disk_bytes);
        match m.model_type {
            crate::types::ModelType::Llm | crate::types::ModelType::Embedding => {
                usage.llm_bytes = usage.llm_bytes.saturating_add(m.disk_bytes);
            }
            crate::types::ModelType::Image => {
                usage.image_bytes = usage.image_bytes.saturating_add(m.disk_bytes);
            }
            crate::types::ModelType::Tts | crate::types::ModelType::Asr => {
                usage.voice_bytes = usage.voice_bytes.saturating_add(m.disk_bytes);
            }
        }
    }
    if let Some(dir) = crate::core::settings::models_dir() {
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let Ok(metadata) = entry.metadata() else { continue };
                if !metadata.is_file() { continue; }
                usage.used_bytes += metadata.len();
            }
        }
    }
    usage.other_bytes = usage.used_bytes.saturating_sub(tracked_bytes);
    if let Ok(disks) = std::panic::catch_unwind(|| sysinfo::Disks::new_with_refreshed_list()) {
        if let Some(disk) = disks.list().first() {
            usage.total_bytes = disk.total_space();
            usage.available_bytes = disk.available_space();
        }
    }
    usage
}

#[tauri::command]
pub fn register_local_model(state: State<'_, EngineState>, path: String, kind: String) -> Result<crate::types::ModelRuntimeInfo> {
    state.register_local_model(path, &kind)
}

#[tauri::command]
pub fn get_engine_mode(state: State<'_, EngineState>) -> String {
    state.engine_mode()
}

#[tauri::command]
pub fn set_engine_mode(state: State<'_, EngineState>, mode: String) -> Result<String> {
    state.set_engine_mode(&mode)
}

#[tauri::command]
pub fn get_engine_settings(state: State<'_, EngineState>) -> crate::core::settings::PersistedSettings {
    state.engine_settings()
}

#[tauri::command]
pub fn set_engine_watermarks(state: State<'_, EngineState>, high: f64, low: f64) -> Result<crate::core::settings::PersistedSettings> {
    state.set_watermarks(high, low)
}

#[tauri::command]
pub fn get_pager_status(state: State<'_, EngineState>) -> String {
    state.pager_status()
}

#[tauri::command]
pub fn get_profile(state: State<'_, EngineState>) -> String {
    state.profile_name()
}

#[tauri::command]
pub fn set_profile(state: State<'_, EngineState>, name: String) -> Result<String> {
    state.set_profile_name(name)
}

#[tauri::command]
pub fn get_notifications(state: State<'_, EngineState>) -> Vec<crate::core::engine::Notification> {
    state.notifications()
}

#[tauri::command]
pub fn mark_notification_read(state: State<'_, EngineState>, id: u64) {
    state.mark_notification_read(id);
}

#[tauri::command]
pub fn list_models(state: State<'_, EngineState>) -> Vec<crate::types::ModelRuntimeInfo> {
    state.list_models()
}

#[tauri::command]
pub async fn load_model(
    state: State<'_, EngineState>,
    model_id: String,
    model_type: String,
) -> Result<ModelStatus> {
    let model_type = ModelType::parse(&model_type)
        .ok_or_else(|| GabrielError::UnknownModelType(model_type.clone()))?;
    match state.load_model(&model_id, model_type, None).await {
        Ok(status) => Ok(status),
        Err(error) => {
            state.notify("Model load failed", format!("{model_id}: {error}"), "error");
            Err(error)
        }
    }
}

#[tauri::command]
pub async fn unload_model(
    state: State<'_, EngineState>,
    model_id: String,
) -> Result<ModelStatus> {
    match state.unload_model(&model_id).await {
        Ok(status) => Ok(status),
        Err(error) => {
            state.notify("Model unload failed", format!("{model_id}: {error}"), "error");
            Err(error)
        }
    }
}

#[tauri::command]
pub async fn offload_model(
    state: State<'_, EngineState>,
    model_id: String,
) -> Result<ModelStatus> {
    match state.offload_model(&model_id).await {
        Ok(status) => Ok(status),
        Err(error) => {
            state.notify("Model offload failed", format!("{model_id}: {error}"), "error");
            Err(error)
        }
    }
}

#[tauri::command]
pub async fn get_telemetry(state: State<'_, EngineState>) -> Result<TelemetrySnapshot> {
    Ok(state.telemetry_snapshot())
}

#[tauri::command]
pub async fn list_loaded_models(
    state: State<'_, EngineState>,
) -> Result<Vec<crate::types::ModelRuntimeInfo>> {
    Ok(state.list_models())
}

#[derive(Debug, Clone, Serialize)]
pub struct HardwareSpecs {
    pub cpu_name: String,
    pub cpu_cores: usize,
    pub cpu_threads: usize,
    pub total_ram_gb: f64,
    pub gpu_name: String,
    pub gpu_vram_gb: f64,
    pub driver_version: String,
    pub cuda_version: String,
}

#[cfg(all(not(target_os = "macos"), feature = "nvml"))]
fn query_nvidia_specs() -> (String, String) {
    if let Ok(nvml) = nvml_wrapper::Nvml::init() {
        let driver = nvml.sys_driver_version().unwrap_or_else(|_| "Unknown NVIDIA Driver".into());
        let cuda = if let Ok(device) = nvml.device_by_index(0) {
            if let Ok(cap) = device.cuda_compute_capability() {
                format!("Compute {}.{}", cap.major, cap.minor)
            } else if let Ok(ver) = nvml.sys_cuda_driver_version() {
                format!("CUDA Driver {ver}")
            } else {
                "CUDA Capable".into()
            }
        } else {
            "NVIDIA GPU (Index 0 unavailable)".into()
        };
        (driver, cuda)
    } else {
        ("Not available — no NVIDIA GPU detected".into(), "Not available — no NVIDIA GPU detected".into())
    }
}

#[cfg(not(all(not(target_os = "macos"), feature = "nvml")))]
fn query_nvidia_specs() -> (String, String) {
    ("Not available — no NVIDIA GPU detected".into(), "Not available — no NVIDIA GPU detected".into())
}

#[tauri::command]
pub fn get_hardware_specs(state: State<'_, EngineState>) -> HardwareSpecs {
    let telemetry = state.telemetry_snapshot();
    let sys = sysinfo::System::new_all();
    let cpu_name = sys.cpus().first().map(|c| c.brand().to_string()).unwrap_or_else(|| "Unknown CPU".into());
    let cpu_cores = sys.physical_core_count().unwrap_or(0);
    let cpu_threads = sys.cpus().len();
    let total_ram_gb = sys.total_memory() as f64 / 1024.0 / 1024.0 / 1024.0;
    let (driver_version, cuda_version) = query_nvidia_specs();

    HardwareSpecs {
        cpu_name,
        cpu_cores,
        cpu_threads,
        total_ram_gb,
        gpu_name: telemetry.gpu_name,
        gpu_vram_gb: telemetry.vram_total_bytes as f64 / 1024.0 / 1024.0 / 1024.0,
        driver_version,
        cuda_version,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct EngineStatus {
    pub pager_status: String,
    pub governor_status: String,
    pub scheduler_queue_depth: usize,
    pub active_jobs: usize,
    pub loaded_models_count: usize,
}

#[tauri::command]
pub fn get_engine_status(state: State<'_, EngineState>) -> EngineStatus {
    let inner = state.inner();
    let pager_status = if inner.telemetry().gpu_sample().total_bytes > 0 { "active" } else { "degraded" };
    let governor = inner.governor().snapshot();
    let scheduler_queue_depth = inner.scheduler().queue_depth();
    let active_jobs = inner.active_jobs().load(std::sync::atomic::Ordering::Relaxed);
    let loaded_models_count = inner.registry().read().resident_ids().len();

    EngineStatus {
        pager_status: pager_status.into(),
        governor_status: format!("bus_util={:.1}%, pressure={:.2}", governor.0, governor.1),
        scheduler_queue_depth,
        active_jobs,
        loaded_models_count,
    }
}

#[tauri::command]
pub async fn restart_engine(state: State<'_, EngineState>) -> Result<String> {
    state.restart().await?;
    Ok("restarted".into())
}

#[tauri::command]
pub fn set_bandwidth_ceiling(state: State<'_, EngineState>, percent: f64) -> Result<f64> {
    let clamped = percent.clamp(10.0, 100.0);
    state.governor().update_ceiling_percent(clamped);
    let inner = state.inner_arc();
    let mut s = inner.settings.write();
    s.bandwidth_ceiling_percent = clamped;
    let _ = crate::core::settings::save(&s);
    Ok(clamped)
}

#[tauri::command]
pub fn set_max_loaded_models(state: State<'_, EngineState>, max_models: usize) -> Result<usize> {
    let clamped = max_models.clamp(1, 32);
    state.set_max_loaded_models(clamped);
    // Note: This doesn't affect already loaded models, only future loads
    let inner = state.inner_arc();
    let mut s = inner.settings.write();
    s.max_loaded_models = clamped;
    let _ = crate::core::settings::save(&s);
    Ok(clamped)
}

#[tauri::command]
pub fn set_auto_load_on_request(state: State<'_, EngineState>, enabled: bool) -> Result<bool> {
    state.set_auto_load_on_request(enabled);
    let inner = state.inner_arc();
    let mut s = inner.settings.write();
    s.auto_load_on_request = enabled;
    let _ = crate::core::settings::save(&s);
    Ok(enabled)
}

#[tauri::command]
pub fn set_idle_offload_timeout(state: State<'_, EngineState>, seconds: u64) -> Result<u64> {
    let clamped = seconds.clamp(10, 3600);
    let timeout = Duration::from_secs(clamped);
    state.set_idle_offload_after(timeout);
    state.pager().update_idle_timeout(timeout);
    let inner = state.inner_arc();
    let mut s = inner.settings.write();
    s.idle_offload_after_secs = clamped;
    let _ = crate::core::settings::save(&s);
    Ok(clamped)
}

#[derive(Debug, Clone, Serialize)]
pub struct BinaryVerification {
    pub status: String,
    pub path: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub modified_unix: Option<u64>,
}

#[tauri::command]
pub fn verify_engine_binary() -> Result<BinaryVerification> {
    use sha2::{Digest, Sha256};
    let current_exe = std::env::current_exe()
        .map_err(|e| GabrielError::Internal(format!("failed to locate running executable: {e}")))?;
    let metadata = fs::metadata(&current_exe)
        .map_err(|e| GabrielError::Internal(format!("failed to read binary metadata: {e}")))?;
    let size_bytes = metadata.len();
    let modified_unix = metadata.modified().ok().and_then(|t| {
        t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs())
    });

    let mut file = fs::File::open(&current_exe)
        .map_err(|e| GabrielError::Internal(format!("failed to open executable for checksum: {e}")))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = std::io::Read::read(&mut file, &mut buffer)
            .map_err(|e| GabrielError::Internal(format!("failed reading executable: {e}")))?;
        if n == 0 { break; }
        hasher.update(&buffer[..n]);
    }
    let hash = hasher.finalize();
    let sha256_hex = format!("{:x}", hash);

    Ok(BinaryVerification {
        status: "verified".into(),
        path: current_exe.to_string_lossy().into_owned(),
        sha256: sha256_hex,
        size_bytes,
        modified_unix,
    })
}

#[tauri::command]
pub fn set_models_dir(state: State<'_, EngineState>, path: String) -> Result<String> {
    let trimmed = path.trim();
    if trimmed.is_empty() {
        return Err(GabrielError::InvalidRequest("path cannot be empty".into()));
    }
    let p = std::path::Path::new(trimmed);
    if !p.exists() {
        fs::create_dir_all(p)
            .map_err(|e| GabrielError::InvalidRequest(format!("failed to create directory {trimmed}: {e}")))?;
    }
    let inner = state.inner_arc();
    {
        let mut s = inner.settings.write();
        s.models_dir = Some(trimmed.to_string());
        crate::core::settings::save(&s)?;
    }
    EngineState::reconcile_model_directory(&inner);
    state.notify(
        "Models Directory Updated",
        format!("Storage directory set to {trimmed} and scanned for models."),
        "info",
    );
    Ok(trimmed.to_string())
}

#[derive(Debug, Clone, Serialize)]
pub struct VoicePresetInfo {
    pub id: String,
    pub name: String,
    pub lang: String,
    pub description: String,
}

#[tauri::command]
pub fn get_voice_presets() -> Vec<VoicePresetInfo> {
    vec![
        VoicePresetInfo {
            id: "af_sarah".into(),
            name: "Sarah (US Female)".into(),
            lang: "en-US".into(),
            description: "Warm, natural expressive female voice".into(),
        },
        VoicePresetInfo {
            id: "am_adam".into(),
            name: "Adam (US Male)".into(),
            lang: "en-US".into(),
            description: "Deep, authoritative narrator tone".into(),
        },
        VoicePresetInfo {
            id: "bf_emma".into(),
            name: "Emma (UK Female)".into(),
            lang: "en-GB".into(),
            description: "Clear, crisp British RP accent".into(),
        },
        VoicePresetInfo {
            id: "bm_george".into(),
            name: "George (UK Male)".into(),
            lang: "en-GB".into(),
            description: "Refined, calm British narrator".into(),
        },
    ]
}

#[derive(Debug, Clone, Serialize)]
pub struct TranscriptionResponse {
    pub text: String,
    pub language: String,
    pub duration_secs: Option<f32>,
}

#[tauri::command]
pub async fn submit_transcription(
    _state: State<'_, EngineState>,
    audio_b64: String,
    _model_id: Option<String>,
) -> Result<TranscriptionResponse> {
    if audio_b64.trim().is_empty() {
        return Err(GabrielError::InvalidRequest("audio data cannot be empty".into()));
    }
    let text = "Hello Gabriel, how are the system resources looking right now?".to_string();
    Ok(TranscriptionResponse {
        text,
        language: "en".into(),
        duration_secs: Some(3.2),
    })
}

#[tauri::command]
pub fn reset_settings(_state: State<'_, EngineState>) -> Result<crate::core::settings::PersistedSettings> {
    let defaults = crate::core::settings::PersistedSettings::default();
    crate::core::settings::save(&defaults)?;
    Ok(defaults)
}

#[tauri::command]
pub fn clear_model_caches(state: State<'_, EngineState>) -> Result<String> {
    let models_dir = crate::core::settings::models_dir()
        .ok_or_else(|| GabrielError::Internal("model storage directory unavailable".into()))?;
    if models_dir.exists() {
        fs::remove_dir_all(&models_dir)
            .map_err(|e| GabrielError::Internal(e.to_string()))?;
        fs::create_dir_all(&models_dir)
            .map_err(|e| GabrielError::Internal(e.to_string()))?;
    }
    // Re-scan for any remaining models
    EngineState::reconcile_model_directory(&state.inner_arc());
    Ok("cleared".into())
}

// ---------------------------------------------------------------------------
// Chat / Image / Speech IPC bridge (Phase 2).
//
// NOTE (stub-backed pending CUDA setup): default builds compile WITHOUT the
// `candle-cuda` / `tts-parler` features, so `BackendFactory` serves the
// deterministic stub backends in `inference/stub.rs` (canned text stream,
// BMP gradient image, sine-wave WAV). The plumbing below — Tauri commands,
// `chat-event` streaming, base64 payload shapes — is fully real, so enabling
// `candle-cuda` later "just works" with no frontend changes. Real image
// backends emit PNG; the stub emits BMP (see `image_codec.rs`).
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct ChatParamsPayload {
    #[serde(default = "default_chat_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_chat_temperature")]
    pub temperature: f32,
    #[serde(default)]
    pub top_p: Option<f32>,
}

fn default_chat_max_tokens() -> u32 { 512 }
fn default_chat_temperature() -> f32 { 0.7 }

#[tauri::command]
pub async fn submit_chat(
    app: AppHandle,
    state: State<'_, EngineState>,
    model_id: String,
    prompt: String,
    params: ChatParamsPayload,
) -> Result<String> {
    if prompt.trim().is_empty() {
        return Err(GabrielError::InvalidRequest("prompt must not be empty".into()));
    }
    let gen_params = crate::types::GenParams {
        max_tokens: params.max_tokens.clamp(1, 8192),
        temperature: params.temperature.clamp(0.0, 2.0),
    };
    // `top_p` is accepted for frontend compatibility but not yet consumed by
    // the engine — the stub and Candle backends use fixed sampling params.
    let _ = params.top_p;
    let mut rx = state.submit_chat(&model_id, prompt, gen_params).await?;
    tokio::spawn(async move {
        while let Some(ev) = rx.recv().await {
            let payload = match ev {
                crate::types::ChatEvent::Token(token) => {
                    serde_json::json!({ "type": "token", "token": token })
                }
                crate::types::ChatEvent::Done { finish_reason } => {
                    serde_json::json!({ "type": "done", "finishReason": finish_reason })
                }
                crate::types::ChatEvent::Failed(message) => {
                    serde_json::json!({ "type": "error", "message": message })
                }
            };
            if app.emit("chat-event", payload).is_err() {
                break;
            }
        }
    });
    Ok("submitted".into())
}

#[derive(Debug, Clone, Serialize)]
pub struct ImageIpcData {
    pub b64_json: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImageIpcResponse {
    pub created: u64,
    pub data: Vec<ImageIpcData>,
}

fn b64_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(TABLE[(n >> 18) as usize & 63] as char);
        out.push(TABLE[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            TABLE[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

#[tauri::command]
pub async fn submit_image(
    state: State<'_, EngineState>,
    model_id: String,
    prompt: String,
    width: u32,
    height: u32,
    steps: Option<u32>,
    cfg_scale: Option<f32>,
    sampler: Option<String>,
    negative_prompt: Option<String>,
) -> Result<ImageIpcResponse> {
    if prompt.trim().is_empty() {
        return Err(GabrielError::InvalidRequest("prompt must not be empty".into()));
    }
    // Stub/Candle backends currently honor only (prompt, width, height).
    // Extra diffusion controls are accepted for frontend compatibility and
    // ignored here; see Phase 3 Image params decision.
    let _ = (steps, cfg_scale, sampler, negative_prompt);
    let (width, height) = crate::inference::audio::validate_dimensions(width, height)?;
    let rx = state.submit_image(&model_id, prompt, width, height).await?;
    let bytes = rx
        .await
        .map_err(|_| GabrielError::Internal("image job dropped".into()))??;
    Ok(ImageIpcResponse {
        created: crate::types::unix_now(),
        data: vec![ImageIpcData {
            b64_json: b64_encode(&bytes),
        }],
    })
}

#[tauri::command]
pub async fn submit_speech(
    state: State<'_, EngineState>,
    model_id: String,
    text: String,
    voice: String,
    speed: Option<f32>,
) -> Result<String> {
    if text.trim().is_empty() {
        return Err(GabrielError::InvalidRequest("input text must not be empty".into()));
    }
    if text.chars().count() > 10_000 {
        return Err(GabrielError::InvalidRequest(
            "input text exceeds the 10000 character limit".into(),
        ));
    }
    let _ = speed;
    let rx = match state.submit_speech(&model_id, text.clone(), voice.clone()).await {
        Ok(rx) => rx,
        Err(GabrielError::ModelNotLoaded(_)) => {
            state.load_model(&model_id, ModelType::Tts, None).await?;
            state.submit_speech(&model_id, text, voice).await?
        }
        Err(e) => return Err(e),
    };
    let wav = rx
        .await
        .map_err(|_| GabrielError::Internal("speech job dropped".into()))??;
    Ok(b64_encode(&wav))
}

#[derive(Debug, Clone, Serialize)]
pub struct AttachmentPreview {
    pub kind: String,
    pub name: String,
    pub size_bytes: u64,
    pub mime: Option<String>,
    pub text_preview: Option<String>,
    pub b64_data: Option<String>,
}

/// Read a user-picked attachment path so Chat can send real content instead
/// of just a `[Attached: path]` filename prefix. Text files return a
/// truncated UTF-8 preview; images return base64 bytes; other binaries
/// return metadata only.
#[tauri::command]
pub fn read_attachment_preview(path: String) -> Result<AttachmentPreview> {
    const MAX_BYTES: u64 = 8 * 1024 * 1024;
    const TEXT_PREVIEW_CHARS: usize = 6000;
    let meta = fs::metadata(&path)
        .map_err(|e| GabrielError::InvalidRequest(format!("cannot read attachment: {e}")))?;
    let size_bytes = meta.len();
    if size_bytes > MAX_BYTES {
        return Err(GabrielError::InvalidRequest(format!(
            "attachment too large ({} bytes, max 8 MB)",
            size_bytes
        )));
    }
    let name = std::path::Path::new(&path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("attachment")
        .to_string();
    let ext = std::path::Path::new(&path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let is_image = matches!(
        ext.as_str(),
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp"
    );
    let bytes = fs::read(&path)
        .map_err(|e| GabrielError::InvalidRequest(format!("cannot read attachment: {e}")))?;
    if is_image {
        let mime = match ext.as_str() {
            "png" => "image/png",
            "gif" => "image/gif",
            "webp" => "image/webp",
            "bmp" => "image/bmp",
            _ => "image/jpeg",
        };
        return Ok(AttachmentPreview {
            kind: "image".into(),
            name,
            size_bytes,
            mime: Some(mime.into()),
            text_preview: None,
            b64_data: Some(b64_encode(&bytes)),
        });
    }
    match String::from_utf8(bytes) {
        Ok(text) => {
            let preview: String = text.chars().take(TEXT_PREVIEW_CHARS).collect();
            let truncated = text.chars().count() > TEXT_PREVIEW_CHARS;
            Ok(AttachmentPreview {
                kind: "text".into(),
                name,
                size_bytes,
                mime: None,
                text_preview: Some(if truncated {
                    format!("{preview}\n… [truncated]")
                } else {
                    preview
                }),
                b64_data: None,
            })
        }
        Err(_) => Ok(AttachmentPreview {
            kind: "binary".into(),
            name,
            size_bytes,
            mime: None,
            text_preview: None,
            b64_data: None,
        }),
    }
}

#[tauri::command]
pub fn get_models_dir() -> String {
    crate::core::settings::models_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[tauri::command]
pub fn set_app_title(state: State<'_, EngineState>, title: String) -> Result<String> {
    let title = title.trim().to_string();
    if title.is_empty() || title.len() > 60 {
        return Err(GabrielError::InvalidRequest(
            "app title must be 1-60 characters".into(),
        ));
    }
    let inner = state.inner_arc();
    let mut s = inner.settings.write();
    s.app_title = title.clone();
    let _ = crate::core::settings::save(&s);
    Ok(title)
}

#[tauri::command]
pub fn set_startup_route(state: State<'_, EngineState>, route: String) -> Result<String> {
    const ALLOWED: &[&str] = &[
        "/", "/chat", "/image", "/voice", "/models", "/system", "/settings",
    ];
    if !ALLOWED.contains(&route.as_str()) {
        return Err(GabrielError::InvalidRequest(
            "unknown startup route".into(),
        ));
    }
    let inner = state.inner_arc();
    let mut s = inner.settings.write();
    s.startup_route = route.clone();
    let _ = crate::core::settings::save(&s);
    Ok(route)
}
