use tauri::State;

use crate::core::engine::EngineState;
use crate::error::{GabrielError, Result};
use crate::types::{ModelStatus, ModelType, TelemetrySnapshot};

#[tauri::command]
pub async fn load_model(
    state: State<'_, EngineState>,
    model_id: String,
    model_type: String,
) -> Result<ModelStatus> {
    let model_type = ModelType::parse(&model_type)
        .ok_or_else(|| GabrielError::UnknownModelType(model_type.clone()))?;
    state.load_model(&model_id, model_type, None).await
}

#[tauri::command]
pub async fn unload_model(
    state: State<'_, EngineState>,
    model_id: String,
) -> Result<ModelStatus> {
    state.unload_model(&model_id).await
}

#[tauri::command]
pub async fn offload_model(
    state: State<'_, EngineState>,
    model_id: String,
) -> Result<ModelStatus> {
    state.offload_model(&model_id).await
}

#[tauri::command]
pub async fn get_telemetry(state: State<'_, EngineState>) -> Result<TelemetrySnapshot> {
    Ok(state.telemetry_snapshot())
}

#[tauri::command]
pub async fn list_loaded_models(
    state: State<'_, EngineState>,
) -> Result<Vec<crate::types::ModelRuntimeInfo>> {
    Ok(state.list_models())
}
