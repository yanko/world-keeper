//! Local world detection (SPEC.md §7 `ResolvingLocalWorld`, §9 "World detection is
//! smart"). Given whatever the native file/folder/zip dialog handed back, find every
//! `N.world.gzip` (+ its optional `.pugbackup` sibling) and group them by slot.
//!
//! The `N.world.gzip[.pugbackup]` naming is Core Keeper's own on-disk format (confirmed
//! on the reference LXC — CONTEXT.md), not `config.worldDetection.patterns` — that glob
//! is for target-side discovery/matching, but grouping by slot number needs this exact
//! structure regardless of what pattern string is configured.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalWorldCandidate {
    pub slot: u32,
    pub main_path: String,
    pub main_size: u64,
    pub main_modified: Option<i64>,
    pub pugbackup_path: Option<String>,
    pub pugbackup_size: Option<u64>,
}

struct Entry {
    path: String,
    size: u64,
    modified: Option<i64>,
}

/// `selected` is whatever path the native dialog returned: a folder, a `.zip`, or a
/// single `*.world.gzip[.pugbackup]` file. Blocking fs/zip work runs off the async
/// runtime via `spawn_blocking`.
pub async fn resolve_local_world(selected: &Path) -> anyhow::Result<Vec<LocalWorldCandidate>> {
    let path = selected.to_path_buf();
    tokio::task::spawn_blocking(move || resolve_blocking(&path)).await?
}

fn resolve_blocking(selected: &Path) -> anyhow::Result<Vec<LocalWorldCandidate>> {
    let scan_root: PathBuf = if selected.is_dir() {
        selected.to_path_buf()
    } else if selected
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
    {
        extract_zip_to_temp(selected)?
    } else if selected
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| parse_world_filename(n).is_some())
    {
        selected
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        bail!(
            "'{}' isn't a folder, a .zip, or a *.world.gzip file",
            selected.display()
        );
    };

    let mut found: HashMap<u32, (Option<Entry>, Option<Entry>)> = HashMap::new();
    walk(&scan_root, &mut found)?;

    let mut candidates: Vec<LocalWorldCandidate> = found
        .into_iter()
        .filter_map(|(slot, (main, pugbackup))| {
            let main = main?;
            Some(LocalWorldCandidate {
                slot,
                main_path: main.path,
                main_size: main.size,
                main_modified: main.modified,
                pugbackup_path: pugbackup.as_ref().map(|p| p.path.clone()),
                pugbackup_size: pugbackup.as_ref().map(|p| p.size),
            })
        })
        .collect();
    candidates.sort_by_key(|c| c.slot);

    if candidates.is_empty() {
        bail!("no *.world.gzip files found under {}", scan_root.display());
    }
    Ok(candidates)
}

fn walk(dir: &Path, found: &mut HashMap<u32, (Option<Entry>, Option<Entry>)>) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            walk(&path, found)?;
        } else if file_type.is_file() {
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            let Some((slot, is_pugbackup)) = parse_world_filename(name) else {
                continue;
            };
            let meta = entry.metadata()?;
            let e = Entry {
                path: path.to_string_lossy().to_string(),
                size: meta.len(),
                modified: meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64),
            };
            let slot_entry = found.entry(slot).or_insert((None, None));
            if is_pugbackup {
                slot_entry.1 = Some(e);
            } else {
                slot_entry.0 = Some(e);
            }
        }
    }
    Ok(())
}

/// Returns `(slot, is_pugbackup)` for `N.world.gzip` / `N.world.gzip.pugbackup`.
pub(crate) fn parse_world_filename(name: &str) -> Option<(u32, bool)> {
    let (num_part, is_pugbackup) = if let Some(stripped) = name.strip_suffix(".world.gzip.pugbackup") {
        (stripped, true)
    } else {
        (name.strip_suffix(".world.gzip")?, false)
    };
    num_part.parse::<u32>().ok().map(|n| (n, is_pugbackup))
}

fn extract_zip_to_temp(zip_path: &Path) -> anyhow::Result<PathBuf> {
    let file = std::fs::File::open(zip_path)
        .with_context(|| format!("opening {}", zip_path.display()))?;
    let mut archive =
        zip::ZipArchive::new(file).with_context(|| format!("reading zip {}", zip_path.display()))?;

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dest = std::env::temp_dir().join(format!("ckm-extract-{nanos}"));
    std::fs::create_dir_all(&dest)?;
    archive
        .extract(&dest)
        .with_context(|| format!("extracting {} to {}", zip_path.display(), dest.display()))?;
    Ok(dest)
}
