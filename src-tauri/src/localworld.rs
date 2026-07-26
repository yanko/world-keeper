//! Local world detection (SPEC.md §7 `ResolvingLocalWorld`, §9 "World detection is
//! smart"). Given whatever the native file/folder/zip dialog handed back, find every
//! `N.world.gzip` (+ its optional `.pugbackup` sibling) and group them by slot.
//!
//! The `N.world.gzip[.pugbackup]` naming is Core Keeper's own on-disk format (confirmed
//! on the reference LXC — CONTEXT.md), not `config.worldDetection.patterns` — that glob
//! is for target-side discovery/matching, but grouping by slot number needs this exact
//! structure regardless of what pattern string is configured.
//!
//! Not every real world file is named that way, though (a manual export, a file shared
//! by someone else, anything renamed along the way) — feature added 2026-07-26: a file
//! that merely *contains* `.world.gzip` is still accepted, just without an inferable
//! slot — `slot_inferred: false` tells the frontend to ask for one instead of assuming
//! the filename encodes it.
//!
//! Recognition is filename-only, deliberately — despite the extension, these files
//! aren't actually standard gzip (confirmed directly: the reference `gzip -t` utility
//! itself rejects a real `0.world.gzip` as "not in gzip format"; the real magic bytes
//! aren't `1F 8B` at all). An earlier version of this tried a gzip-magic-bytes +
//! decompress check for exactly this reason and it rejected *every* real world file,
//! strictly-named ones included, since it was validating against a format these files
//! don't actually use. There's no known alternative signature to check for either — see
//! SPEC's note on this. So: the name is genuinely the only signal available.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalWorldCandidate {
    pub slot: u32,
    /// `false` when `slot` is just a default (0) because the filename didn't encode
    /// one — the frontend shows an editable slot field instead of treating it as fixed.
    pub slot_inferred: bool,
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
/// single file. Blocking fs/zip work runs off the async runtime via `spawn_blocking`.
pub async fn resolve_local_world(selected: &Path) -> anyhow::Result<Vec<LocalWorldCandidate>> {
    let path = selected.to_path_buf();
    tokio::task::spawn_blocking(move || resolve_blocking(&path)).await?
}

fn resolve_blocking(selected: &Path) -> anyhow::Result<Vec<LocalWorldCandidate>> {
    if selected.is_dir() {
        return scan_strict(selected);
    }
    if selected
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
    {
        return scan_strict(&extract_zip_to_temp(selected)?);
    }

    let name = selected.file_name().and_then(|n| n.to_str()).unwrap_or("");

    // Exact `N.world.gzip[.pugbackup]` — scan its directory too, so a sibling
    // `.pugbackup` (or other slots sitting alongside it) still gets picked up, same as
    // picking the containing folder directly would.
    if parse_world_filename(name).is_some() {
        let dir = selected
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        return scan_strict(&dir);
    }

    // Doesn't match the naming convention — still accept it if its name at least
    // mentions "world.gzip" somewhere, defaulting to slot 0 (the frontend lets this be
    // changed before pushing). No content validation — see the module doc comment.
    if name.to_lowercase().contains(".world.gzip") {
        let meta = std::fs::metadata(selected)
            .with_context(|| format!("reading metadata for {}", selected.display()))?;
        return Ok(vec![LocalWorldCandidate {
            slot: 0,
            slot_inferred: false,
            main_path: selected.to_string_lossy().to_string(),
            main_size: meta.len(),
            main_modified: modified_epoch(&meta),
            pugbackup_path: None,
            pugbackup_size: None,
        }]);
    }

    bail!(
        "'{}' isn't a folder, a .zip, or a world.gzip file",
        selected.display()
    );
}

/// Folder/zip-root scan: every `N.world.gzip[.pugbackup]` found (recursively), grouped
/// by the slot number its own filename encodes. Anything not named that way is simply
/// not found by this scan — pick that single file directly instead (see
/// `resolve_blocking`) to get the loose-match path.
fn scan_strict(root: &Path) -> anyhow::Result<Vec<LocalWorldCandidate>> {
    let mut found: HashMap<u32, (Option<Entry>, Option<Entry>)> = HashMap::new();
    walk(root, &mut found)?;

    let mut candidates: Vec<LocalWorldCandidate> = found
        .into_iter()
        .filter_map(|(slot, (main, pugbackup))| {
            let main = main?;
            Some(LocalWorldCandidate {
                slot,
                slot_inferred: true,
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
        bail!("no *.world.gzip files found under {}", root.display());
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
                modified: modified_epoch(&meta),
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

fn modified_epoch(meta: &std::fs::Metadata) -> Option<i64> {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
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
