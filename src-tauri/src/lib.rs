pub mod api;
pub mod core;
pub mod error;
pub mod inference;
pub mod ipc;
pub mod telemetry;
pub mod types;

use tauri::Manager;
use core::engine::EngineState;
use telemetry::Telemetry;

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

    tauri::Builder::default()
        .setup(|app| {
            let engine = tauri::async_runtime::block_on(async {
                EngineState::new(core::EngineConfig::default())
            });
            app.manage(engine.clone());

            let telemetry = Telemetry::new();
            app.manage(telemetry.clone());
            telemetry::spawn_emitter(app.handle().clone(), telemetry);

            tauri::async_runtime::spawn(async move {
                if let Err(e) = api::serve(engine).await {
                    if e.kind() == std::io::ErrorKind::AddrInUse {
                        tracing::error!(
                            "Port 8080 already in use — another Gabriel instance may be running. \
                             Close the existing gabriel.exe (or run `pnpm run kill:stale`) and retry. \
                             Underlying error: {e}"
                        );
                    } else {
                        tracing::error!("REST server terminated: {e}");
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc::commands::get_engine_mode,
            ipc::commands::set_engine_mode,
            ipc::commands::get_engine_settings,
            ipc::commands::set_engine_watermarks,
            ipc::commands::get_pager_status,
            ipc::commands::get_profile,
            ipc::commands::set_profile,
            ipc::commands::get_notifications,
            ipc::commands::mark_notification_read,
            ipc::commands::list_models,
            ipc::commands::search_huggingface_models,
            ipc::commands::download_huggingface_model,
            ipc::commands::cancel_hf_download,
            ipc::commands::get_disk_usage,
            ipc::commands::register_local_model,
            ipc::commands::load_model,
            ipc::commands::unload_model,
            ipc::commands::offload_model,
            ipc::commands::get_telemetry,
            ipc::commands::list_loaded_models,
            ipc::commands::get_hardware_specs,
            ipc::commands::get_engine_status,
            ipc::commands::restart_engine,
            ipc::commands::set_bandwidth_ceiling,
            ipc::commands::set_max_loaded_models,
            ipc::commands::set_auto_load_on_request,
            ipc::commands::set_idle_offload_timeout,
            ipc::commands::verify_engine_binary,
            ipc::commands::reset_settings,
            ipc::commands::clear_model_caches,
            ipc::commands::submit_chat,
            ipc::commands::submit_image,
            ipc::commands::submit_speech,
            ipc::commands::read_attachment_preview,
            ipc::commands::get_models_dir,
            ipc::commands::set_models_dir,
            ipc::commands::get_voice_presets,
            ipc::commands::submit_transcription,
            ipc::commands::set_app_title,
            ipc::commands::set_startup_route,
        ])
        .plugin(tauri_plugin_dialog::init())
        // Enforce single-instance: WebView2 uses a fixed shared user-data
        // folder, so two gabriel.exe processes cannot coexist — the second
        // fails with "failed to create webview ... resource is in use" and
        // cannot bind the REST port (os error 10048). When a second instance
        // is launched, this plugin hands its args to the running instance and
        // exits the newcomer before any webview/port is created; we focus the
        // existing main window instead of spawning a duplicate.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}