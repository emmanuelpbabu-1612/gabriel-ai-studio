use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub stream: bool,
    pub max_tokens: Option<u32>,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
}

fn default_temperature() -> f32 {
    0.7
}

impl ChatCompletionRequest {
    pub fn flatten_prompt(&self) -> String {
        self.messages
            .iter()
            .map(|m| format!("{}: {}", m.role, m.content))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

fn usage(prompt_tokens: u32, completion_tokens: u32) -> Usage {
    Usage {
        prompt_tokens,
        completion_tokens,
        total_tokens: prompt_tokens + completion_tokens,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChoiceDelta {
    pub index: u32,
    pub delta: Delta,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Delta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

impl Delta {
    fn role(r: &str) -> Self {
        Self {
            role: Some(r.to_string()),
            content: None,
        }
    }
    fn text(t: &str) -> Self {
        Self {
            role: None,
            content: Some(t.to_string()),
        }
    }
    fn empty() -> Self {
        Self {
            role: None,
            content: None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Choice {
    pub index: u32,
    pub message: AssistantMessage,
    pub finish_reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssistantMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<Choice>,
    pub usage: Usage,
}

impl ChatCompletionResponse {
    pub fn new(model: &str, content: &str, prompt_tokens: u32, completion_tokens: u32) -> Self {
        Self {
            id: format!("chatcmpl-{}", uuid::Uuid::new_v4().simple()),
            object: "chat.completion".into(),
            created: crate::types::ipc::unix_now(),
            model: model.into(),
            choices: vec![Choice {
                index: 0,
                message: AssistantMessage {
                    role: "assistant".into(),
                    content: content.into(),
                },
                finish_reason: "stop".into(),
            }],
            usage: usage(prompt_tokens, completion_tokens),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatChunk {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChoiceDelta>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

impl ChatChunk {
    pub fn initial(model: &str) -> Self {
        Self::with_delta(model, Delta::role("assistant"), None)
    }

    pub fn token(model: &str, token: &str) -> Self {
        Self::with_delta(model, Delta::text(token), None)
    }

    pub fn final_chunk(model: &str, finish_reason: &str) -> Self {
        Self::with_delta(
            model,
            Delta::empty(),
            Some(finish_reason.to_string()),
        )
    }

    fn with_delta(model: &str, delta: Delta, finish_reason: Option<String>) -> Self {
        Self {
            id: format!("chatcmpl-{}", uuid::Uuid::new_v4().simple()),
            object: "chat.completion.chunk".into(),
            created: crate::types::ipc::unix_now(),
            model: model.into(),
            choices: vec![ChoiceDelta {
                index: 0,
                delta,
                finish_reason,
            }],
            usage: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ImageGenerationRequest {
    pub prompt: String,
    pub model: Option<String>,
    #[serde(default = "default_n")]
    pub n: u32,
    pub size: Option<String>,
}

fn default_n() -> u32 {
    1
}

#[derive(Debug, Clone, Serialize)]
pub struct ImageData {
    pub b64_json: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImageGenerationResponse {
    pub created: u64,
    pub data: Vec<ImageData>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SpeechRequest {
    pub model: String,
    pub input: String,
    pub voice: Option<String>,
    #[serde(default = "default_speed")]
    pub speed: f32,
}

fn default_speed() -> f32 {
    1.0
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelListEntry {
    pub id: String,
    pub object: String,
    pub owned_by: String,
    pub gabriel: GabrielModelMeta,
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelListEntryMeta {
    pub object: String,
    pub data: Vec<ModelListEntry>,
}

impl ModelListEntryMeta {
    pub fn new(entries: Vec<ModelListEntry>) -> Self {
        Self {
            object: "list".into(),
            data: entries,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct GabrielModelMeta {
    pub model_type: String,
    pub residency: String,
    pub vram_bytes: u64,
}
