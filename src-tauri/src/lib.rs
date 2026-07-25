mod audit;
mod commands;
mod config;
mod secrets;
mod state;

// Public so `examples/ssh_test.rs` (a manual, no-UI connectivity check the user runs
// themselves against the real server) can reach them without duplicating the transport.
pub mod discovery;
pub mod ssh;
pub mod target;

use tauri::Manager;
use tokio::sync::Mutex;

// Kept from the scaffold so `pnpm tauri dev`'s default template button still works
// until the real UI replaces it.
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_sql::Builder::new().build())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let app_handle = app.handle().clone();
            let app_state = tauri::async_runtime::block_on(async move {
                let audit = audit::AuditDb::init(&app_handle)
                    .await
                    .expect("failed to initialize audit database");
                let session_id = audit
                    .start_session(env!("CARGO_PKG_VERSION"))
                    .await
                    .expect("failed to start audit session");
                audit
                    .log_event(session_id, "app_start", None, None, Some(true))
                    .await
                    .expect("failed to log app_start");

                state::AppState {
                    audit,
                    session_id,
                    connection: Mutex::new(None),
                }
            });
            app.manage(app_state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            commands::get_config,
            commands::upsert_profile,
            commands::delete_profile,
            commands::set_active_profile,
            commands::save_password,
            commands::has_saved_password,
            commands::delete_saved_password,
            commands::connect,
            commands::disconnect,
            commands::run_discovery,
            commands::confirm_discovery,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
