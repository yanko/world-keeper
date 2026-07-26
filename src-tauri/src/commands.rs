//! Tauri commands — the thin IPC surface the (future) React frontend calls. All real
//! logic lives in `config`/`audit`/`secrets`/`ssh`/`discovery`; these just wire them to
//! `invoke`.

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::audit::{
    BackupRow, CommandFilter, CommandRow, ErrorFilter, ErrorRow, EventFilter, EventRow, SessionRow,
};
use crate::config::{self, AppConfig, Discovered, Profile};
use crate::discovery::{self, DiscoveryResult};
use crate::keygen;
use crate::localworld::{self, LocalWorldCandidate};
use crate::pull::{self, PulledWorld, RemoteWorldInfo};
use crate::push::{self, PushResult};
use crate::restore::{self, RestoreResult};
use crate::secrets;
use crate::ssh::{SshSession, SshTarget};
use crate::state::{AppState, Connection};
use crate::stats::{self, ServerStats};
use crate::target::ServerTarget;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Push/pull/restore all stop and restart the same live server, so running two at once
/// isn't just confusing — it's a real corruption risk (two overlapping stop/backup/copy/
/// start sequences racing each other). `operation_lock` (see `state.rs`) is what
/// actually prevents that; this is just the error surfaced when `try_lock` on it fails.
/// The frontend already disables the triggering buttons while one is in flight, so
/// reaching this in practice means a genuine race slipped past that — worth showing as
/// a real (if terse) error, not silently swallowing.
const BUSY: &str = "another operation is already in progress";

// ---- config ----------------------------------------------------------------

#[tauri::command]
pub fn get_config(app: AppHandle) -> Result<AppConfig, String> {
    config::load(&app).map_err(err)
}

#[tauri::command]
pub fn upsert_profile(app: AppHandle, profile: Profile) -> Result<AppConfig, String> {
    config::upsert_profile(&app, profile).map_err(err)
}

#[tauri::command]
pub fn delete_profile(app: AppHandle, profile_id: String) -> Result<AppConfig, String> {
    config::delete_profile(&app, &profile_id).map_err(err)
}

#[tauri::command]
pub fn set_active_profile(app: AppHandle, profile_id: String) -> Result<AppConfig, String> {
    let mut cfg = config::load(&app).map_err(err)?;
    cfg.active_profile_id = Some(profile_id);
    config::save(&app, &cfg).map_err(err)?;
    Ok(cfg)
}

// ---- secrets -----------------------------------------------------------------

#[tauri::command]
pub fn save_password(profile_id: String, password: String) -> Result<(), String> {
    secrets::save_password(&profile_id, &password).map_err(err)
}

#[tauri::command]
pub fn has_saved_password(profile_id: String) -> Result<bool, String> {
    secrets::get_password(&profile_id)
        .map(|p| p.is_some())
        .map_err(err)
}

#[tauri::command]
pub fn delete_saved_password(profile_id: String) -> Result<(), String> {
    secrets::delete_password(&profile_id).map_err(err)
}

// ---- connection / discovery ---------------------------------------------------

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectResult {
    pub profile_id: String,
    /// True when this profile has no cached `discovered` block yet, i.e. the frontend
    /// should call `run_discovery` next and show the confirmation UI.
    pub needs_discovery: bool,
}

#[tauri::command]
pub async fn connect(
    app: AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
    password: Option<String>,
) -> Result<ConnectResult, String> {
    let cfg = config::load(&app).map_err(err)?;
    let profile = cfg
        .profiles
        .iter()
        .find(|p| p.id == profile_id)
        .cloned()
        .ok_or_else(|| format!("no such profile: {profile_id}"))?;

    let (session_result, auth_method) = match profile.auth {
        config::AuthMethod::Key => {
            let key_path = profile
                .key_path
                .as_deref()
                .ok_or("this profile is set to key auth but has no keyPath")?;
            let result = SshSession::connect_key(
                &profile.host,
                profile.port,
                &profile.user,
                std::path::Path::new(key_path),
            )
            .await;
            (result, "key")
        }
        config::AuthMethod::Password => {
            let password = password.ok_or("password required for this profile")?;
            let result =
                SshSession::connect_password(&profile.host, profile.port, &profile.user, &password)
                    .await;
            (result, "password")
        }
    };
    let mut session = match session_result {
        Ok(s) => s,
        Err(e) => {
            let msg = err(e);
            state
                .audit
                .log_error(
                    state.session_id,
                    "connect",
                    &msg,
                    Some(&serde_json::json!({ "host": profile.host, "user": profile.user, "authMethod": auth_method })),
                )
                .await
                .ok();
            return Err(msg);
        }
    };
    session.attach_audit(state.audit.clone(), state.session_id);

    state
        .audit
        .record_connect(state.session_id, &profile.id, &profile.host, &profile.user, auth_method)
        .await
        .map_err(err)?;
    state
        .audit
        .log_event(
            state.session_id,
            "connect",
            None,
            Some(&serde_json::json!({ "host": profile.host, "user": profile.user })),
            Some(true),
        )
        .await
        .map_err(err)?;

    let needs_discovery = profile.discovered.is_none();
    let new_connection = match profile.discovered.as_ref().and_then(|d| d.service_kind.clone()) {
        Some(kind) => Connection::Target {
            profile_id: profile.id.clone(),
            target: SshTarget::new(
                session,
                kind,
                profile
                    .discovered
                    .as_ref()
                    .and_then(|d| d.worlds_path.clone())
                    .unwrap_or_default(),
                profile.discovered.as_ref().and_then(|d| d.steam_user.clone()),
            ),
        },
        None => Connection::Raw {
            profile_id: profile.id.clone(),
            session,
        },
    };

    *state.connection.lock().await = Some(new_connection);

    Ok(ConnectResult {
        profile_id: profile.id,
        needs_discovery,
    })
}

