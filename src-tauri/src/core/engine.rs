use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use parking_lot::RwLock;
use tokio::sync::{Semaphore, mpsc, oneshot};

use crate::error::{GabrielError, Result};
use crate::inference::{BackendFactory, ImageBackend, SpeechBackend, TextBackend, hub};
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
    pub config: EngineConfig,
    pub registry: Arc<RwLock<Registry>>,
    pub telemetry: Arc<Telemetry>,
    pub pager: Arc<MemoryPager>,
    pub governor: Arc<BandwidthGovernor>,
    pub scheduler: Scheduler,
    pub factory: BackendFactory,
    image_permits: Arc<Semaphore>,
    active_jobs: AtomicUsize,
    load_gate: tokio::sync::Mutex<()>,
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
            config: config.clone(),
            registry: registry.clone(),
            telemetry: telemetry.clone(),
            pager: pager.clone(),
            governor: governor.clone(),
            scheduler,
            factory: BackendFactory::new(governor.clone()),
        });

        {
            let governor = governor.clone();
            let telemetry = telemetry.clone();
            let interval = config.pager_poll_interval;
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
            host = %config.host,
            port = config.port,
            high_watermark = config.vram_high_watermark,
            "engine initialized"
        );

        Self(inner)
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

    pub async fn load_model(
        &self,
        model_id: &str,
        model_type: ModelType,
        vram_bytes: Option<u64>,
    ) -> Result<ModelStatus> {
        let _gate = self.inner().load_gate.lock().await;

        if self.inner().registry.read().contains(model_id) {
            return Err(GabrielError::AlreadyLoaded(model_id.to_string()));
        }

        let slot_limit = self.inner().config.max_loaded_models;

        // Eviction with infinite loop safeguard
        let mut attempts = 0;
        while self.inner().registry.read().len() >= slot_limit {
            attempts += 1;
            if attempts > slot_limit * 2 {
                return Err(GabrielError::SlotPoolExhausted { limit: slot_limit });
            }

            let victim = {
                let reg = self.inner().registry.read();
                reg.least_recently_used_any_idle(self.inner().config.idle_offload_after)
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

        let spec = ModelSpec::new(model_id, model_type, vram_bytes);
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
        }

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

        if !self.inner().config.auto_load_on_request {
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
        self.inner().registry.read().snapshot()
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
            vram_high_watermark: self.inner().config.vram_high_watermark,
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

    pub fn config(&self) -> &EngineConfig {
        &self.inner().config
    }
}
