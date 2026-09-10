use sysinfo::System;

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
        sys.refresh_memory();
        sys.refresh_cpu_usage();
        std::thread::sleep(std::time::Duration::from_millis(50));
        sys.refresh_cpu_usage();
        Self { sys }
    }

    pub fn refresh(&mut self) {
        self.sys.refresh_memory();
    }

    pub fn refresh_cpu(&mut self) {
        self.sys.refresh_cpu_usage();
    }

    pub fn total_ram_bytes(&self) -> u64 {
        self.sys.total_memory()
    }

    pub fn used_ram_bytes(&self) -> u64 {
        self.sys.used_memory()
    }

    pub fn cpu_usage(&self) -> f32 {
        self.sys.global_cpu_usage()
    }
}
