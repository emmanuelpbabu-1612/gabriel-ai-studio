use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use serde::Serialize;

use parking_lot::RwLock;
use tokio::sync::{Semaphore, mpsc, oneshot};

use crate::error::{GabrielError, Result};
use crate::inference::{BackendFactory, ImageBackend, SpeechBackend, TextBackend};
#[cfg(any(feature = "candle-cuda", feature = "tts-parler"))]
use crate::inference::hub;
use crate::telemetry::Telemetry;
use crate::types::{
    ChatEvent, GenParams, Job, JobId, JobKind, ModelRuntimeInfo, ModelSpec, ModelStatus, ModelType,
    Priority, Residency, TelemetrySnapshot, unix_now,
};

use super::EngineConfig;
use super::bandwidth::{BandwidthGovernor, GovernorConfig};
use super::pager::{MemoryPager, PagerConfig};
use super::registry::Registry;
use super::scheduler::{Scheduler, dispatch};
use super::settings::{self, PersistedSettings};

#[derive(Debug, Clone, Serialize)]
pub struct Notification {
    pub id: u64,
    pub title: String,
    pub message: String,
    pub level: String,
    pub read: bool,
    pub created_at_unix: u64,
}

#[derive(Clone)]
pub enum ModelHandle {
    Text(Arc<dyn TextBackend>),
    Image(Arc<dyn ImageBackend>),
    Speech(Arc<dyn SpeechBackend>),
}

impl ModelHandle {
    pub fn measured_vram_bytes(&self) -> Option<u64> {
        match self {
            Self::Text(b) => b.measured_vram_bytes(),
            Self::Image(b) => b.measured_vram_bytes(),
            Self::Speech(_) => None,
        }
    }
}

impl std::fmt::Debug for ModelHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Text(b) => f.debug_tuple("Text").field(b).finish(),
            Self::Image(b) => f.debug_tuple("Image").field(b).finish(),
            Self::Speech(b) => f.debug_tuple("Speech").field(b).finish(),
        }
    }
}

pub struct EngineInner {
    pub config: Arc<RwLock<EngineConfig>>,
    pub registry: Arc<RwLock<Registry>>,
    pub telemetry: Arc<Telemetry>,
    pub pager: Arc<MemoryPager>,
    pub governor: Arc<BandwidthGovernor>,
    pub scheduler: Scheduler,
    pub factory: BackendFactory,
    pub settings: Arc<RwLock<PersistedSettings>>,
    pub notifications: Arc<RwLock<Vec<Notification>>>,
    next_notification_id: AtomicUsize,
    image_permits: Arc<Semaphore>,
    active_jobs: AtomicUsize,
    load_gate: tokio::sync::Mutex<()>,
    download_cancels: Arc<RwLock<HashMap<String, Arc<AtomicBool>>>>,
}

impl EngineInner {
    pub async fn execute(self: Arc<Self>, job: Job) {
        self.active_jobs.fetch_add(1, Ordering::Relaxed);
        let started = Instant::now();

        tracing::trace!(job_id = %job.id, kind = ?job.kind, "executing job");

        match job.kind {
            JobKind::Chat {
                prompt,
                params,
                events,
            } => {
                self.run_chat(&job.model_id, &prompt, params, events).await;
            }
            JobKind::Image {
                prompt,
                width,
                height,
                reply,
            } => {
                let _permit = self.image_permits.acquire().await;
                let result = self.run_image(&job.model_id, &prompt, width, height).await;
                let _ = reply.send(result);
            }
            JobKind::Speech {
                text,
                voice,
                reply,
                cpu_fallback,
            } => {
                let result = self
                    .run_speech(&job.model_id, &text, &voice, cpu_fallback)
                    .await;
                let _ = reply.send(result);
            }
        }

        tracing::debug!(
            job_id = %job.id,
            elapsed_ms = started.elapsed().as_millis() as u64,
            "job finished"
        );
        self.active_jobs.fetch_sub(1, Ordering::Relaxed);
    }

    async fn run_chat(
        &self,
        model_id: &str,
        prompt: &str,
        params: GenParams,
        events: mpsc::Sender<ChatEvent>,
    ) {
        let (real_id, handle) = {
            let reg = self.registry.read();
            match reg.find_matching(model_id, ModelType::Llm) {
                Some((id, e)) => match e.handle.as_ref() {
                    Some(ModelHandle::Text(t)) => (id.to_string(), Some(t.clone())),
                    _ => (id.to_string(), None),
                },
                None => (model_id.to_string(), None),
            }
        };

        let Some(backend) = handle else {
            let _ = events
                .send(ChatEvent::Failed(format!(
                    "model {model_id} is offloaded or unavailable"
                )))
                .await;
            return;
        };

        // Touch timestamp without long write lock contention
        self.touch_model(&real_id);

        match backend.stream_tokens(prompt, params, events.clone()).await {
            Ok(count) => {
                let _ = events
                    .send(ChatEvent::Done {
                        finish_reason: "stop".into(),
                    })
                    .await;
                tracing::debug!(model = %real_id, tokens = count, "chat stream complete");
            }
            Err(e) => {
                tracing::error!(model = %real_id, error = %e, "chat stream failed");
                let _ = events.send(ChatEvent::Failed(e.to_string())).await;
            }
        }
    }

