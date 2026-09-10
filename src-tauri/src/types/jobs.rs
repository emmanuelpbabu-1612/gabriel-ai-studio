use serde::{Deserialize, Serialize};

use super::ipc::Residency;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Interactive,
    Standard,
}

impl Priority {
    pub fn level(&self) -> i8 {
        match self {
            Self::Interactive => -1,
            Self::Standard => 0,
        }
    }
}

pub type JobId = uuid::Uuid;

#[derive(Debug)]
pub enum JobKind {
    Chat {
        prompt: String,
        params: GenParams,
        events: tokio::sync::mpsc::Sender<ChatEvent>,
    },
    Image {
        prompt: String,
        width: u32,
        height: u32,
        reply: tokio::sync::oneshot::Sender<Result<Vec<u8>, crate::error::GabrielError>>,
    },
    Speech {
        text: String,
        voice: String,
        reply: tokio::sync::oneshot::Sender<Result<Vec<u8>, crate::error::GabrielError>>,
        /// Adaptive scheduler hint: true = CPU fallback (VRAM tight), false = GPU
        cpu_fallback: bool,
    },
}

pub struct Job {
    pub id: JobId,
    pub priority: Priority,
    pub model_id: String,
    pub kind: JobKind,
}

impl std::fmt::Debug for Job {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Job")
            .field("id", &self.id)
            .field("priority", &self.priority)
            .field("model_id", &self.model_id)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct GenParams {
    pub max_tokens: u32,
    pub temperature: f32,
}

impl Default for GenParams {
    fn default() -> Self {
        Self {
            max_tokens: 512,
            temperature: 0.7,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ChatEvent {
    Token(String),
    Done { finish_reason: String },
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum QueueDepth {
    Interactive { pending: usize },
    Standard { pending: usize },
}

#[allow(dead_code)]
pub fn residency_label(r: Residency) -> &'static str {
    match r {
        Residency::Gpu => "vram",
        Residency::Cpu => "ram",
    }
}
