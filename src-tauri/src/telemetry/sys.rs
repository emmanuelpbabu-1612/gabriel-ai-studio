use sysinfo::{System, MINIMUM_CPU_UPDATE_INTERVAL};

#[derive(Debug)]
pub struct SystemSampler {
    sys: System,
}

impl Default for SystemSampler {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemSampler {
    pub fn new() -> Self {
        let mut sys = System::new();
        // CPU usage is a delta between two refreshes, so prime it once.
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        std::thread::sleep(MINIMUM_CPU_UPDATE_INTERVAL);
        sys.refresh_cpu_usage();
        Self { sys }
    }

    /// Refresh CPU + memory and return (cpu_percent, total_ram_bytes, used_ram_bytes).
    pub fn sample(&mut self) -> (f32, u64, u64) {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        (
            self.sys.global_cpu_usage(),
            self.sys.total_memory(),
            self.sys.used_memory(),
        )
    }
}