    async fn run_image(
        &self,
        model_id: &str,
        prompt: &str,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>> {
        let (real_id, handle) = {
            let reg = self.registry.read();
            match reg.find_matching(model_id, ModelType::Image) {
                Some((id, e)) => match e.handle.as_ref() {
                    Some(ModelHandle::Image(i)) => (id.to_string(), Some(i.clone())),
                    _ => (id.to_string(), None),
                },
                None => (model_id.to_string(), None),
            }
        };

        let Some(backend) = handle else {
            return Err(GabrielError::ModelNotLoaded(model_id.to_string()));
        };

        self.touch_model(&real_id);

        let img_prompt = prompt.to_owned();
        let bg = backend.clone();

        bg.generate(&img_prompt, width, height).await
    }

    async fn run_speech(
        &self,
        model_id: &str,
        text: &str,
        voice: &str,
        cpu_fallback: bool,
    ) -> Result<Vec<u8>> {
        let (real_id, handle) = {
            let reg = self.registry.read();
            match reg.find_matching(model_id, ModelType::Tts) {
                Some((id, e)) => match e.handle.as_ref() {
                    Some(ModelHandle::Speech(s)) => (id.to_string(), Some(s.clone())),
                    _ => (id.to_string(), None),
                },
                None => (model_id.to_string(), None),
            }
        };

        let Some(backend) = handle else {
            return Err(GabrielError::ModelNotLoaded(model_id.to_string()));
        };

        self.touch_model(&real_id);

        let tts_text = text.to_owned();
        let tts_voice = voice.to_owned();
        let bg = backend.clone();

        bg.synthesize(&tts_text, &tts_voice, cpu_fallback).await
    }

    #[inline]
    fn touch_model(&self, model_id: &str) {
        // Quick touch helper to minimize hold duration
        self.registry.write().touch(model_id);
    }
}

#[derive(Clone)]
pub struct EngineState(Arc<EngineInner>);

impl std::fmt::Debug for EngineState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EngineState")
            .field("models", &self.list_models())
            .finish()
    }
}

impl EngineState {
    pub fn new(config: EngineConfig) -> Self {
        let persisted = settings::load();
        let mut config = config;
        config.vram_high_watermark = persisted.vram_high_watermark / 100.0;
        config.vram_low_watermark = persisted.vram_low_watermark / 100.0;
        config.bandwidth_ceiling_percent = persisted.bandwidth_ceiling_percent.clamp(10.0, 100.0);
        config.max_loaded_models = persisted.max_loaded_models.clamp(1, 32);
        config.auto_load_on_request = persisted.auto_load_on_request;
        config.idle_offload_after = Duration::from_secs(persisted.idle_offload_after_secs.clamp(10, 3600));
        match persisted.engine_mode.as_str() {
            "performance" => {
                config.vram_high_watermark = 0.95;
                config.vram_low_watermark = 0.80;
                config.bandwidth_ceiling_percent = 100.0;
            }
            "efficiency" => {
                config.vram_high_watermark = 0.75;
                config.vram_low_watermark = 0.55;
                config.bandwidth_ceiling_percent = 60.0;
            }
            _ => {}
        }
        // Save values for logging before config is moved
        let log_host = config.host.clone();
        let log_port = config.port;
        let log_high_watermark = config.vram_high_watermark;
        let log_pager_poll_interval = config.pager_poll_interval;

        let telemetry = Arc::new(Telemetry::new());
        let registry = Arc::new(RwLock::new(Registry::default()));

        let pager = Arc::new(MemoryPager::new(
            PagerConfig::new(
                config.vram_high_watermark,
                config.vram_low_watermark,
                config.idle_offload_after,
                config.pager_poll_interval,
            ),
            telemetry.clone(),
            registry.clone(),
        ));

        let (scheduler, rx) = Scheduler::new(config.queue_capacity);

        let governor = Arc::new(BandwidthGovernor::new(GovernorConfig {
            ceiling_percent: config.bandwidth_ceiling_percent,
            max_yield: config.max_bandwidth_yield,
        }));

        let inner = Arc::new(EngineInner {
            image_permits: Arc::new(Semaphore::new(config.max_concurrent_image_jobs)),
            active_jobs: AtomicUsize::new(0),
            load_gate: tokio::sync::Mutex::new(()),
            config: Arc::new(RwLock::new(config)),
            registry: registry.clone(),
            telemetry: telemetry.clone(),
            pager: pager.clone(),
            governor: governor.clone(),
            scheduler,
            factory: BackendFactory::new(governor.clone()),
            settings: Arc::new(RwLock::new(persisted)),
            notifications: Arc::new(RwLock::new(Vec::new())),
            next_notification_id: AtomicUsize::new(1),
            download_cancels: Arc::new(RwLock::new(HashMap::new())),
        });

        Self::cleanup_partial_downloads();
        Self::reconcile_model_directory(&inner);

        {
            let governor = governor.clone();
            let telemetry = telemetry.clone();
            let interval = log_pager_poll_interval;
            tokio::spawn(async move {
                let mut tick = tokio::time::interval(interval);
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                loop {
                    tick.tick().await;
                    let sample = telemetry.gpu_sample();
                    governor.update(sample.memory_bandwidth_percent);
                }
            });
        }

        pager.spawn_loop();
        tokio::spawn(dispatch::run(rx, Self(inner.clone())));

        tracing::info!(
            host = %log_host,
            port = log_port,
            high_watermark = log_high_watermark,
            "engine initialized"
        );

        Self(inner)
    }

