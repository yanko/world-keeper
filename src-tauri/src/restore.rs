//! Restoring a previously-created backup (SPEC.md §11 "World Library", §15). The safe
//! envelope minus Upload: the source world is already sitting on the remote host in
//! `<worldsPath>/.backups/<slot>.<timestamp>/`, so there's nothing to transfer in
//! first. Otherwise the same safety posture as push: back up whatever's currently live
//! *before* overwriting it, so a restore-gone-wrong can itself be rolled back.
//!
//! Reuses push.rs's `back_up`/`rollback`/`capture_game_id`/`wait_for_udp_listening` —
//! genuinely identical operations ("back up the live world," "restart and hope," "poll
//! for the game id"), not just superficially similar.

use anyhow::{bail, Context};
use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::audit::AuditDb;
use crate::envelope::{start_and_wait, stop_and_wait};
use crate::push;
use crate::ssh::shq;
use crate::target::{RunState, ServerTarget};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RestoreState {
    CheckingService,
    Stopping,
    BackingUp,
    Copying,
    FixingPerms,
    Starting,
    CapturingGameId,
    Verifying,
    Done,
    RollingBack,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreProgress {
    pub state: RestoreState,
    pub detail: Option<String>,
    pub ok: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreResult {
    pub slot: u32,
    pub game_id: Option<String>,
    /// The safety backup made of what was live *before* this restore — a restore can
    /// itself be undone by restoring this one.
    pub backup_path: Option<String>,
}

pub async fn run_restore(
    app: &AppHandle,
    audit: &AuditDb,
    session_id: i64,
    target: &dyn ServerTarget,
    source_backup_path: &str,
    slot: u32,
) -> anyhow::Result<RestoreResult> {
    audit
        .log_event(
            session_id,
            "restore_begin",
            None,
            Some(&serde_json::json!({ "slot": slot, "source": source_backup_path })),
            None,
        )
        .await
        .ok();

    let worlds_path = target.worlds_path().to_string();

    // Defense in depth: the World Library's backup list is scoped to the connected
    // host, but a stale list or direct API misuse could still hand us a path that
    // belongs to an entirely different server — refuse before ever stopping anything.
    if !source_backup_path.starts_with(&worlds_path) {
        bail!(
            "backup path {source_backup_path} doesn't belong to this server's worlds \
             path ({worlds_path}) — refusing to restore across targets"
        );
    }

    emit(app, audit, session_id, RestoreState::CheckingService, None, None).await;
    let _ = target.status().await; // recorded for parity with push; not branched on

    let mut safety_backup_path: Option<String> = None;
    let result = do_restore(
        app,
        audit,
        session_id,
        target,
        source_backup_path,
        &worlds_path,
        slot,
        &mut safety_backup_path,
    )
    .await;

    match result {
        Ok(game_id) => {
            emit(app, audit, session_id, RestoreState::Done, None, Some(true)).await;
            audit
                .log_event(
                    session_id,
                    "restore_done",
                    None,
                    Some(&serde_json::json!({ "slot": slot, "gameId": &game_id })),
                    Some(true),
                )
                .await
                .ok();
            Ok(RestoreResult {
                slot,
                game_id,
                backup_path: safety_backup_path,
            })
        }
        Err(e) => {
            let msg = format!("{e:#}");
            emit(app, audit, session_id, RestoreState::RollingBack, Some(msg.clone()), None).await;
            let rollback_note = push::rollback(target, &worlds_path, safety_backup_path.as_deref()).await;
            emit(app, audit, session_id, RestoreState::Failed, Some(msg.clone()), Some(false)).await;
            audit
                .log_event(
                    session_id,
                    "error",
                    None,
                    Some(&serde_json::json!({
                        "slot": slot,
                        "reason": &msg,
                        "rollback": &rollback_note,
                    })),
                    Some(false),
                )
                .await
                .ok();
            audit
                .log_error(
                    session_id,
                    "restore",
                    &msg,
                    Some(&serde_json::json!({ "slot": slot, "rollback": &rollback_note })),
                )
                .await
                .ok();
            Err(e)
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn do_restore(
    app: &AppHandle,
    audit: &AuditDb,
    session_id: i64,
    target: &dyn ServerTarget,
    source_backup_path: &str,
    worlds_path: &str,
    slot: u32,
    safety_backup_path: &mut Option<String>,
) -> anyhow::Result<Option<String>> {
    emit(app, audit, session_id, RestoreState::Stopping, None, None).await;
    stop_and_wait(target).await.context("stopping service")?;

    emit(app, audit, session_id, RestoreState::BackingUp, None, None).await;
    let backup = push::back_up(target, worlds_path, slot)
        .await
        .context("backing up current world")?;
    *safety_backup_path = backup.clone();
    if let Some(bp) = &backup {
        let bytes = target
            .stat(bp, "*")
            .await
            .map(|entries| entries.iter().map(|e| e.size).sum::<u64>())
            .unwrap_or(0);
        audit
            .log_backup(
                session_id,
                slot as i64,
                bp,
                bytes as i64,
                None,
                Some("pre-restore safety backup"),
            )
            .await
            .ok();
    }

    emit(app, audit, session_id, RestoreState::Copying, None, None).await;
    copy_backup_into_place(target, source_backup_path, worlds_path)
        .await
        .context("copying backup into place")?;

    emit(app, audit, session_id, RestoreState::FixingPerms, None, None).await;
    target.fix_perms(worlds_path).await.context("fixing permissions")?;

    emit(app, audit, session_id, RestoreState::Starting, None, None).await;
    start_and_wait(target).await.context("starting service")?;

    emit(app, audit, session_id, RestoreState::CapturingGameId, None, None).await;
    let game_id = push::capture_game_id(target).await;

    emit(app, audit, session_id, RestoreState::Verifying, None, None).await;
    let udp_ok = push::wait_for_udp_listening(target).await;
    let service_ok = matches!(target.status().await.unwrap_or(RunState::Unknown), RunState::Running);
    if !service_ok || !udp_ok {
        bail!("verification failed (service running: {service_ok}, udp listening: {udp_ok})");
    }

    Ok(game_id)
}

/// Copies (not moves — the backup dir stays intact) every file in `source_backup_path`
/// into `worlds_path`.
async fn copy_backup_into_place(
    target: &dyn ServerTarget,
    source_backup_path: &str,
    worlds_path: &str,
) -> anyhow::Result<()> {
    let entries = target
        .stat(source_backup_path, "*")
        .await
        .context("listing backup contents")?;
    if entries.is_empty() {
        bail!("backup directory is empty or missing: {source_backup_path}");
    }
    for entry in &entries {
        let out = target
            .exec(&format!("cp -a {} {}/", shq(&entry.path), shq(worlds_path)))
            .await?;
        if !out.success() {
            bail!("copying {} failed: {}", entry.path, out.stderr.trim());
        }
    }
    Ok(())
}

async fn emit(
    app: &AppHandle,
    audit: &AuditDb,
    session_id: i64,
    state: RestoreState,
    detail: Option<String>,
    ok: Option<bool>,
) {
    let payload = RestoreProgress {
        state,
        detail: detail.clone(),
        ok,
    };
    let _ = app.emit("restore-progress", &payload);
    let detail_json = detail.map(|d| serde_json::json!({ "message": d }));
    let _ = audit
        .log_event(session_id, "state_enter", Some(&format!("{state:?}")), detail_json.as_ref(), ok)
        .await;
}
