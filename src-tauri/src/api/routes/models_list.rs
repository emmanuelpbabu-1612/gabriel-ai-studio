
use axum::extract::State;
use axum::Json;

use crate::core::engine::EngineState;
use crate::types::openai::{
    GabrielModelMeta, ModelListEntry, ModelListEntryMeta,
};

pub async fn list_models(
    State(engine): State<EngineState>,
) -> Json<ModelListEntryMeta> {
    let telemetry = engine.telemetry_snapshot();

    let entries = telemetry
        .loaded_models
        .iter()
        .map(|m| ModelListEntry {
            id: m.id.clone(),
            object: "model".into(),
            owned_by: "gabriel".into(),
            gabriel: GabrielModelMeta {
                model_type: format!("{:?}", m.model_type).to_lowercase(),
                residency: match m.residency {
                    crate::types::Residency::Gpu => "vram".into(),
                    crate::types::Residency::Cpu => "ram".into(),
                },
                vram_bytes: m.vram_bytes,
            },
        })
        .collect();

    Json(ModelListEntryMeta::new(entries))
}