#[tauri::command]
pub async fn disconnect(state: State<'_, AppState>) -> Result<(), String> {
    // Never tear down the connection out from under a running push/pull/restore — this
    // is the one thing `operation_lock` guards for a command that isn't itself one of
    // the three (see `AppState::operation_lock`'s doc comment).
    let _op_guard = state.operation_lock.try_lock().map_err(|_| BUSY.to_string())?;
    if let Some(term) = state.terminal.lock().await.take() {
        term.close();
    }
    let mut guard = state.connection.lock().await;
    if let Some(conn) = guard.take() {
        state
            .audit
            .log_event(
                state.session_id,
                "disconnect",
                None,
                Some(&serde_json::json!({ "profileId": conn.profile_id() })),
                Some(true),
            )
            .await
            .map_err(err)?;
    }
    Ok(())
}

/// SPEC §6's "one-time setup helper": generates a fresh keypair, installs the public
/// key on the currently-connected host via the live session, and switches the profile
/// over to key auth. Requires an existing connection (any auth method) — meant to run
/// right after a first password connect, not as a way to connect in the first place.
#[tauri::command]
pub async fn setup_key_auth(app: AppHandle, state: State<'_, AppState>) -> Result<Profile, String> {
    let profile_id = {
        let guard = state.connection.lock().await;
        guard.as_ref().ok_or("not connected")?.profile_id().to_string()
    };

    let cfg = config::load(&app).map_err(err)?;
    let profile = cfg
        .profiles
        .iter()
        .find(|p| p.id == profile_id)
        .cloned()
        .ok_or_else(|| format!("no such profile: {profile_id}"))?;

    let generated = keygen::generate_for_profile(&app, &profile_id, "ck-world-manager")
        .await
        .map_err(err)?;

    {
        let guard = state.connection.lock().await;
        let conn = guard.as_ref().ok_or("not connected")?;
        conn.session()
            .install_public_key(&generated.public_key_line)
            .await
            .map_err(err)?;
    }

    let mut updated = profile;
    updated.auth = config::AuthMethod::Key;
    updated.key_path = Some(generated.private_key_path.to_string_lossy().to_string());
    updated.save_password = false;
    config::upsert_profile(&app, updated.clone()).map_err(err)?;

    state
        .audit
        .log_event(
            state.session_id,
            "key_auth_setup",
            None,
            Some(&serde_json::json!({ "profileId": profile_id })),
            Some(true),
        )
        .await
        .map_err(err)?;

    Ok(updated)
}

#[tauri::command]
pub async fn run_discovery(state: State<'_, AppState>) -> Result<DiscoveryResult, String> {
    let guard = state.connection.lock().await;
    let conn = guard.as_ref().ok_or("not connected")?;
    let probe_result = discovery::probe(conn.session()).await;
    let profile_id = conn.profile_id().to_string();
    drop(guard);

    let result = match probe_result {
        Ok(r) => r,
        Err(e) => {
            let msg = err(e);
            state
                .audit
                .log_error(state.session_id, "discovery", &msg, Some(&serde_json::json!({ "profileId": profile_id })))
                .await
                .ok();
            return Err(msg);
        }
    };

    state
        .audit
        .log_event(
            state.session_id,
            "discover",
            None,
            Some(&serde_json::json!({ "profileId": profile_id, "result": &result })),
            Some(true),
        )
        .await
        .map_err(err)?;

    Ok(result)
}

