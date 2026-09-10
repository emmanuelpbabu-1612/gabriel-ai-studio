#[derive(Debug, Clone, Default)]
pub struct GpuSample {
    pub name: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub utilization_percent: f32,
    pub memory_bandwidth_percent: f32,
}

pub trait GpuMonitor: std::fmt::Debug {
    fn sample(&self) -> GpuSample;
}

#[cfg(all(not(target_os = "macos"), feature = "nvml"))]
mod nvml_monitor {
    use super::{GpuMonitor, GpuSample};
    use nvml_wrapper::Nvml;
    use parking_lot::Mutex;

    #[derive(Debug)]
    pub struct NvmlMonitor {
        nvml: Mutex<Nvml>,
    }

    impl NvmlMonitor {
        pub fn new() -> Option<Self> {
            Nvml::init().ok().map(|nvml| Self {
                nvml: Mutex::new(nvml),
            })
        }
    }

    impl GpuMonitor for NvmlMonitor {
        fn sample(&self) -> GpuSample {
            let guard = self.nvml.lock();
            let Ok(device) = guard.device_by_index(0) else {
                return GpuSample::default();
            };
            let name = device
                .name()
                .unwrap_or_else(|_| "unknown-nvidia-gpu".to_string());
            let Some(mem) = device.memory_info().ok() else {
                return GpuSample {
                    name,
                    ..GpuSample::default()
                };
            };
            let util = device
                .utilization_rates()
                .map(|u| (u.gpu as f32, u.memory as f32))
                .unwrap_or((0.0, 0.0));
            GpuSample {
                name,
                total_bytes: mem.total,
                used_bytes: mem.used,
                utilization_percent: util.0,
                memory_bandwidth_percent: util.1,
            }
        }
    }
}

#[cfg(all(target_os = "macos", feature = "metal"))]
mod metal_monitor {
    use super::{GpuMonitor, GpuSample};
    use metal::Device;

    #[derive(Debug)]
    pub struct MetalMonitor;

    impl MetalMonitor {
        pub fn new() -> Option<Self> {
            Device::all().first().map(|_| Self)
        }
    }

    impl GpuMonitor for MetalMonitor {
        fn sample(&self) -> GpuSample {
            let Some(device) = Device::all().into_iter().next() else {
                return GpuSample::default();
            };
            GpuSample {
                name: device.name().to_string(),
                total_bytes: device.recommended_max_working_set_size(),
                used_bytes: device.current_allocated_size(),
                utilization_percent: 0.0,
            }
        }
    }
}

#[derive(Debug)]
pub struct FallbackMonitor;

impl GpuMonitor for FallbackMonitor {
    fn sample(&self) -> GpuSample {
        GpuSample {
            name: "unified-memory".to_string(),
            ..GpuSample::default()
        }
    }
}

pub fn probe() -> Box<dyn GpuMonitor + Send + Sync> {
    #[cfg(all(not(target_os = "macos"), feature = "nvml"))]
    if let Some(m) = nvml_monitor::NvmlMonitor::new() {
        return Box::new(m);
    }

    #[cfg(all(target_os = "macos", feature = "metal"))]
    if let Some(m) = metal_monitor::MetalMonitor::new() {
        return Box::new(m);
    }

    tracing::warn!("no GPU monitor backend available; VRAM budgeting degraded");
    Box::new(FallbackMonitor)
}
