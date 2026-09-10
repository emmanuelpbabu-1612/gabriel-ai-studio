use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::{Mutex, RwLock};

use crate::error::{GabrielError, Result};
use crate::telemetry::Telemetry;

use super::registry::Registry;

#[derive(Debug, Clone)]
pub struct PagerConfig {
    pub high_watermark: f64,
    pub low_watermark: f64,
    pub idle_offload_after: Duration,
    pub poll_interval: Duration,
    /// Safety margin for dynamic allocations (KV cache, activations, etc.)
    /// Default: 512 MB to account for runtime memory growth
    pub vram_safety_margin_bytes: u64,
    /// Enable preflight memory check before model load
    pub preflight_check_enabled: bool,
}

impl PagerConfig {
    pub fn new(
        high_watermark: f64,
        low_watermark: f64,
        idle_offload_after: Duration,
        poll_interval: Duration,
    ) -> Self {
        Self {
            high_watermark,
            low_watermark,
            idle_offload_after,
            poll_interval,
            vram_safety_margin_bytes: 512 * 1024 * 1024, // 512 MB safety margin
            preflight_check_enabled: true,
        }
    }
}

/// Memory pressure level for eviction decisions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryPressure {
    /// Below low watermark, no pressure
    None,
    /// Between low and high watermark, moderate pressure
    Moderate,
    /// Above high watermark, critical pressure
    Critical,
}

/// Runtime VRAM snapshot including measured vs estimated usage
#[derive(Debug, Clone)]
pub struct VramSnapshot {
    /// Total GPU memory in bytes
    pub total_bytes: u64,
    /// Currently used GPU memory (from NVML/driver)
    pub used_bytes: u64,
    /// Memory tracked by engine (sum of model weights)
    pub engine_tracked_bytes: u64,
    /// Untracked memory (KV cache, activations, fragmentation)
    pub untracked_bytes: u64,
    /// Current pressure level
    pub pressure: MemoryPressure,
    /// Utilization ratio (0.0 - 1.0)
    pub utilization_ratio: f64,
}

pub struct MemoryPager {
    config: PagerConfig,
    telemetry: Arc<Telemetry>,
    registry: Arc<RwLock<Registry>>,
    /// Bytes tracked by the engine (model weights only)
    engine_resident_bytes: Arc<Mutex<u64>>,
    /// Peak observed untracked memory (for adaptive safety margin)
    peak_untracked_bytes: Arc<Mutex<u64>>,
    /// Last snapshot time for caching
    last_snapshot: Arc<Mutex<Option<(Instant, VramSnapshot)>>>,
}

impl std::fmt::Debug for MemoryPager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemoryPager")
            .field("config", &self.config)
            .field("engine_resident_bytes", &*self.engine_resident_bytes.lock())
            .finish()
    }
}

impl Clone for MemoryPager {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            telemetry: self.telemetry.clone(),
            registry: self.registry.clone(),
            engine_resident_bytes: self.engine_resident_bytes.clone(),
            peak_untracked_bytes: self.peak_untracked_bytes.clone(),
            last_snapshot: self.last_snapshot.clone(),
        }
    }
}

impl MemoryPager {
    pub fn new(
        config: PagerConfig,
        telemetry: Arc<Telemetry>,
        registry: Arc<RwLock<Registry>>,
    ) -> Self {
        Self {
            config,
            telemetry,
            registry,
            engine_resident_bytes: Arc::new(Mutex::new(0)),
            peak_untracked_bytes: Arc::new(Mutex::new(0)),
            last_snapshot: Arc::new(Mutex::new(None)),
        }
    }

    pub fn engine_resident_bytes(&self) -> u64 {
        *self.engine_resident_bytes.lock()
    }

    /// Peak observed untracked memory (KV cache, activations, fragmentation)
    pub fn peak_untracked_bytes(&self) -> u64 {
        *self.peak_untracked_bytes.lock()
    }

    fn charge(&self, bytes: u64) {
        let mut guard = self.engine_resident_bytes.lock();
        *guard = guard.saturating_add(bytes);
        // Invalidate cached snapshot so next vram_snapshot sees fresh ledger
        *self.last_snapshot.lock() = None;
    }

    fn discharge(&self, bytes: u64) {
        let mut guard = self.engine_resident_bytes.lock();
        *guard = guard.saturating_sub(bytes);
        *self.last_snapshot.lock() = None;
    }

    pub fn on_promoted(&self, vram_bytes: u64) {
        self.charge(vram_bytes);
    }

    pub fn on_demoted_or_unloaded(&self, vram_bytes: u64) {
        self.discharge(vram_bytes);
    }

    /// Compute a live VRAM snapshot, distinguishing engine-tracked allocations
    /// from untracked dynamic memory (KV cache, activations, fragmentation).
    ///
    /// This mirrors Lemonade's GlobalVramMonitor: we poll the driver for the
    /// real used/total numbers instead of trusting our own ledger. The gap
    /// between driver-reported usage and our ledger is the untracked memory
    /// that the old pager ignored entirely.
    pub fn vram_snapshot(&self) -> VramSnapshot {
        if let Some((at, snap)) = &*self.last_snapshot.lock() {
            if at.elapsed() < Duration::from_millis(50) {
                return snap.clone();
            }
        }

        let sample = self.telemetry.gpu_sample();
        let tracked = self.engine_resident_bytes();
        let mut snap = VramSnapshot {
            total_bytes: sample.total_bytes,
            used_bytes: sample.used_bytes,
            engine_tracked_bytes: tracked,
            untracked_bytes: 0,
            pressure: MemoryPressure::None,
            utilization_ratio: 0.0,
        };

        if sample.total_bytes > 0 {
            snap.untracked_bytes = sample.used_bytes.saturating_sub(tracked);
            snap.utilization_ratio = sample.used_bytes as f64 / sample.total_bytes as f64;
            snap.pressure = if snap.utilization_ratio >= self.config.high_watermark {
                MemoryPressure::Critical
            } else if snap.utilization_ratio >= self.config.low_watermark {
                MemoryPressure::Moderate
            } else {
                MemoryPressure::None
            };

            // Track peak untracked memory for adaptive safety margin
            let mut peak = self.peak_untracked_bytes.lock();
            if snap.untracked_bytes > *peak {
                *peak = snap.untracked_bytes;
            }
        }

        *self.last_snapshot.lock() = Some((Instant::now(), snap.clone()));
        snap
    }

