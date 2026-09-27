use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedSettings {
    pub engine_mode: String,
    pub display_name: String,
    pub vram_high_watermark: f64,
    pub vram_low_watermark: f64,
    #[serde(default = "default_bandwidth_ceiling")]
    pub bandwidth_ceiling_percent: f64,
    #[serde(default = "default_max_loaded_models")]
    pub max_loaded_models: usize,
    #[serde(default)]
    pub auto_load_on_request: bool,
    #[serde(default = "default_idle_offload_secs")]
    pub idle_offload_after_secs: u64,
    #[serde(default = "default_app_title")]
    pub app_title: String,
    #[serde(default = "default_startup_route")]
    pub startup_route: String,
    #[serde(default)]
    pub models_dir: Option<String>,
}

fn default_bandwidth_ceiling() -> f64 { 80.0 }
fn default_max_loaded_models() -> usize { 4 }
fn default_idle_offload_secs() -> u64 { 120 }
fn default_app_title() -> String { "Gabriel".into() }
fn default_startup_route() -> String { "/".into() }

impl Default for PersistedSettings {
    fn default() -> Self {
        Self {
            engine_mode: "balanced".into(),
            display_name: "Jackson".into(),
            vram_high_watermark: 85.0,
            vram_low_watermark: 70.0,
            bandwidth_ceiling_percent: default_bandwidth_ceiling(),
            max_loaded_models: default_max_loaded_models(),
            auto_load_on_request: false,
            idle_offload_after_secs: default_idle_offload_secs(),
            app_title: default_app_title(),
            startup_route: default_startup_route(),
            models_dir: None,
        }
    }
}

fn settings_path() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join("Gabriel").join("settings.json"))
}

pub fn load() -> PersistedSettings {
    let Some(path) = settings_path() else { return PersistedSettings::default() };
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save(settings: &PersistedSettings) -> std::io::Result<()> {
    let Some(path) = settings_path() else { return Ok(()) };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_vec_pretty(settings)
        .map_err(|error| std::io::Error::other(error.to_string()))?;
    fs::write(path, json)
}

pub fn apply_mode(settings: &mut PersistedSettings, mode: &str) -> Result<(), String> {
    let mode = mode.to_ascii_lowercase();
    if !matches!(mode.as_str(), "balanced" | "performance" | "efficiency") {
        return Err("engine mode must be balanced, performance, or efficiency".into());
    }
    settings.engine_mode = mode;
    Ok(())
}

pub fn models_dir() -> Option<PathBuf> {
    let s = load();
    if let Some(custom) = s.models_dir {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            return Some(PathBuf::from(trimmed));
        }
    }
    dirs::data_local_dir().map(|dir| dir.join("Gabriel").join("models"))
}