#[derive(Debug, Clone, Default)]
pub struct GpuSample {
    pub name: String,
    pub total_bytes: u64,
    pub used_bytes: u64,
    pub utilization_percent: f32,
    pub memory_bandwidth_percent: f32,
    pub temperature_c: Option<f32>,
    pub power_w: Option<f32>,
    pub fan_percent: Option<u32>,
}

pub trait GpuMonitor: std::fmt::Debug + Send + Sync {
    fn sample(&self) -> GpuSample;
}

#[cfg(all(not(target_os = "macos"), feature = "nvml"))]
mod nvml_monitor {
    use super::{GpuMonitor, GpuSample};
    use nvml_wrapper::enum_wrappers::device::TemperatureSensor;
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

            let (total_bytes, used_bytes) = device
                .memory_info()
                .map(|m| (m.total, m.used))
                .unwrap_or((0, 0));

            let (gpu_util, mem_util) = device
                .utilization_rates()
                .map(|u| (u.gpu as f32, u.memory as f32))
                .unwrap_or((0.0, 0.0));

            GpuSample {
                name,
                total_bytes,
                used_bytes,
                utilization_percent: gpu_util,
                memory_bandwidth_percent: mem_util,
                temperature_c: device
                    .temperature(TemperatureSensor::Gpu)
                    .ok()
                    .map(|t| t as f32),
                // NVML reports milliwatts
                power_w: device.power_usage().ok().map(|mw| mw as f32 / 1000.0),
                // Percentage of max fan speed, NOT RPM
                fan_percent: device.fan_speed(0).ok(),
            }
        }
    }

    pub fn create() -> Option<Box<dyn GpuMonitor>> {
        NvmlMonitor::new().map(|m| Box::new(m) as Box<dyn GpuMonitor>)
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
                ..GpuSample::default()
            }
        }
    }

    pub fn create() -> Option<Box<dyn GpuMonitor>> {
        MetalMonitor::new().map(|m| Box::new(m) as Box<dyn GpuMonitor>)
    }
}

#[derive(Debug)]
pub struct FallbackMonitor;

impl GpuMonitor for FallbackMonitor {
    fn sample(&self) -> GpuSample {
        GpuSample {
            name: "no-gpu-backend".to_string(),
            ..GpuSample::default()
        }
    }
}

#[cfg(all(not(target_os = "macos"), feature = "nvml"))]
fn platform_probe() -> Option<Box<dyn GpuMonitor>> {
    nvml_monitor::create()
}

#[cfg(all(target_os = "macos", feature = "metal"))]
fn platform_probe() -> Option<Box<dyn GpuMonitor>> {
    metal_monitor::create()
}

#[cfg(not(any(
    all(not(target_os = "macos"), feature = "nvml"),
    all(target_os = "macos", feature = "metal")
)))]
fn platform_probe() -> Option<Box<dyn GpuMonitor>> {
    None
}

pub fn probe() -> Box<dyn GpuMonitor> {
    platform_probe().unwrap_or_else(|| {
        tracing::warn!("no GPU monitor backend available; GPU telemetry degraded");
        Box::new(FallbackMonitor)
    })
}