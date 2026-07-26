mod audit;
mod command_reference;
mod commands;
mod config;
mod envelope;
mod keygen;
mod localworld;
mod pull;
mod push;
mod restore;
mod secrets;
mod state;
mod stats;
mod terminal;

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
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_clipboard_manager::init())
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
                    terminal: Mutex::new(None),
                    operation_lock: Mutex::new(()),
                }
            });
            app.manage(app_state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            greet,
            commands::get_about_info,
            commands::get_config,
            commands::upsert_profile,
            commands::delete_profile,
            commands::set_active_profile,
            commands::save_password,
            commands::has_saved_password,
            commands::delete_saved_password,
            commands::connect,
            commands::disconnect,
            commands::setup_key_auth,
            commands::run_discovery,
            commands::confirm_discovery,
            commands::get_stats,
            commands::get_log_tail,
            commands::resolve_local_world,
            commands::push_world,
            commands::list_remote_worlds,
            commands::pull_world,
            commands::package_pulled_world,
            commands::query_audit_events,
            commands::list_audit_event_kinds,
            commands::list_audit_sessions,
            commands::query_audit_commands,
            commands::query_audit_errors,
            commands::list_audit_error_contexts,
            commands::terminal_open,
            commands::terminal_write,
            commands::terminal_resize,
            commands::terminal_close,
            commands::clear_audit_log,
            commands::export_audit,
            commands::list_backups,
            commands::restore_backup,
            commands::get_command_reference,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
