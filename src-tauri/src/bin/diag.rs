use gabriel_lib::core::EngineConfig;
use gabriel_lib::core::engine::EngineState;
use gabriel_lib::types::ModelType;

#[tokio::main]
async fn main() {
    let mut config = EngineConfig::default();
    config.max_loaded_models = 8;
    config.vram_high_watermark = 0.85;
    let engine = EngineState::new(config);
    
    engine.load_model("qwen-2.5-3b-instruct-q4_k_m.gguf", ModelType::Llm, Some(1_900_000_000)).await.unwrap();
    let s1 = engine.telemetry_snapshot();
    println!("after llm: resident={} models={:?}", s1.engine_resident_bytes, s1.loaded_models);
    
    engine.load_model("sd-turbo", ModelType::Image, Some(1_600_000_000)).await.unwrap();
    let s2 = engine.telemetry_snapshot();
    println!("after image: resident={} models={:?}", s2.engine_resident_bytes, s2.loaded_models);
    
    engine.load_model("parler-tts", ModelType::Tts, Some(450_000_000)).await.unwrap();
    let s3 = engine.telemetry_snapshot();
    println!("after tts: resident={} models={:?}", s3.engine_resident_bytes, s3.loaded_models);
    println!("vram_total {} vram_used {}", s3.vram_total_bytes, s3.vram_used_bytes);
}
