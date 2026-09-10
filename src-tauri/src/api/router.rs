use axum::routing::{get, post};
use axum::Router;

use crate::core::engine::EngineState;

use super::routes::{chat, health, images, models_list, speech};

pub fn build_router(engine: EngineState) -> Router {
    Router::new()
        .route("/v1/chat/completions", post(chat::chat_completions))
        .route("/v1/images/generations", post(images::images_generations))
        .route("/v1/audio/speech", post(speech::audio_speech))
        .route("/v1/models", get(models_list::list_models))
        .route("/health", get(health::health))
        .with_state(engine)
}