    /// Effective headroom available for a new allocation, accounting for the
    /// configured safety margin (dynamic allocations during inference).
    pub fn headroom_bytes(&self) -> u64 {
        let snap = self.vram_snapshot();
        if snap.total_bytes == 0 {
            return u64::MAX;
        }
        let cap = (snap.total_bytes as f64 * self.config.high_watermark) as u64;
        let margin = self.config.vram_safety_margin_bytes;
        cap.saturating_sub(snap.used_bytes).saturating_sub(margin)
    }

    /// Preflight check: does the requested model fit within the high-watermark
    /// budget, accounting for current pressure and the safety margin?
    ///
    /// Lemonade's PR #1744 performs a base-model memory preflight before
    /// launching any backend. We do the same here using the driver's real
    /// usage numbers.
    pub fn preflight(&self, incoming_bytes: u64) -> Result<()> {
        let snap = self.vram_snapshot();

        if snap.total_bytes == 0 {
            tracing::debug!("pager: degraded telemetry mode, skipping preflight");
            return Ok(());
        }

        let budget_cap = (snap.total_bytes as f64 * self.config.high_watermark) as u64;

        if incoming_bytes > budget_cap {
            return Err(GabrielError::VramExhausted {
                required_bytes: incoming_bytes,
                available_bytes: budget_cap,
            });
        }

        let headroom = self.headroom_bytes();
        if headroom >= incoming_bytes {
            return Ok(());
        }

        // Not enough headroom: try to evict idle models first (Lemonade's
        // EvictionEngine behavior), then re-check.
        let mut freed = 0u64;
        let candidates = self
            .registry
            .read()
            .evictable_idle(self.config.idle_offload_after);
        for id in candidates {
            if freed >= incoming_bytes.saturating_sub(headroom) {
                break;
            }
            let entry_bytes = {
                let reg = self.registry.read();
                reg.get(&id).map(|e| e.spec.vram_bytes).unwrap_or(0)
            };
            tracing::info!(
                model = %id,
                freed_bytes = entry_bytes,
                "pager: preflight eviction of idle model"
            );
            self.demote(&id);
            freed = freed.saturating_add(entry_bytes);
        }

        let snap_after = self.vram_snapshot();
        let headroom_after = {
            let cap = (snap_after.total_bytes as f64 * self.config.high_watermark) as u64;
            let margin = self.config.vram_safety_margin_bytes;
            cap.saturating_sub(snap_after.used_bytes)
                .saturating_sub(margin)
        };
        if headroom_after >= incoming_bytes {
            return Ok(());
        }

        Err(GabrielError::VramExhausted {
            required_bytes: incoming_bytes,
            available_bytes: headroom_after,
        })
    }

    pub fn admit(&self, incoming_bytes: u64) -> Result<()> {
        if !self.config.preflight_check_enabled {
            return Ok(());
        }
        self.preflight(incoming_bytes)
    }

    pub fn demote(&self, id: &str) {
        let entry_bytes = {
            let reg = self.registry.read();
            match reg.get(id) {
                Some(e) if e.residency == crate::types::Residency::Gpu => e.spec.vram_bytes,
                _ => return,
            }
        };
        self.registry.write().demote(id);
        self.discharge(entry_bytes);
    }

    pub fn promote(&self, id: &str, handle: super::engine::ModelHandle) {
        let bytes = {
            let reg = self.registry.read();
            match reg.get(id) {
                Some(e) => e.spec.vram_bytes,
                None => return,
            }
        };
        self.registry.write().promote(id, handle);
        self.charge(bytes);
    }

    pub fn run_maintenance_pass(&self) {
        let snap = self.vram_snapshot();
        if snap.total_bytes == 0 {
            return;
        }

        // End a call "pass" and measure against driver-reported usage, not the
        // internal ledger. Lemonade's GlobalVramMonitor drives eviction from
        // real GPU pressure, precisely because the ledger drifts from reality
        // during inference (KV cache growth, activation memory, fragmentation).
        if snap.utilization_ratio <= self.config.high_watermark {
            return;
        }

        let target_used = (snap.total_bytes as f64 * self.config.low_watermark) as u64;
        let mut to_free = snap.used_bytes.saturating_sub(target_used);

        let candidates = self
            .registry
            .read()
            .evictable_idle(self.config.idle_offload_after);

        for id in candidates {
            if to_free == 0 {
                break;
            }
            let freed = {
                let reg = self.registry.read();
                reg.get(&id).map(|e| e.spec.vram_bytes).unwrap_or(0)
            };
            tracing::info!(
                model = %id,
                freed_bytes = freed,
                pressure_ratio = snap.utilization_ratio,
                "pager: offloading idle model VRAM -> RAM (driver pressure)"
            );
            self.demote(&id);
            to_free = to_free.saturating_sub(freed);
        }
    }

    pub fn spawn_loop(self: &Arc<Self>) {
        let pager = self.clone();
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(pager.config.poll_interval);
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                tick.tick().await;
                pager.run_maintenance_pass();
            }
        });
    }
}