    pub fn reconcile_model_directory(inner: &Arc<EngineInner>) {
        let Some(models_dir) = settings::models_dir() else { return };
        let Ok(entries) = fs::read_dir(&models_dir) else { return };
        let mut registry = inner.registry.write();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() { continue; }
            let Some(file_name) = path.file_stem().and_then(|value| value.to_str()) else { continue };
            let extension = path.extension().and_then(|value| value.to_str()).unwrap_or_default().to_ascii_lowercase();
            // Filename hint for voice models: TTS weight files (e.g. the
            // kokoro-voice fixture) can carry a generic extension, so a known
            // voice marker in the stem takes precedence over the extension
            // sniff below. Files without a marker are typed as before.
            let stem_lower = file_name.to_ascii_lowercase();
            let voice_hint = ["kokoro", "tts", "text-to-speech", "voice", "parler", "bark", "xtts", "piper"]
                .iter()
                .any(|marker| stem_lower.contains(marker));
            let model_type = if voice_hint {
                ModelType::Tts
            } else {
                match extension.as_str() {
                    "gguf" | "ggml" => ModelType::Llm,
                    "safetensors" => {
                        tracing::warn!(path = %path.display(), "skipping ambiguous safetensors file; import it with an explicit model kind");
                        continue;
                    }
                    _ => continue,
                }
            };
            if registry.contains(file_name) { continue; }
            let mut spec = ModelSpec::new(file_name, model_type, None);
            spec.disk_bytes = entry.metadata().map(|metadata| metadata.len()).unwrap_or(0);
            registry.insert_available(spec);
            tracing::info!(model = file_name, "reconciled model file into registry");
        }
    }

    pub fn engine_mode(&self) -> String {
        self.inner().settings.read().engine_mode.clone()
    }

    pub fn set_engine_mode(&self, mode: &str) -> Result<String> {
        let mut settings = self.inner().settings.write();
        settings::apply_mode(&mut settings, mode)
            .map_err(GabrielError::InvalidRequest)?;
        settings::save(&settings)?;
        Ok(settings.engine_mode.clone())
    }

    pub fn engine_settings(&self) -> PersistedSettings {
        self.inner().settings.read().clone()
    }

    pub fn set_watermarks(&self, high: f64, low: f64) -> Result<PersistedSettings> {
        if !(50.0..=98.0).contains(&high) || !(30.0..=80.0).contains(&low) || low >= high {
            return Err(GabrielError::InvalidRequest("invalid VRAM watermark range".into()));
        }
        let mut settings = self.inner().settings.write();
        settings.vram_high_watermark = high;
        settings.vram_low_watermark = low;
        settings::save(&settings)?;
        let saved = settings.clone();
        drop(settings);
        // Apply live (previously this only took effect after a restart:
        // the pager loop reads its own config snapshot, not settings).
        self.inner().config.write().vram_high_watermark = high / 100.0;
        self.inner().config.write().vram_low_watermark = low / 100.0;
        self.pager().update_watermarks(high / 100.0, low / 100.0);
        Ok(saved)
    }

    pub fn profile_name(&self) -> String {
        self.inner().settings.read().display_name.clone()
    }

    pub fn set_profile_name(&self, name: String) -> Result<String> {
        let name = name.trim().to_string();
        if name.is_empty() || name.len() > 80 {
            return Err(GabrielError::InvalidRequest("display name must be 1-80 characters".into()));
        }
        let mut settings = self.inner().settings.write();
        settings.display_name = name;
        settings::save(&settings)?;
        Ok(settings.display_name.clone())
    }

    pub fn pager_status(&self) -> String {
        if self.inner().telemetry.gpu_sample().total_bytes > 0 { "active".into() } else { "degraded".into() }
    }

    pub fn notifications(&self) -> Vec<Notification> {
        self.inner().notifications.read().clone()
    }

    pub fn mark_notification_read(&self, id: u64) {
        if let Some(notification) = self.inner().notifications.write().iter_mut().find(|item| item.id == id) {
            notification.read = true;
        }
    }

    pub fn notify(&self, title: impl Into<String>, message: impl Into<String>, level: &str) {
        let id = self.inner().next_notification_id.fetch_add(1, Ordering::Relaxed) as u64;
        self.inner().notifications.write().insert(0, Notification {
            id,
            title: title.into(),
            message: message.into(),
            level: level.into(),
            read: false,
            created_at_unix: unix_now(),
        });
    }

    pub async fn restart(&self) -> Result<()> {
        let fresh_settings = settings::load();
        *self.inner().settings.write() = fresh_settings.clone();

        {
            let mut registry = self.inner().registry.write();
            registry.reset_resident();
        }

        let high = fresh_settings.vram_high_watermark / 100.0;
        let low = fresh_settings.vram_low_watermark / 100.0;
        self.inner().config.write().vram_high_watermark = high;
        self.inner().config.write().vram_low_watermark = low;
        self.pager().update_watermarks(high, low);
        self.pager().update_idle_timeout(Duration::from_secs(fresh_settings.idle_offload_after_secs));
        self.governor().update_ceiling_percent(fresh_settings.bandwidth_ceiling_percent);
        self.set_max_loaded_models(fresh_settings.max_loaded_models);
        self.set_auto_load_on_request(fresh_settings.auto_load_on_request);

        Self::reconcile_model_directory(&self.inner_arc());

        self.notify(
            "Engine Restarted",
            "Engine state reloaded in-place, configurations refreshed, and model directory rescanned.",
            "info",
        );

        tracing::info!("Engine restarted in-place successfully");
        Ok(())
    }

    fn inner(&self) -> &EngineInner {
        &self.0
    }

    pub(crate) fn inner_arc(&self) -> Arc<EngineInner> {
        self.0.clone()
    }

    pub fn governor(&self) -> Arc<BandwidthGovernor> {
        self.0.governor.clone()
    }

    pub fn set_max_loaded_models(&self, max_models: usize) {
        self.0.config.write().max_loaded_models = max_models;
    }

    pub fn set_auto_load_on_request(&self, enabled: bool) {
        self.0.config.write().auto_load_on_request = enabled;
    }

    pub fn set_idle_offload_after(&self, duration: Duration) {
        self.0.config.write().idle_offload_after = duration;
    }

    pub async fn load_model(
        &self,
        model_id: &str,
        model_type: ModelType,
        vram_bytes: Option<u64>,
    ) -> Result<ModelStatus> {
        let _gate = self.inner().load_gate.lock().await;

        if let Some(entry) = self.inner().registry.read().get(model_id) {
            if entry.handle.is_some() {
                return Err(GabrielError::AlreadyLoaded(model_id.to_string()));
            }
        }

        let preserved_disk_bytes = self.inner().registry.read()
            .get(model_id)
            .map(|e| e.spec.disk_bytes)
            .unwrap_or(0);

        if self.inner().registry.read().contains(model_id) {
            self.inner().registry.write().remove(model_id);
        }

        let slot_limit = self.inner().config.read().max_loaded_models;

        // Eviction with infinite loop safeguard
        let mut attempts = 0;
        while self.inner().registry.read().len() >= slot_limit {
            attempts += 1;
            if attempts > slot_limit * 2 {
                return Err(GabrielError::SlotPoolExhausted { limit: slot_limit });
            }

            let victim = {
                let reg = self.inner().registry.read();
                reg.least_recently_used_any_idle(self.inner().config.read().idle_offload_after)
            };

            let Some(victim_id) = victim else {
                return Err(GabrielError::SlotPoolExhausted { limit: slot_limit });
            };

            tracing::info!(model = %victim_id, "slot pool full: evicting least recently used");
            if let Err(e) = self.unload_model(&victim_id).await {
                tracing::warn!(model = %victim_id, error = %e, "failed to evict victim model");
                break;
            }
        }

        let mut spec = ModelSpec::new(model_id, model_type, vram_bytes);
        if spec.disk_bytes == 0 {
            spec.disk_bytes = preserved_disk_bytes;
        }
        self.inner().pager.admit(spec.vram_bytes)?;

        let handle = self.inner().factory.create(&spec).await.inspect_err(|_| {
            tracing::warn!(model = %model_id, "backend construction failed");
            self.inner().pager.on_demoted_or_unloaded(spec.vram_bytes);
        })?;

        let mut final_spec = spec.clone();
        if let Some(measured) = handle.measured_vram_bytes() {
            tracing::info!(
                model = %model_id,
                estimated_bytes = final_spec.vram_bytes,
                measured_bytes = measured,
                "reconciling VRAM budget with measured allocation"
            );
            final_spec.vram_bytes = measured;
        }

        {
            let mut reg = self.inner().registry.write();
            reg.insert_new(final_spec.clone());
            reg.promote(model_id, handle);
        }

        self.inner().pager.on_promoted(final_spec.vram_bytes);

        tracing::info!(
            model = %model_id,
            vram_bytes = final_spec.vram_bytes,
            ?model_type,
            "model resident on GPU"
        );
        self.notify("Model loaded", format!("{model_id} is resident on the GPU"), "success");

        Ok(ModelStatus {
            model_id: model_id.to_string(),
            model_type,
            residency: Residency::Gpu,
            vram_bytes: final_spec.vram_bytes,
            loaded_at_unix: unix_now(),
        })
    }

    pub async fn offload_model(&self, model_id: &str) -> Result<ModelStatus> {
        let (spec, was_resident) = {
            let reg = self.inner().registry.read();
            let entry = reg
                .get(model_id)
                .ok_or_else(|| GabrielError::ModelNotLoaded(model_id.to_string()))?;
            (
                entry.spec.clone(),
                entry.residency == Residency::Gpu && entry.handle.is_some(),
            )
        };

        if was_resident {
            self.inner().registry.write().demote(model_id);
            self.inner().pager.on_demoted_or_unloaded(spec.vram_bytes);
            tracing::info!(model = %model_id, "model offloaded VRAM -> RAM by request");
            self.notify("Model offloaded", format!("{model_id} was paged to system RAM"), "info");
        }
        // No notify when nothing was resident: an offload request against an
        // already-idle model is a no-op, and logging it as an event makes
        // System Logs look like the model is flapping in and out of residency.

        Ok(ModelStatus {
            model_id: model_id.to_string(),
            model_type: spec.model_type,
            residency: Residency::Cpu,
            vram_bytes: 0,
            loaded_at_unix: unix_now(),
        })
    }

    pub async fn unload_model(&self, model_id: &str) -> Result<ModelStatus> {
        let entry = {
            let reg = self.inner().registry.read();
            reg.get(model_id)
                .map(|e| (e.spec.clone(), e.residency))
                .ok_or_else(|| GabrielError::ModelNotLoaded(model_id.to_string()))?
        };

        let was_resident = entry.1 == Residency::Gpu;
        self.inner().registry.write().remove(model_id);

        if was_resident {
            self.inner()
                .pager
                .on_demoted_or_unloaded(entry.0.vram_bytes);
        }

        tracing::info!(model = %model_id, "model unloaded");
        self.notify("Model unloaded", format!("{model_id} was removed from the engine"), "info");
        Ok(ModelStatus {
            model_id: model_id.to_string(),
            model_type: entry.0.model_type,
            residency: Residency::Cpu,
            vram_bytes: 0,
            loaded_at_unix: unix_now(),
        })
    }

    async fn ensure_ready(&self, model_id: &str, expected: ModelType) -> Result<String> {
        {
            let reg = self.inner().registry.read();
            if let Some((matched_id, entry)) = reg.find_matching(model_id, expected) {
                if entry.spec.model_type != expected {
                    return Err(GabrielError::InvalidRequest(format!(
                        "model {model_id} is a {:?} model, not {:?}",
                        entry.spec.model_type, expected
                    )));
                }
                if (entry.residency == Residency::Gpu || expected == ModelType::Tts) && entry.handle.is_some() {
                    let real_id = matched_id.to_string();
                    drop(reg);
                    self.inner().touch_model(&real_id);
                    return Ok(real_id);
                }
            }
        }

        if !self.inner().config.read().auto_load_on_request {
            return Err(GabrielError::ModelNotLoaded(model_id.to_string()));
        }

        match self.load_model(model_id, expected, None).await {
            Ok(_) => Ok(model_id.to_string()),
            Err(GabrielError::AlreadyLoaded(_)) => Ok(model_id.to_string()),
            Err(e) => Err(e),
        }
    }

    pub async fn submit_chat(
        &self,
        model_id: &str,
        prompt: String,
        params: GenParams,
    ) -> Result<mpsc::Receiver<ChatEvent>> {
        let model_id = self.ensure_ready(model_id, ModelType::Llm).await?;

        let (tx, rx) = mpsc::channel::<ChatEvent>(256);
        let job = Job {
            id: JobId::new_v4(),
            priority: Priority::Interactive,
            model_id,
            kind: JobKind::Chat {
                prompt,
                params,
                events: tx,
            },
        };

        self.enqueue(job).await?;
        Ok(rx)
    }

    pub async fn submit_image(
        &self,
        model_id: &str,
        prompt: String,
        width: u32,
        height: u32,
    ) -> Result<oneshot::Receiver<std::result::Result<Vec<u8>, GabrielError>>> {
        let model_id = self.ensure_ready(model_id, ModelType::Image).await?;

        let (tx, rx) = oneshot::channel();
        let job = Job {
            id: JobId::new_v4(),
            priority: Priority::Standard,
            model_id,
            kind: JobKind::Image {
                prompt,
                width,
                height,
                reply: tx,
            },
        };

        self.enqueue(job).await?;
        Ok(rx)
    }

    pub async fn submit_speech(
        &self,
        model_id: &str,
        text: String,
        voice: String,
    ) -> Result<oneshot::Receiver<std::result::Result<Vec<u8>, GabrielError>>> {
        let model_id = self.ensure_ready(model_id, ModelType::Tts).await?;

        let tts_vram_required: usize = 450_000_000;
        let snap = self.telemetry_snapshot();

        let vram_limit = if snap.vram_total_bytes == 0 {
            usize::MAX
        } else {
            (snap.vram_total_bytes as f64 * self.config().vram_high_watermark) as usize
        };
        let available_vram_budget = vram_limit.saturating_sub(snap.engine_resident_bytes as usize);

        #[cfg(any(feature = "candle-cuda", feature = "tts-parler"))]
        let cpu_fallback = {
            let actual_free = {
                let gpu_used = hub::gpu_used_bytes().unwrap_or(0);
                if snap.vram_total_bytes > 0 {
                    snap.vram_total_bytes.saturating_sub(gpu_used as u64) as usize
                } else {
                    available_vram_budget
                }
            };
            const CUDA_WORKSPACE_RESERVE: usize = 512 * 1024 * 1024;
            let effective_budget = actual_free.saturating_sub(CUDA_WORKSPACE_RESERVE);
            let is_cpu = effective_budget < tts_vram_required;
            if is_cpu {
                tracing::info!(
                    free_mb = effective_budget / 1_000_000,
                    "VRAM budget tight. Fallback TTS to CPU."
                );
            } else {
                tracing::info!(
                    free_mb = effective_budget / 1_000_000,
                    "VRAM budget available. Routing TTS to CUDA."
                );
            }
            is_cpu
        };
        #[cfg(not(any(feature = "candle-cuda", feature = "tts-parler")))]
        let cpu_fallback = {
            let is_cpu =
                available_vram_budget != usize::MAX && available_vram_budget < tts_vram_required;
            if is_cpu {
                tracing::info!(
                    free_mb = available_vram_budget / 1_000_000,
                    "VRAM budget tight. Fallback TTS to CPU."
                );
            } else {
                let free_disp = if available_vram_budget == usize::MAX {
                    snap.engine_resident_bytes as usize
                } else {
                    available_vram_budget
                };
                tracing::info!(
                    free_mb = free_disp / 1_000_000,
                    "VRAM budget available. Routing TTS to CUDA."
                );
            }
            is_cpu
        };

        let (tx, rx) = oneshot::channel();
        let job = Job {
            id: JobId::new_v4(),
            priority: Priority::Interactive,
            model_id,
            kind: JobKind::Speech {
                text,
                voice,
                reply: tx,
                cpu_fallback,
            },
        };

        self.enqueue(job).await?;
        Ok(rx)
    }

    async fn enqueue(&self, job: Job) -> Result<()> {
        self.inner()
            .scheduler
            .submit(job)
            .await
            .map_err(|j| GabrielError::QueueRejected {
                job_id: j.id.to_string(),
                reason: "queue saturated".into(),
            })
    }

    pub fn list_models(&self) -> Vec<ModelRuntimeInfo> {
        let models = self.inner().registry.read().snapshot();
        tracing::info!(count = models.len(), "listing models");
        models
    }

    pub fn register_local_model(&self, source: String, kind: &str) -> Result<ModelRuntimeInfo> {
        tracing::info!(source = %source, kind, "registering local model");
        let source_path = Path::new(&source);
        let file_name = source_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| GabrielError::InvalidRequest("invalid model file path".into()))?;
        let model_type = ModelType::parse(kind)
            .ok_or_else(|| GabrielError::UnknownModelType(kind.to_string()))?;
        let models_dir = settings::models_dir()
            .ok_or_else(|| GabrielError::Internal("model storage directory unavailable".into()))?;
        fs::create_dir_all(&models_dir)?;
        let destination = if source_path.parent() == Some(models_dir.as_path()) {
            source_path.to_path_buf()
        } else {
            let destination = models_dir.join(file_name);
            fs::copy(source_path, &destination)?;
            destination
        };
        let disk_bytes = fs::metadata(&destination)?.len();
        let id = destination
            .file_stem()
            .and_then(|name| name.to_str())
            .unwrap_or(file_name)
            .to_string();
        let mut spec = ModelSpec::new(id.clone(), model_type, None);
        spec.disk_bytes = disk_bytes;
        let mut registry = self.inner().registry.write();
        registry.insert_available(spec);
        tracing::info!(model = %id, disk_bytes, kind, "registered local model");
        registry
            .snapshot()
            .into_iter()
            .find(|model| model.id == id)
            .ok_or_else(|| GabrielError::Internal("registered model was not found".into()))
    }

    /// Remove stale `.part` files left by downloads interrupted by an app
    /// shutdown. Only the known temp suffix is cleaned — anything else on
    /// disk (even unregistered files) is left alone.
    pub fn cleanup_partial_downloads() {
        let Some(dir) = settings::models_dir() else { return };
        let Ok(entries) = fs::read_dir(&dir) else { return };
        let mut removed = 0u32;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("part")
                && fs::remove_file(&path).is_ok()
            {
                removed += 1;
            }
        }
        if removed > 0 {
            tracing::warn!(removed, "cleaned up partial model downloads from a previous session");
        }
    }

    pub async fn download_huggingface_model(&self, repo_id: String, filename: String, kind: String) -> Result<ModelRuntimeInfo> {
        let cancel = Arc::new(AtomicBool::new(false));
        match self.download_inner(repo_id, filename, kind, cancel, |_, _| {}).await? {
            Some(info) => Ok(info),
            None => Err(GabrielError::Internal("download cancelled".into())),
        }
    }

    pub async fn download_huggingface_model_with_progress(
        &self,
        repo_id: String,
        filename: String,
        kind: String,
        on_progress: impl Fn(u64, Option<u64>) + Send + 'static,
    ) -> Result<ModelRuntimeInfo> {
        let cancel = Arc::new(AtomicBool::new(false));
        match self.download_inner(repo_id, filename, kind, cancel, on_progress).await? {
            Some(info) => Ok(info),
            None => Err(GabrielError::Internal("download cancelled".into())),
        }
    }

    /// Fire-and-forget variant for the Tauri UI: the caller supplies a
    /// download id (so progress/done events can be correlated with no race),
    /// the outcome is delivered to `on_done` (`Ok(Some)` = registered,
    /// `Ok(None)` = cancelled, `Err` = failed). Progress flows through
    /// `on_progress` as before.
    pub fn start_hf_download(
        &self,
        download_id: String,
        repo_id: String,
        filename: String,
        kind: String,
        on_progress: impl Fn(u64, Option<u64>) + Send + 'static,
        on_done: impl FnOnce(Result<Option<ModelRuntimeInfo>>) + Send + 'static,
    ) {
        let flag = Arc::new(AtomicBool::new(false));
        self.inner()
            .download_cancels
            .write()
            .insert(download_id.clone(), flag.clone());
        let this = self.clone();
        tokio::spawn(async move {
            let outcome = this
                .download_inner(repo_id, filename, kind, flag, on_progress)
                .await;
            this.inner().download_cancels.write().remove(&download_id);
            on_done(outcome);
        });
    }

    /// Signal a running download to abort. Returns true if a matching active
    /// download was found. The loop exits at the next chunk boundary, deletes
    /// the partial file, and never registers anything.
    pub fn cancel_hf_download(&self, download_id: &str) -> bool {
        if let Some(flag) = self.inner().download_cancels.read().get(download_id) {
            flag.store(true, Ordering::Relaxed);
            tracing::info!(download_id, "cancellation requested for Hugging Face download");
            true
        } else {
            false
        }
    }

    /// Streaming download core shared by the blocking and fire-and-forget
    /// paths. Returns `Ok(None)` when `cancel` is set — the partial file is
    /// deleted and nothing is registered in that case.
    async fn download_inner(
        &self,
        repo_id: String,
        filename: String,
        kind: String,
        cancel: Arc<AtomicBool>,
        on_progress: impl Fn(u64, Option<u64>) + Send + 'static,
    ) -> Result<Option<ModelRuntimeInfo>> {
        tracing::info!(repo = %repo_id, file = %filename, kind = %kind, "starting Hugging Face model download");
        let url = format!("https://huggingface.co/{repo_id}/resolve/main/{filename}");
        let download_repo_id = repo_id.clone();
        let log_filename = filename.clone();
        let outcome = tokio::task::spawn_blocking(move || -> Result<Option<(std::path::PathBuf, u64)>> {
            // Generous timeouts: model files are GB-scale, so the connect is
            // bounded but the transfer itself gets a 30-minute budget.
            let config = ureq::Agent::config_builder()
                .timeout_connect(Some(Duration::from_secs(30)))
                .timeout_global(Some(Duration::from_secs(30 * 60)))
                .build();
            let agent = ureq::Agent::new_with_config(config);
            let fail = |error: String| GabrielError::DownloadFailed {
                repo: download_repo_id.clone(),
                filename: error,
            };
            let response = agent.get(&url).call().map_err(|error| fail(error.to_string()))?;
            let body = response.into_body();
            let total = body.content_length();
            let mut reader = body.into_reader();
            let dir = settings::models_dir()
                .ok_or_else(|| GabrielError::Internal("model storage directory unavailable".into()))?;
            fs::create_dir_all(&dir)?;
            let basename = Path::new(&filename)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("model.bin");
            let destination = dir.join(basename);
            let tmp = dir.join(format!("{basename}.part"));
            let result: Result<Option<(std::path::PathBuf, u64)>> = (|| {
                let mut file = fs::File::create(&tmp)?;
                let mut buf = [0u8; 64 * 1024];
                let mut downloaded: u64 = 0;
                let mut last_emit: u64 = 0;
                on_progress(0, total);
                loop {
                    if cancel.load(Ordering::Relaxed) {
                        return Ok(None);
                    }
                    let n = reader
                        .read(&mut buf)
                        .map_err(|error| fail(error.to_string()))?;
                    if n == 0 {
                        break;
                    }
                    file.write_all(&buf[..n])?;
                    downloaded += n as u64;
                    if downloaded - last_emit >= 1024 * 1024 {
                        last_emit = downloaded;
                        on_progress(downloaded, total);
                    }
                }
                if cancel.load(Ordering::Relaxed) {
                    return Ok(None);
                }
                on_progress(downloaded, total);
                drop(file);
                fs::rename(&tmp, &destination)?;
                Ok(Some((destination, downloaded)))
            })();
            if !matches!(result, Ok(Some(_))) {
                let _ = fs::remove_file(&tmp);
            }
            result
        })
        .await
        .map_err(|error| GabrielError::Internal(error.to_string()))??;
        let Some((destination, downloaded)) = outcome else {
            tracing::info!(repo = %repo_id, file = %log_filename, "Hugging Face download cancelled, partial file removed");
            return Ok(None);
        };
        tracing::info!(repo = %repo_id, file = %log_filename, bytes = downloaded, "downloaded Hugging Face model file");
        let result = self.register_local_model(destination.to_string_lossy().into_owned(), &kind);
        if result.is_ok() {
            tracing::info!(repo = %repo_id, file = %log_filename, "finished Hugging Face model download");
        }
        result.map(Some)
    }

    pub fn telemetry_snapshot(&self) -> TelemetrySnapshot {
        let gpu = self.inner().telemetry.gpu_sample();
        let (ram_total, ram_used) = self.inner().telemetry.ram_sample();
        let cpu = self.inner().telemetry.cpu_usage();
        let (memory_bus_percent, bandwidth_pressure) = self.inner().governor.snapshot();

        let vram_snap = self.inner().pager.vram_snapshot();

        TelemetrySnapshot {
            gpu_name: gpu.name,
            gpu_util_percent: gpu.utilization_percent,
            vram_total_bytes: gpu.total_bytes,
            vram_used_bytes: gpu.used_bytes,
            vram_high_watermark: self.inner().config.read().vram_high_watermark,
            engine_resident_bytes: vram_snap.engine_tracked_bytes,
            vram_untracked_bytes: vram_snap.untracked_bytes,
            vram_peak_untracked_bytes: self.inner().pager.peak_untracked_bytes(),
            memory_bus_percent,
            bandwidth_pressure,
            ram_total_bytes: ram_total,
            ram_used_bytes: ram_used,
            cpu_usage_percent: cpu,
            loaded_models: self.list_models(),
        }
    }

    pub fn config(&self) -> EngineConfig {
        self.inner().config.read().clone()
    }

    pub fn telemetry(&self) -> Arc<Telemetry> {
        self.0.telemetry.clone()
    }

    pub fn registry(&self) -> Arc<RwLock<Registry>> {
        self.0.registry.clone()
    }

    pub fn pager(&self) -> Arc<MemoryPager> {
        self.0.pager.clone()
    }

    pub fn scheduler(&self) -> &Scheduler {
        &self.0.scheduler
    }

    pub fn active_jobs(&self) -> &AtomicUsize {
        &self.0.active_jobs
    }
}