/// Persist a (possibly user-edited) discovery result and, if it names a `ServiceKind`,
/// swap the live connection over to a confirmed `SshTarget`. Never called implicitly —
/// SPEC §8 requires findings to be shown for confirmation first.
#[tauri::command]
pub async fn confirm_discovery(
    app: AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
    discovered: Discovered,
) -> Result<AppConfig, String> {
    let cfg = config::set_discovered(&app, &profile_id, discovered.clone()).map_err(err)?;

    let mut slot = state.connection.lock().await;
    if let Some(current) = slot.take() {
        if current.profile_id() == profile_id {
            match discovered.service_kind.clone() {
                Some(kind) => {
                    let session = match current {
                        Connection::Raw { session, .. } => session,
                        Connection::Target { target, .. } => target.into_session(),
                    };
                    *slot = Some(Connection::Target {
                        profile_id: profile_id.clone(),
                        target: SshTarget::new(
                            session,
                            kind,
                            discovered.worlds_path.clone().unwrap_or_default(),
                            discovered.steam_user.clone(),
                        ),
                    });
                }
                None => *slot = Some(current),
            }
        } else {
            *slot = Some(current);
        }
    }
    drop(slot);

    state
        .audit
        .log_event(
            state.session_id,
            "discover_confirmed",
            None,
            Some(&serde_json::json!({ "profileId": profile_id, "discovered": &discovered })),
            Some(true),
        )
        .await
        .map_err(err)?;

    Ok(cfg)
}

// ---- stats -----------------------------------------------------------------

/// Clones the confirmed target out of `state.connection` under a brief lock, then drops
/// the lock immediately. `SshTarget`/`SshSession` are cheap to clone (the underlying
/// handle is an `Arc`, shared, not reopened) — this is what lets stats/log-tail polling,
/// the Terminal, and a long-running push/pull/restore all use the same SSH connection
/// concurrently instead of queuing behind one held lock. Only push/pull/restore
/// themselves contend on the separate `operation_lock` (see `commands.rs`'s `BUSY`).
async fn clone_target(state: &AppState) -> Result<SshTarget, String> {
    let guard = state.connection.lock().await;
    match guard.as_ref() {
        Some(Connection::Target { target, .. }) => Ok(target.clone()),
        Some(Connection::Raw { .. }) => Err("discovery not confirmed yet".to_string()),
        None => Err("not connected".to_string()),
    }
}

#[tauri::command]
pub async fn get_stats(state: State<'_, AppState>) -> Result<ServerStats, String> {
    let target = clone_target(&state).await?;
    stats::collect(&target).await.map_err(err)
}

/// Just the log tail, polled on its own (e.g. by the live log drawer) — deliberately
/// separate from `get_stats` so watching the log doesn't also re-fetch mem/disk/uptime
/// on every poll tick.
#[tauri::command]
pub async fn get_log_tail(state: State<'_, AppState>, lines: u32) -> Result<String, String> {
    let target = clone_target(&state).await?;
    target.logs_tail(lines).await.map_err(err)
}

// ---- push --------------------------------------------------------------------

/// `path` is whatever the frontend's native dialog (folder / .zip / single file)
/// returned. Local-only, no connection required.
#[tauri::command]
pub async fn resolve_local_world(path: String) -> Result<Vec<LocalWorldCandidate>, String> {
    localworld::resolve_local_world(std::path::Path::new(&path))
        .await
        .map_err(err)
}

/// Runs the full safe-swap envelope (SPEC §7) against the currently confirmed target,
/// emitting `push-progress` events as it goes. Stops the live server for the duration.
#[tauri::command]
pub async fn push_world(
    app: AppHandle,
    state: State<'_, AppState>,
    candidate: LocalWorldCandidate,
) -> Result<PushResult, String> {
    let _op_guard = state.operation_lock.try_lock().map_err(|_| BUSY.to_string())?;
    let target = clone_target(&state).await?;
    push::run_push(&app, &state.audit, state.session_id, &target, &candidate)
        .await
        .map_err(err)
}

// ---- pull --------------------------------------------------------------------

#[tauri::command]
pub async fn list_remote_worlds(state: State<'_, AppState>) -> Result<Vec<RemoteWorldInfo>, String> {
    let target = clone_target(&state).await?;
    pull::list_remote_worlds(&target).await.map_err(err)
}

/// Runs SPEC §7's pull envelope (stop → download → restart) against the currently
/// confirmed target, emitting `pull-progress` events. Local packaging is a separate
/// step (`package_pulled_world`) that happens after the server's already back up.
#[tauri::command]
pub async fn pull_world(
    app: AppHandle,
    state: State<'_, AppState>,
    slot: u32,
) -> Result<PulledWorld, String> {
    let _op_guard = state.operation_lock.try_lock().map_err(|_| BUSY.to_string())?;
    let target = clone_target(&state).await?;
    pull::run_pull(&app, &state.audit, state.session_id, &target, slot)
        .await
        .map_err(err)
}

