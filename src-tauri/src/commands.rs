//! Tauri commands — the thin IPC surface the (future) React frontend calls. All real
//! logic lives in `config`/`audit`/`secrets`/`ssh`/`discovery`; these just wire them to
//! `invoke`.

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::config::{self, AppConfig, Discovered, Profile};
use crate::discovery::{self, DiscoveryResult};
use crate::secrets;
use crate::ssh::{SshSession, SshTarget};
use crate::state::{AppState, Connection};
use crate::stats::{self, ServerStats};

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

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
    password: String,
) -> Result<ConnectResult, String> {
    let cfg = config::load(&app).map_err(err)?;
    let profile = cfg
        .profiles
        .iter()
        .find(|p| p.id == profile_id)
        .cloned()
        .ok_or_else(|| format!("no such profile: {profile_id}"))?;

    let session =
        SshSession::connect_password(&profile.host, profile.port, &profile.user, &password)
            .await
            .map_err(err)?;

    state
        .audit
        .record_connect(state.session_id, &profile.id, &profile.host, &profile.user, "password")
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
    if let Some(conn) = state.connection.lock().await.take() {
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

#[tauri::command]
pub async fn run_discovery(state: State<'_, AppState>) -> Result<DiscoveryResult, String> {
    let guard = state.connection.lock().await;
    let conn = guard.as_ref().ok_or("not connected")?;
    let result = discovery::probe(conn.session()).await.map_err(err)?;
    let profile_id = conn.profile_id().to_string();
    drop(guard);

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

#[tauri::command]
pub async fn get_stats(state: State<'_, AppState>) -> Result<ServerStats, String> {
    let guard = state.connection.lock().await;
    match guard.as_ref() {
        Some(Connection::Target { target, .. }) => stats::collect(target).await.map_err(err),
        Some(Connection::Raw { .. }) => Err("discovery not confirmed yet".to_string()),
        None => Err("not connected".to_string()),
    }
}
