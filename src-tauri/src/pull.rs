//! The pull flow (SPEC.md §7 "Pull states"): CheckingService → Stopping → Downloading
//! → Starting → (locally) Packaging → Done. Deliberately *shorter* than push's
//! envelope — pulling never touches the remote world files, so there's no backup, perm
//! fix, or game-id recapture to do; the service is restarted immediately after the
//! download to minimize downtime, and local packaging (save-as folder/zip) happens
//! afterward, off the critical path.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};

use crate::audit::AuditDb;
use crate::envelope::{start_and_wait, stop_and_wait};
use crate::localworld::parse_world_filename;
use crate::target::ServerTarget;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PullState {
    CheckingService,
    Stopping,
    Downloading,
    Starting,
    Done,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PullProgress {
    pub state: PullState,
    pub detail: Option<String>,
    pub ok: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteWorldInfo {
    pub slot: u32,
    pub size: u64,
    pub modified: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PulledWorld {
    pub slot: u32,
    pub temp_dir: String,
    pub main_path: String,
    pub pugbackup_path: Option<String>,
    pub bytes: u64,
}

/// What's available to pull, for the picker — only the main file per slot; a bare
/// `.pugbackup` with no live world alongside it isn't something to offer as "world N".
pub async fn list_remote_worlds(target: &dyn ServerTarget) -> anyhow::Result<Vec<RemoteWorldInfo>> {
    let entries = target
        .stat(target.worlds_path(), "*.world.gzip")
        .await
        .context("listing worlds")?;
    let mut worlds: Vec<RemoteWorldInfo> = entries
        .into_iter()
        .filter_map(|e| {
            let name = Path::new(&e.path).file_name()?.to_str()?;
            let (slot, is_pugbackup) = parse_world_filename(name)?;
            (!is_pugbackup).then_some(RemoteWorldInfo {
                slot,
                size: e.size,
                modified: e.modified,
            })
        })
        .collect();
    worlds.sort_by_key(|w| w.slot);
    Ok(worlds)
}

pub async fn run_pull(
    app: &AppHandle,
    audit: &AuditDb,
    session_id: i64,
    target: &dyn ServerTarget,
    slot: u32,
) -> anyhow::Result<PulledWorld> {
    audit
        .log_event(session_id, "pull_begin", None, Some(&serde_json::json!({ "slot": slot })), None)
        .await
        .ok();

    emit(app, audit, session_id, PullState::CheckingService, None, None).await;
    let _ = target.status().await; // recorded for parity with push; not branched on

    emit(app, audit, session_id, PullState::Stopping, None, None).await;
    if let Err(e) = stop_and_wait(target).await {
        return fail(app, audit, session_id, slot, e.context("stopping service")).await;
    }

    emit(app, audit, session_id, PullState::Downloading, None, None).await;
    let downloaded = download_world(target, slot).await;

    // Restart regardless of download outcome: SPEC's rationale is minimum downtime, and
    // leaving the service stopped after a failed download would only compound the
    // problem — never leave it in an unknown state silently.
    emit(app, audit, session_id, PullState::Starting, None, None).await;
    let started = start_and_wait(target).await;

    let downloaded = match downloaded {
        Ok(d) => d,
        Err(e) => {
            let e = match started {
                Err(se) => e.context(format!("(and the restart afterward also failed: {se})")),
                Ok(()) => e,
            };
            return fail(app, audit, session_id, slot, e.context("downloading world")).await;
        }
    };
    if let Err(e) = started {
        return fail(app, audit, session_id, slot, e.context("starting service")).await;
    }

    emit(app, audit, session_id, PullState::Done, None, Some(true)).await;
    audit
        .log_event(
            session_id,
            "pull_done",
            None,
            Some(&serde_json::json!({ "slot": slot, "bytes": downloaded.bytes })),
            Some(true),
        )
        .await
        .ok();

    Ok(downloaded)
}

async fn fail(
    app: &AppHandle,
    audit: &AuditDb,
    session_id: i64,
    slot: u32,
    e: anyhow::Error,
) -> anyhow::Result<PulledWorld> {
    let msg = format!("{e:#}");
    emit(app, audit, session_id, PullState::Failed, Some(msg.clone()), Some(false)).await;
    audit
        .log_event(
            session_id,
            "error",
            None,
            Some(&serde_json::json!({ "slot": slot, "reason": &msg })),
            Some(false),
        )
        .await
        .ok();
    audit
        .log_error(session_id, "pull", &msg, Some(&serde_json::json!({ "slot": slot })))
        .await
        .ok();
    Err(e)
}

async fn emit(
    app: &AppHandle,
    audit: &AuditDb,
    session_id: i64,
    state: PullState,
    detail: Option<String>,
    ok: Option<bool>,
) {
    let payload = PullProgress {
        state,
        detail: detail.clone(),
        ok,
    };
    let _ = app.emit("pull-progress", &payload);
    let detail_json = detail.map(|d| serde_json::json!({ "message": d }));
    let _ = audit
        .log_event(session_id, "state_enter", Some(&format!("{state:?}")), detail_json.as_ref(), ok)
        .await;
}

async fn download_world(target: &dyn ServerTarget, slot: u32) -> anyhow::Result<PulledWorld> {
    let worlds_path = target.worlds_path();
    let entries = target
        .stat(worlds_path, &format!("{slot}.world.gzip*"))
        .await
        .context("listing world files to download")?;
    if entries.is_empty() {
        bail!("no world files found for slot {slot}");
    }

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp_dir = std::env::temp_dir().join(format!("ckm-pull-{nanos}"));
    tokio::fs::create_dir_all(&temp_dir)
        .await
        .with_context(|| format!("creating temp dir {}", temp_dir.display()))?;

    let mut main_path = None;
    let mut pugbackup_path = None;
    let mut total = 0u64;

    for entry in &entries {
        let remote_name = Path::new(&entry.path)
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| anyhow::anyhow!("unexpected remote path {}", entry.path))?;
        let local_path = temp_dir.join(remote_name);
        let bytes = target
            .pull_file(&entry.path, &local_path)
            .await
            .with_context(|| format!("downloading {}", entry.path))?;
        total += bytes;
        if remote_name.ends_with(".pugbackup") {
            pugbackup_path = Some(local_path.to_string_lossy().to_string());
        } else {
            main_path = Some(local_path.to_string_lossy().to_string());
        }
    }

    let main_path = main_path
        .ok_or_else(|| anyhow::anyhow!("no main .world.gzip found for slot {slot} (only a .pugbackup?)"))?;

    Ok(PulledWorld {
        slot,
        temp_dir: temp_dir.to_string_lossy().to_string(),
        main_path,
        pugbackup_path,
        bytes: total,
    })
}

/// SPEC's "native save-as (folder or zip) → package", run after the server's already
/// safely back up. Cleans up the downloaded temp files afterward either way.
pub async fn package_pulled(pulled: &PulledWorld, destination: &str, as_zip: bool) -> anyhow::Result<String> {
    let result = if as_zip {
        package_as_zip(pulled, destination).await
    } else {
        package_as_folder(pulled, destination).await
    };

    tokio::fs::remove_dir_all(&pulled.temp_dir).await.ok();
    result
}

async fn package_as_folder(pulled: &PulledWorld, destination: &str) -> anyhow::Result<String> {
    let dest_dir = PathBuf::from(destination);
    tokio::fs::create_dir_all(&dest_dir)
        .await
        .with_context(|| format!("creating {}", dest_dir.display()))?;

    let main_name = format!("{}.world.gzip", pulled.slot);
    tokio::fs::copy(&pulled.main_path, dest_dir.join(&main_name))
        .await
        .with_context(|| format!("copying to {}", dest_dir.display()))?;

    if let Some(pb) = &pulled.pugbackup_path {
        let pb_name = format!("{}.world.gzip.pugbackup", pulled.slot);
        tokio::fs::copy(pb, dest_dir.join(&pb_name))
            .await
            .with_context(|| format!("copying {pb_name} to {}", dest_dir.display()))?;
    }

    Ok(dest_dir.to_string_lossy().to_string())
}

async fn package_as_zip(pulled: &PulledWorld, destination: &str) -> anyhow::Result<String> {
    let destination = destination.to_string();
    let slot = pulled.slot;
    let main_path = pulled.main_path.clone();
    let pugbackup_path = pulled.pugbackup_path.clone();

    tokio::task::spawn_blocking(move || -> anyhow::Result<String> {
        use std::io::Write;

        let file = std::fs::File::create(&destination)
            .with_context(|| format!("creating {destination}"))?;
        let mut writer = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();

        writer.start_file(format!("{slot}.world.gzip"), options)?;
        writer.write_all(&std::fs::read(&main_path)?)?;

        if let Some(pb) = &pugbackup_path {
            writer.start_file(format!("{slot}.world.gzip.pugbackup"), options)?;
            writer.write_all(&std::fs::read(pb)?)?;
        }

        writer.finish()?;
        Ok(destination)
    })
    .await?
}
