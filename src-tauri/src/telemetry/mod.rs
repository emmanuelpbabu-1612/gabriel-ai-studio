pub mod gpu;
pub mod sys;

pub use gpu::{GpuMonitor, GpuSample};
pub use sys::SystemSampler;

use parking_lot::Mutex;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct Telemetry {
    gpu: Arc<Box<dyn GpuMonitor + Send + Sync>>,
    system: Arc<Mutex<SystemSampler>>,
}

impl Telemetry {
    pub fn new() -> Self {
        Self {
            gpu: Arc::new(gpu::probe()),
            system: Arc::new(Mutex::new(SystemSampler::new())),
        }
    }

    pub fn gpu_sample(&self) -> GpuSample {
        self.gpu.sample()
    }

    pub fn ram_sample(&self) -> (u64, u64) {
        let mut sys = self.system.lock();
        sys.refresh();
        (sys.total_ram_bytes(), sys.used_ram_bytes())
    }

    pub fn cpu_usage(&self) -> f32 {
        let mut sys = self.system.lock();
        sys.refresh_cpu();
        sys.cpu_usage()
    }
}

impl Default for Telemetry {
    fn default() -> Self {
        Self::new()
    }
}
