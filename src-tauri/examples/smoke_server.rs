use gabriel_lib::core::engine::EngineState;
use gabriel_lib::types::ModelType;

#[tokio::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("info,gabriel_lib=debug")
        .init();

    let engine = EngineState::new(gabriel_lib::core::EngineConfig::default());
    engine
        .load_model("gabriel-mini", ModelType::Llm, Some(512 * 1024 * 1024))
        .await
        .expect("load llm");
    engine
        .load_model("gabriel-diffusion", ModelType::Image, Some(384 * 1024 * 1024))
        .await
        .expect("load image");
    engine
        .load_model("gabriel-voice", ModelType::Tts, Some(64 * 1024 * 1024))
        .await
        .expect("load tts");

    gabriel_lib::api::serve(engine).await
}
