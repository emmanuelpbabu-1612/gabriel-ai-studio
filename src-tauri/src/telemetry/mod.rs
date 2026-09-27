pub mod gpu;
pub mod sys;

pub use gpu::{GpuMonitor, GpuSample};
pub use sys::SystemSampler;

use parking_lot::Mutex;
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

const GIB: f64 = 1024.0 * 1024.0 * 1024.0;

#[derive(Debug, Clone, Serialize)]
pub struct TelemetrySnapshot {
    pub cpu_load: f32,
    pub gpu_load: f32,
    pub gpu_name: String,
    pub vram_used_gb: f32,
    pub vram_total_gb: f32,
    pub ram_used_gb: f32,
    pub ram_total_gb: f32,
    pub temp_c: Option<f32>,
    pub power_w: Option<f32>,
    pub fan_percent: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct Telemetry {
    gpu: Arc<dyn GpuMonitor>,
    system: Arc<Mutex<SystemSampler>>,
}

impl Telemetry {
    pub fn new() -> Self {
        Self {
            gpu: Arc::from(gpu::probe()),
            system: Arc::new(Mutex::new(SystemSampler::new())),
        }
    }

    pub fn gpu_sample(&self) -> GpuSample {
        self.gpu.sample()
    }

    pub fn ram_sample(&self) -> (u64, u64) {
        let (_, total, used) = self.system.lock().sample();
        (total, used)
    }

    pub fn cpu_usage(&self) -> f32 {
        self.system.lock().sample().0
    }

    pub fn snapshot(&self) -> TelemetrySnapshot {
        let (cpu, ram_total, ram_used) = self.system.lock().sample();
        let gpu = self.gpu.sample();

        TelemetrySnapshot {
            cpu_load: cpu,
            gpu_load: gpu.utilization_percent,
            gpu_name: gpu.name,
            vram_used_gb: (gpu.used_bytes as f64 / GIB) as f32,
            vram_total_gb: (gpu.total_bytes as f64 / GIB) as f32,
            ram_used_gb: (ram_used as f64 / GIB) as f32,
            ram_total_gb: (ram_total as f64 / GIB) as f32,
            temp_c: gpu.temperature_c,
            power_w: gpu.power_w,
            fan_percent: gpu.fan_percent,
        }
    }
}

impl Default for Telemetry {
    fn default() -> Self {
        Self::new()
    }
}

#[tauri::command(rename = "get_telemetry")]
pub fn get_system_telemetry(state: tauri::State<'_, Telemetry>) -> TelemetrySnapshot {
    state.snapshot()
}

/// Plain OS thread: no dependency on a Tokio runtime being active.
pub fn spawn_emitter(app: AppHandle, telemetry: Telemetry) {
    std::thread::Builder::new()
        .name("telemetry-emitter".into())
        .spawn(move || loop {
            let snapshot = telemetry.snapshot();
            if let Err(e) = app.emit("telemetry-update", snapshot) {
                tracing::warn!("telemetry emit failed: {e}");
            }
            std::thread::sleep(Duration::from_secs(1));
        })
        .expect("failed to spawn telemetry thread");
}