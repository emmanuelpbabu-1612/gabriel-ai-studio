pub mod bandwidth;
pub mod engine;
pub mod pager;
pub mod registry;
pub mod scheduler;

use std::time::Duration;

#[derive(Debug, Clone)]
pub struct EngineConfig {
    pub host: String,
    pub port: u16,
    pub vram_high_watermark: f64,
    pub vram_low_watermark: f64,
    pub idle_offload_after: Duration,
    pub pager_poll_interval: Duration,
    pub queue_capacity: usize,
    pub max_concurrent_image_jobs: usize,
    pub auto_load_on_request: bool,
    pub bandwidth_ceiling_percent: f64,
    pub max_bandwidth_yield: Duration,
    pub max_loaded_models: usize,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: 8080,
            vram_high_watermark: 0.85,
            vram_low_watermark: 0.70,
            idle_offload_after: Duration::from_secs(120),
            pager_poll_interval: Duration::from_secs(2),
            queue_capacity: 256,
            max_concurrent_image_jobs: 1,
            auto_load_on_request: false,
            bandwidth_ceiling_percent: 80.0,
            max_bandwidth_yield: Duration::from_millis(250),
            max_loaded_models: 4,
        }
    }
}
