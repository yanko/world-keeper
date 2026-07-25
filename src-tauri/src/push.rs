//! The safe push envelope (SPEC.md §7): Upload → Stop → Backup → Copy → Fix perms →
//! Start → Capture game id → Verify → Done, with rollback on any failure from Stopping
//! onward. Written entirely against `ServerTarget` — no systemd/docker/process
//! branching here; that all lives in `ssh.rs`.
//!
//! Every state transition is emitted as a `push-progress` event (for the frontend
//! stepper) and written to the audit log.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{bail, Context};
use chrono::Utc;
use serde::Serialize;
use sha2::{Digest, Sha256};
use tauri::{AppHandle, Emitter};

use crate::audit::AuditDb;
use crate::envelope::{start_and_wait, stop_and_wait, POLL_INTERVAL};
use crate::localworld::LocalWorldCandidate;
use crate::ssh::shq;
use crate::stats::udp_listening;
use crate::target::{RunState, ServerTarget, ServiceAction};

const GAME_ID_TIMEOUT: Duration = Duration::from_secs(45);
const VERIFY_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PushState {
    Uploading,
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
pub struct PushProgress {
    pub state: PushState,
    pub detail: Option<String>,
    pub ok: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PushResult {
    pub slot: u32,
    pub game_id: Option<String>,
    pub bytes_uploaded: u64,
    pub backup_path: Option<String>,
    pub udp_listening: bool,
}

/// Everything the failure path needs to know, threaded through as the swap progresses
/// so a bail partway through still leaves `rollback` with accurate state.
struct SwapProgress {
    backup_path: Option<String>,
}

pub async fn run_push(
    app: &AppHandle,
    audit: &AuditDb,
    session_id: i64,
    target: &dyn ServerTarget,
    candidate: &LocalWorldCandidate,
) -> anyhow::Result<PushResult> {
    audit
        .log_event(
            session_id,
            "push_begin",
            None,
            Some(&serde_json::json!({ "slot": candidate.slot })),
            None,
        )
        .await
        .ok();

    let worlds_path = target.worlds_path().to_string();
    let staging_dir = format!("{worlds_path}/.staging");

    emit(app, audit, session_id, PushState::Uploading, None, None).await;
    let bytes_uploaded = upload_staged(target, &staging_dir, candidate)
        .await
        .context("uploading world to staging")?;

    // Recorded for the audit trail; not currently branched on. From here on, a failure
    // triggers rollback — SPEC: "If any push step after BackingUp fails" plus "never
    // leave the service in unknown state silently" for Stopping/Starting specifically,
    // so in practice we treat every failure from this point as rollback-worthy.
    emit(app, audit, session_id, PushState::CheckingService, None, None).await;
    let prior_state = target.status().await.unwrap_or(RunState::Unknown);

    let mut progress = SwapProgress { backup_path: None };
    let swap = do_swap(
        app,
        audit,
        session_id,
        target,
        candidate,
        &worlds_path,
        &staging_dir,
        &mut progress,
    )
    .await;

    match swap {
        Ok((game_id, udp_ok)) => {
            emit(app, audit, session_id, PushState::Done, None, Some(true)).await;
            audit
                .log_event(
                    session_id,
                    "push_done",
                    None,
                    Some(&serde_json::json!({
                        "slot": candidate.slot,
                        "gameId": &game_id,
                        "bytesUploaded": bytes_uploaded,
                        "backupPath": &progress.backup_path,
                        "priorState": format!("{prior_state:?}"),
                    })),
                    Some(true),
                )
                .await
                .ok();
            Ok(PushResult {
                slot: candidate.slot,
                game_id,
                bytes_uploaded,
                backup_path: progress.backup_path,
                udp_listening: udp_ok,
            })
        }
        Err(e) => {
            let msg = format!("{e:#}");
            emit(
                app,
                audit,
                session_id,
                PushState::RollingBack,
                Some(msg.clone()),
                None,
            )
            .await;
            let rollback_note = rollback(target, &worlds_path, progress.backup_path.as_deref()).await;
            emit(
                app,
                audit,
                session_id,
                PushState::Failed,
                Some(msg.clone()),
                Some(false),
            )
            .await;
            audit
                .log_event(
                    session_id,
                    "error",
                    None,
                    Some(&serde_json::json!({
                        "slot": candidate.slot,
                        "reason": &msg,
                        "rollback": &rollback_note,
                    })),
                    Some(false),
                )
                .await
                .ok();
            Err(e)
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn do_swap(
    app: &AppHandle,
    audit: &AuditDb,
    session_id: i64,
    target: &dyn ServerTarget,
    candidate: &LocalWorldCandidate,
    worlds_path: &str,
    staging_dir: &str,
    progress: &mut SwapProgress,
) -> anyhow::Result<(Option<String>, bool)> {
    emit(app, audit, session_id, PushState::Stopping, None, None).await;
    stop_and_wait(target).await.context("stopping service")?;

    emit(app, audit, session_id, PushState::BackingUp, None, None).await;
    let backup_path = back_up(target, worlds_path, candidate.slot)
        .await
        .context("backing up current world")?;
    progress.backup_path = backup_path.clone();
    if let Some(bp) = &backup_path {
        let bytes = target
            .stat(bp, "*")
            .await
            .map(|entries| entries.iter().map(|e| e.size).sum::<u64>())
            .unwrap_or(0);
        audit
            .log_backup(session_id, candidate.slot as i64, bp, bytes as i64, None, None)
            .await
            .ok();
    }

    emit(app, audit, session_id, PushState::Copying, None, None).await;
    move_staged_into_place(target, staging_dir, worlds_path, candidate)
        .await
        .context("moving staged files into place")?;

    emit(app, audit, session_id, PushState::FixingPerms, None, None).await;
    target
        .fix_perms(worlds_path)
        .await
        .context("fixing permissions")?;

    emit(app, audit, session_id, PushState::Starting, None, None).await;
    start_and_wait(target).await.context("starting service")?;

    emit(app, audit, session_id, PushState::CapturingGameId, None, None).await;
    let game_id = capture_game_id(target).await;

    emit(app, audit, session_id, PushState::Verifying, None, None).await;
    let udp_ok = wait_for_udp_listening(target).await;
    let service_ok = matches!(target.status().await.unwrap_or(RunState::Unknown), RunState::Running);
    if !service_ok || !udp_ok {
        bail!("verification failed (service running: {service_ok}, udp listening: {udp_ok})");
    }

    Ok((game_id, udp_ok))
}

async fn emit(
    app: &AppHandle,
    audit: &AuditDb,
    session_id: i64,
    state: PushState,
    detail: Option<String>,
    ok: Option<bool>,
) {
    let payload = PushProgress {
        state,
        detail: detail.clone(),
        ok,
    };
    let _ = app.emit("push-progress", &payload);
    let detail_json = detail.map(|d| serde_json::json!({ "message": d }));
    let _ = audit
        .log_event(session_id, "state_enter", Some(&format!("{state:?}")), detail_json.as_ref(), ok)
        .await;
}

async fn upload_staged(
    target: &dyn ServerTarget,
    staging_dir: &str,
    candidate: &LocalWorldCandidate,
) -> anyhow::Result<u64> {
    let mkdir = target.exec(&format!("mkdir -p {}", shq(staging_dir))).await?;
    if !mkdir.success() {
        bail!("mkdir -p {staging_dir} failed: {}", mkdir.stderr.trim());
    }

    let mut total = 0u64;

    let remote_main = format!("{staging_dir}/{}.world.gzip", candidate.slot);
    total += push_and_verify(target, Path::new(&candidate.main_path), &remote_main)
        .await
        .with_context(|| format!("uploading {}", candidate.main_path))?;

    if let Some(local_pb) = &candidate.pugbackup_path {
        let remote_pb = format!("{staging_dir}/{}.world.gzip.pugbackup", candidate.slot);
        total += push_and_verify(target, Path::new(local_pb), &remote_pb)
            .await
            .with_context(|| format!("uploading {local_pb}"))?;
    }

    Ok(total)
}

/// Uploads, then compares a local SHA-256 against a remote `sha256sum` — SPEC's
/// "verify sizes/checksums" for the Uploading step.
async fn push_and_verify(target: &dyn ServerTarget, local: &Path, remote: &str) -> anyhow::Result<u64> {
    let local_bytes = tokio::fs::read(local)
        .await
        .with_context(|| format!("reading {}", local.display()))?;
    let local_hash = to_hex(&Sha256::digest(&local_bytes));
    drop(local_bytes);

    let bytes = target.push_file(local, remote).await?;

    let out = target.exec(&format!("sha256sum {}", shq(remote))).await?;
    let remote_hash = out.stdout.split_whitespace().next().unwrap_or("");
    if remote_hash.is_empty() {
        bail!("could not verify checksum of {remote} (sha256sum produced no output)");
    }
    if remote_hash != local_hash {
        bail!("checksum mismatch for {remote}: local {local_hash} != remote {remote_hash}");
    }
    Ok(bytes)
}

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Copies the live `N.world.gzip*` set (if any exist yet) to a dated backup dir before
/// anything is overwritten. Returns `None` only when there's genuinely nothing to back
/// up (first-ever push to this slot) — never as a way of skipping a failure.
async fn back_up(target: &dyn ServerTarget, worlds_path: &str, slot: u32) -> anyhow::Result<Option<String>> {
    let existing = target
        .stat(worlds_path, &format!("{slot}.world.gzip*"))
        .await
        .context("listing existing world files")?;
    if existing.is_empty() {
        return Ok(None);
    }

    let timestamp = Utc::now().format("%Y-%m-%d_%H%M%S");
    let backup_dir = format!("{worlds_path}/.backups/{slot}.{timestamp}");
    let mkdir = target.exec(&format!("mkdir -p {}", shq(&backup_dir))).await?;
    if !mkdir.success() {
        bail!("mkdir -p {backup_dir} failed: {}", mkdir.stderr.trim());
    }
    for entry in &existing {
        let out = target
            .exec(&format!("cp -a {} {}/", shq(&entry.path), shq(&backup_dir)))
            .await?;
        if !out.success() {
            bail!("backing up {} failed: {}", entry.path, out.stderr.trim());
        }
    }
    Ok(Some(backup_dir))
}

/// Moves the staged upload into place. If the incoming world has no `.pugbackup`, the
/// slot's existing one (already preserved in the dated backup) is left untouched rather
/// than guessed at — Core Keeper will regenerate it on its own next save cycle.
async fn move_staged_into_place(
    target: &dyn ServerTarget,
    staging_dir: &str,
    worlds_path: &str,
    candidate: &LocalWorldCandidate,
) -> anyhow::Result<()> {
    let slot = candidate.slot;

    let staged_main = format!("{staging_dir}/{slot}.world.gzip");
    let dest_main = format!("{worlds_path}/{slot}.world.gzip");
    let out = target
        .exec(&format!("mv {} {}", shq(&staged_main), shq(&dest_main)))
        .await?;
    if !out.success() {
        bail!("moving {staged_main} into place failed: {}", out.stderr.trim());
    }

    if candidate.pugbackup_path.is_some() {
        let staged_pb = format!("{staging_dir}/{slot}.world.gzip.pugbackup");
        let dest_pb = format!("{worlds_path}/{slot}.world.gzip.pugbackup");
        let out = target
            .exec(&format!("mv {} {}", shq(&staged_pb), shq(&dest_pb)))
            .await?;
        if !out.success() {
            bail!("moving {staged_pb} into place failed: {}", out.stderr.trim());
        }
    }
    Ok(())
}

/// Polls the log tail for Core Keeper's `GameID: <id>` line (format confirmed against
/// the reference LXC), taking the *last* match so a stale line from a previous run
/// (still present in the tail window) never wins over the fresh one.
async fn capture_game_id(target: &dyn ServerTarget) -> Option<String> {
    let deadline = Instant::now() + GAME_ID_TIMEOUT;
    loop {
        if let Ok(text) = target.logs_tail(200).await {
            if let Some(id) = find_game_id(&text) {
                return Some(id);
            }
        }
        if Instant::now() >= deadline {
            return None;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

fn find_game_id(text: &str) -> Option<String> {
    text.lines().rev().find_map(|line| {
        let idx = line.find("GameID:")?;
        let id = line[idx + "GameID:".len()..].trim();
        (!id.is_empty()).then(|| id.to_string())
    })
}

async fn wait_for_udp_listening(target: &dyn ServerTarget) -> bool {
    let deadline = Instant::now() + VERIFY_TIMEOUT;
    loop {
        if !udp_listening(target).await.is_empty() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}

/// Best-effort recovery: restore the backup (if one was made), fix perms, and always
/// attempt to get the service running again — SPEC: "never leave service in unknown
/// state silently." Returns a human-readable note for the audit log, never fails itself.
async fn rollback(target: &dyn ServerTarget, worlds_path: &str, backup_path: Option<&str>) -> String {
    let mut notes = Vec::new();

    if let Some(backup_path) = backup_path {
        match target
            .exec(&format!("cp -a {}/. {}/", shq(backup_path), shq(worlds_path)))
            .await
        {
            Ok(out) if out.success() => notes.push("restored backup".to_string()),
            Ok(out) => notes.push(format!("restore failed: {}", out.stderr.trim())),
            Err(e) => notes.push(format!("restore error: {e}")),
        }
        if let Err(e) = target.fix_perms(worlds_path).await {
            notes.push(format!("fix_perms after restore failed: {e}"));
        }
    } else {
        notes.push("no backup existed to restore".to_string());
    }

    match target.service_control(ServiceAction::Start).await {
        Ok(out) if out.success() => notes.push("restart issued".to_string()),
        Ok(out) => notes.push(format!("restart command failed: {}", out.stderr.trim())),
        Err(e) => notes.push(format!("restart error: {e}")),
    }

    notes.join("; ")
}