/// Saves a previously pulled world (still sitting in a temp dir) as a folder or a
/// `.zip`, then cleans up the temp files. Purely local — no connection required.
#[tauri::command]
pub async fn package_pulled_world(
    pulled: PulledWorld,
    destination: String,
    as_zip: bool,
) -> Result<String, String> {
    pull::package_pulled(&pulled, &destination, as_zip).await.map_err(err)
}

// ---- audit -------------------------------------------------------------------

#[tauri::command]
pub async fn query_audit_events(
    state: State<'_, AppState>,
    filter: EventFilter,
) -> Result<Vec<EventRow>, String> {
    state.audit.query_events(&filter).await.map_err(err)
}

#[tauri::command]
pub async fn list_audit_event_kinds(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    state.audit.list_event_kinds().await.map_err(err)
}

#[tauri::command]
pub async fn list_audit_sessions(state: State<'_, AppState>) -> Result<Vec<SessionRow>, String> {
    state.audit.list_sessions().await.map_err(err)
}

#[tauri::command]
pub async fn query_audit_commands(
    state: State<'_, AppState>,
    filter: CommandFilter,
) -> Result<Vec<CommandRow>, String> {
    state.audit.query_ssh_commands(&filter).await.map_err(err)
}

#[tauri::command]
pub async fn query_audit_errors(
    state: State<'_, AppState>,
    filter: ErrorFilter,
) -> Result<Vec<ErrorRow>, String> {
    state.audit.query_errors(&filter).await.map_err(err)
}

#[tauri::command]
pub async fn list_audit_error_contexts(state: State<'_, AppState>) -> Result<Vec<String>, String> {
    state.audit.list_error_contexts().await.map_err(err)
}

// ---- terminal ------------------------------------------------------------------

/// Opens a fresh PTY session over the current connection, replacing any previous one.
/// Deliberately manual (no auto-open on connect) — this is a real, unsandboxed shell,
/// not something to spin up without the user asking for it.
#[tauri::command]
pub async fn terminal_open(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let term = {
        let guard = state.connection.lock().await;
        let conn = guard.as_ref().ok_or("not connected")?;
        crate::terminal::TerminalSession::open(app, conn.session())
            .await
            .map_err(err)?
    };
    let mut slot = state.terminal.lock().await;
    if let Some(old) = slot.take() {
        old.close();
    }
    *slot = Some(term);
    Ok(())
}

#[tauri::command]
pub async fn terminal_write(state: State<'_, AppState>, data: String) -> Result<(), String> {
    let guard = state.terminal.lock().await;
    let term = guard.as_ref().ok_or("terminal session not open")?;
    term.write(&data).await.map_err(err)
}

#[tauri::command]
pub async fn terminal_resize(state: State<'_, AppState>, cols: u32, rows: u32) -> Result<(), String> {
    let guard = state.terminal.lock().await;
    let term = guard.as_ref().ok_or("terminal session not open")?;
    term.resize(cols, rows).await.map_err(err)
}

#[tauri::command]
pub async fn terminal_close(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(term) = state.terminal.lock().await.take() {
        term.close();
    }
    Ok(())
}

// ---- audit: destructive/export ops --------------------------------------------

/// Wipes the audit trail. The frontend is expected to have already gotten two rounds
/// of "are you sure" out of the user before ever calling this — nothing here re-asks.
#[tauri::command]
pub async fn clear_audit_log(state: State<'_, AppState>) -> Result<(), String> {
    state.audit.clear_audit_log(state.session_id).await.map_err(err)
}

/// Writes a zip (events.csv + the raw ckmanager.db) to `destination`, which the
/// frontend resolves via a native "save as" dialog first.
#[tauri::command]
pub async fn export_audit(state: State<'_, AppState>, destination: String) -> Result<(), String> {
    state
        .audit
        .export_zip(std::path::Path::new(&destination))
        .await
        .map_err(err)
}

// ---- world library / restore ---------------------------------------------------

#[tauri::command]
pub async fn list_backups(state: State<'_, AppState>) -> Result<Vec<BackupRow>, String> {
    let host = state
        .connection
        .lock()
        .await
        .as_ref()
        .map(|c| c.session().host.clone());
    state.audit.list_backups(host.as_deref()).await.map_err(err)
}

/// Restores a previously-created backup (SPEC §11/§15 World Library): stops the
/// server, backs up whatever's currently live (so this itself can be undone), copies
/// the chosen backup into place, restarts, and verifies. Emits `restore-progress`.
#[tauri::command]
pub async fn restore_backup(
    app: AppHandle,
    state: State<'_, AppState>,
    backup_path: String,
    slot: u32,
) -> Result<RestoreResult, String> {
    let _op_guard = state.operation_lock.try_lock().map_err(|_| BUSY.to_string())?;
    let target = clone_target(&state).await?;
    restore::run_restore(&app, &state.audit, state.session_id, &target, &backup_path, slot)
        .await
        .map_err(err)
}
