use std::convert::Infallible;

use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::Json;
use futures::StreamExt;
use tokio::sync::mpsc::Receiver;
use tokio_stream::wrappers::ReceiverStream;

use crate::core::engine::EngineState;
use crate::error::{ApiError, GabrielError};
use axum::response::IntoResponse;
use crate::types::openai::{ChatChunk, ChatCompletionRequest, ChatCompletionResponse};
use crate::types::{ChatEvent, GenParams};

const MAX_PROMPT_CHARS: usize = 64_000;
const MAX_TOKENS_CAP: u32 = 8_192;

pub async fn chat_completions(
    State(engine): State<EngineState>,
    Json(req): Json<ChatCompletionRequest>,
) -> Result<axum::response::Response, ApiError> {
    if req.messages.is_empty() {
        return Err(ApiError(GabrielError::InvalidRequest(
            "messages must not be empty".into(),
        )));
    }

    let prompt = req.flatten_prompt();
    if prompt.chars().count() > MAX_PROMPT_CHARS {
        return Err(ApiError(GabrielError::InvalidRequest(format!(
            "prompt exceeds the {MAX_PROMPT_CHARS} character limit"
        ))));
    }

    let params = GenParams {
        max_tokens: req.max_tokens.unwrap_or(512).clamp(1, MAX_TOKENS_CAP),
        temperature: req.temperature.clamp(0.0, 2.0),
    };
    let model_id = req.model.clone();

    let rx = engine
        .submit_chat(&model_id, prompt, params)
        .await
        .map_err(ApiError)?;

    if req.stream {
        Ok(sse_response(rx, &model_id))
    } else {
        collect_response(rx, &model_id).await
    }
}

fn sse_response(
    rx: Receiver<ChatEvent>,
    model_id: &str,
) -> axum::response::Response {
    let model = model_id.to_string();
    let model_for_initial = model.clone();
    let initial = futures::stream::once(async move {
        Ok::<_, Infallible>(to_sse_event(
            ChatEvent::Token(String::new()),
            &model_for_initial,
        ))
    });
    let tokens = ReceiverStream::new(rx).map(move |ev| Ok::<_, Infallible>(to_sse_event(ev, &model)));
    let terminator =
        futures::stream::once(async { Ok::<_, Infallible>(Event::default().data("[DONE]")) });
    let sse = Sse::new(initial.chain(tokens).chain(terminator)).keep_alive(KeepAlive::default());

    sse.into_response()
}

fn to_sse_event(ev: ChatEvent, model: &str) -> Event {
    match ev {
        ChatEvent::Token(t) if t.is_empty() => chunk_event(ChatChunk::initial(model)),
        ChatEvent::Token(t) => chunk_event(ChatChunk::token(model, &t)),
        ChatEvent::Done { finish_reason } => {
            chunk_event(ChatChunk::final_chunk(model, &finish_reason))
        }
        ChatEvent::Failed(msg) => Event::default()
            .event("error")
            .data(serde_json::json!({ "error": msg }).to_string()),
    }
}

fn chunk_event(chunk: ChatChunk) -> Event {
    match serde_json::to_string(&chunk) {
        Ok(json) => Event::default().data(json),
        Err(_) => Event::default().data("{}"),
    }
}

async fn collect_response(
    mut rx: Receiver<ChatEvent>,
    model: &str,
) -> Result<axum::response::Response, ApiError> {
    let mut content = String::new();
    let mut failed: Option<String> = None;
    let mut completion_tokens = 0u32;

    while let Some(ev) = rx.recv().await {
        match ev {
            ChatEvent::Token(t) if !t.is_empty() => {
                content.push_str(&t);
                completion_tokens += 1;
            }
            ChatEvent::Failed(m) => {
                failed = Some(m);
                break;
            }
            ChatEvent::Done { .. } | ChatEvent::Token(_) => {}
        }
    }

    if let Some(err) = failed {
        return Err(ApiError(GabrielError::Backend(err)));
    }

    let prompt_tokens = (content.len() as u32 / 4).max(1);
    let body = ChatCompletionResponse::new(model, &content, prompt_tokens, completion_tokens);
    Ok(axum::Json(body).into_response())
}
