pub mod api;
pub mod core;
pub mod error;
pub mod inference;
pub mod ipc;
pub mod telemetry;
pub mod types;

use core::engine::EngineState;

#[cfg(debug_assertions)]
fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,gabriel_lib=debug".into()),
        )
        .init();
}

#[cfg(not(debug_assertions))]
fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_tracing();

    let engine = EngineState::new(core::EngineConfig::default());

    tauri::Builder::default()
        .manage(engine.clone())
        .setup(move |_app| {
            let engine = engine.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = api::serve(engine).await {
                    tracing::error!("REST server terminated: {e}");
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc::commands::load_model,
            ipc::commands::unload_model,
            ipc::commands::offload_model,
            ipc::commands::get_telemetry,
            ipc::commands::list_loaded_models,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Gabriel");
}
