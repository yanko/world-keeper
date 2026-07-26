//! Auto-discovery probe (SPEC.md §8): "how do I stop the server here, and where are its
//! worlds" — answered by running read-only commands over the raw SSH session, never by
//! asking the user to declare a target type.
//!
//! Runs against `SshSession` directly (not `ServerTarget`) because the whole point is to
//! *determine* the `ServiceKind` a `ServerTarget` would need — this is the one place in
//! the app that's allowed to know about systemd/docker/process before a target exists.
//! The result is always surfaced for confirmation before being written to
//! `profile.discovered`; this module never saves anything itself.

use chrono::Utc;
use serde::{Deserialize, Serialize};

use crate::ssh::{shq, SshSession};
use crate::target::ServiceKind;

/// A systemd/docker/process hit that *matched* the "looks like Core Keeper" grep but
/// couldn't be auto-selected because it wasn't the only one. Carries enough identifying
/// detail for a human to tell "corekeeper" apart from, say, an unrelated "corekeeper-api"
/// sidecar — real ambiguity seen on the reference LXC's systemd units, and the same
/// grep-can-multi-match risk applies to the docker probe below.
///
/// `exec_start` doubles up across ServiceKinds: systemd's literal `ExecStart=` line, or
/// docker's image name — both answer "what does this actually run."
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServiceKindCandidate {
    pub kind: ServiceKind,
    pub exec_start: Option<String>,
    pub working_directory: Option<String>,
    pub steam_user: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveryResult {
    pub os: Option<String>,
    pub service_kind: Option<ServiceKind>,
    /// Populated instead of `service_kind` when more than one service matched "looks
    /// like Core Keeper" (e.g. the real unit plus an unrelated API sidecar); the UI
    /// should ask the user to pick one.
    pub service_kind_candidates: Vec<ServiceKindCandidate>,
    pub worlds_path: Option<String>,
    /// Populated instead of `worlds_path` when more than one `*.world.gzip` parent dir
    /// was found; the UI should ask the user to pick one.
    pub worlds_path_candidates: Vec<String>,
    pub steam_user: Option<String>,
    pub launch_script: Option<String>,
    pub probed_at: String,
}

pub async fn probe(session: &SshSession) -> anyhow::Result<DiscoveryResult> {
    let os = probe_os(session).await;
    let service_kind_probe = probe_service_kind(session).await;
    let (worlds_path, worlds_path_candidates) =
        probe_worlds_path(session, &service_kind_probe.service_kind).await;

    Ok(DiscoveryResult {
        os,
        service_kind: service_kind_probe.service_kind,
        service_kind_candidates: service_kind_probe.candidates,
        worlds_path,
        worlds_path_candidates,
        steam_user: service_kind_probe.steam_user,
        launch_script: service_kind_probe.launch_script,
        probed_at: Utc::now().to_rfc3339(),
    })
}

async fn probe_os(session: &SshSession) -> Option<String> {
    let out = session.exec("cat /etc/os-release 2>/dev/null").await.ok()?;
    out.stdout
        .lines()
        .find_map(|l| l.strip_prefix("PRETTY_NAME="))
        .map(|v| v.trim_matches('"').to_string())
}

struct ServiceKindProbe {
    service_kind: Option<ServiceKind>,
    candidates: Vec<ServiceKindCandidate>,
    launch_script: Option<String>,
    steam_user: Option<String>,
}

impl ServiceKindProbe {
    fn none() -> Self {
        Self {
            service_kind: None,
            candidates: Vec::new(),
            launch_script: None,
            steam_user: None,
        }
    }
}

/// How closely a discovered name matches "core keeper" — 0 is closest. Used only to
/// *order* multiple candidates (best guess sorts first, which the confirmation UI
/// pre-selects) — never to silently auto-pick one instead of showing the full list
/// (SPEC §8: always confirm). Hyphen/underscore/case-insensitive, so "core-keeper" and
/// "corekeeper" tie for rank 0; a suffixed lookalike like "core-keeper-api" ranks below.
fn specificity_rank(name: &str) -> u8 {
    let normalized: String = name
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase();
    if normalized == "corekeeper" {
        0
    } else if normalized.starts_with("corekeeper") || normalized.ends_with("corekeeper") {
        1
    } else {
        2
    }
}

/// Step B (SPEC §8): systemd, then docker, then a raw process, in that order — whichever
/// actually serves Core Keeper wins if more than one is present. A `grep -i core` can
/// still match more than one unrelated service (confirmed on the reference LXC: a
/// `corekeeper-api` sidecar alongside the real `corekeeper` unit) — when that happens we
/// refuse to guess, but do sort the closest-looking match first so confirming is a
/// single click instead of a coin flip.
async fn probe_service_kind(session: &SshSession) -> ServiceKindProbe {
    // 1) systemd units
    if let Ok(out) = session
        .exec("systemctl list-units --type=service 2>/dev/null | grep -i core")
        .await
    {
        let mut units: Vec<String> = out
            .stdout
            .lines()
            .filter_map(|line| line.split_whitespace().next())
            .map(|u| u.trim_end_matches(".service").to_string())
            .collect();
        units.sort_by_key(|u| specificity_rank(u));

        if units.len() == 1 {
            let unit = units.into_iter().next().expect("len == 1");
            let info = probe_systemd_unit(session, &unit).await;
            return ServiceKindProbe {
                service_kind: Some(ServiceKind::Systemd { unit }),
                candidates: Vec::new(),
                launch_script: info.exec_start,
                steam_user: info.user,
            };
        } else if units.len() > 1 {
            let mut candidates = Vec::new();
            for unit in units {
                let info = probe_systemd_unit(session, &unit).await;
                candidates.push(ServiceKindCandidate {
                    kind: ServiceKind::Systemd { unit },
                    exec_start: info.exec_start,
                    working_directory: info.working_directory,
                    steam_user: info.user,
                });
            }
            return ServiceKindProbe {
                service_kind: None,
                candidates,
                launch_script: None,
                steam_user: None,
            };
        }
    }

    // 2) docker containers — same "grep can match more than one thing" risk as
    // systemd above, so the same fix applies: don't guess, surface every hit.
    if let Ok(out) = session
        .exec("docker ps --format '{{.Names}} {{.Image}}' 2>/dev/null | grep -i core")
        .await
    {
        let mut containers: Vec<(String, String)> = out
            .stdout
            .lines()
            .filter_map(|line| {
                let mut parts = line.split_whitespace();
                let name = parts.next()?.to_string();
                let image = parts.next().unwrap_or("").to_string();
                Some((name, image))
            })
            .collect();
        containers.sort_by_key(|(name, _)| specificity_rank(name));

        if containers.len() == 1 {
            let (container, _image) = containers.into_iter().next().expect("len == 1");
            return ServiceKindProbe {
                service_kind: Some(ServiceKind::Docker { container }),
                ..ServiceKindProbe::none()
            };
        } else if containers.len() > 1 {
            let candidates = containers
                .into_iter()
                .map(|(container, image)| ServiceKindCandidate {
                    kind: ServiceKind::Docker { container },
                    exec_start: (!image.is_empty()).then_some(image),
                    working_directory: None,
                    steam_user: None,
                })
                .collect();
            return ServiceKindProbe {
                candidates,
                ..ServiceKindProbe::none()
            };
        }
    }

    // 3) raw process
    if let Ok(out) = session
        .exec("pgrep -af _launch.sh 2>/dev/null || pgrep -af CoreKeeperServer 2>/dev/null")
        .await
    {
        if let Some(line) = out.stdout.lines().next() {
            // `pgrep -af` prints "<pid> <full command line>"
            let cmdline = line
                .split_once(' ')
                .map_or(line, |(_, rest)| rest)
                .to_string();
            return ServiceKindProbe {
                service_kind: Some(ServiceKind::Process {
                    stop_cmd: format!("pkill -f {}", shq(&cmdline)),
                    start_cmd: cmdline,
                    log_path: None,
                }),
                ..ServiceKindProbe::none()
            };
        }
    }

    ServiceKindProbe::none()
}

struct SystemdUnitInfo {
    exec_start: Option<String>,
    working_directory: Option<String>,
    user: Option<String>,
}

async fn probe_systemd_unit(session: &SshSession, unit: &str) -> SystemdUnitInfo {
    let Ok(out) = session
        .exec(&format!(
            "systemctl cat {} 2>/dev/null | grep -iE 'ExecStart=|User=|WorkingDirectory='",
            shq(unit)
        ))
        .await
    else {
        return SystemdUnitInfo {
            exec_start: None,
            working_directory: None,
            user: None,
        };
    };

    let mut exec_start = None;
    let mut working_directory = None;
    let mut user = None;
    for line in out.stdout.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("ExecStart=") {
            exec_start = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("WorkingDirectory=") {
            working_directory = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("User=") {
            user = Some(v.trim().to_string());
        }
    }
    SystemdUnitInfo {
        exec_start,
        working_directory,
        user,
    }
}

/// Step C (SPEC §8): locate the worlds dir via `find`. For a Docker target, the search
/// is scoped to the container's own host-side mounts (never inside the container itself
/// — Docker containers don't expose SSH, so every file op has to go through the host's
/// filesystem view of whatever's mounted in).
async fn probe_worlds_path(
    session: &SshSession,
    service_kind: &Option<ServiceKind>,
) -> (Option<String>, Vec<String>) {
    if let Some(ServiceKind::Docker { container }) = service_kind {
        return probe_docker_worlds_path(session, container).await;
    }

    let Ok(out) = session
        .exec("find /home -name '*.world.gzip' 2>/dev/null")
        .await
    else {
        return (None, Vec::new());
    };
    dedupe_parent_dirs(&out.stdout)
}

/// Takes `find -name '*.world.gzip'` output (one match per line) to a worlds-path
/// result: the parent dir of each match, deduped — 0 found, exactly 1 (confident), or
/// several (surfaced as candidates, never guessed at). Shared between the plain-host
/// search above and the per-mount Docker search below, which both reduce to this same
/// "here are the matches, now what" step.
fn dedupe_parent_dirs(find_stdout: &str) -> (Option<String>, Vec<String>) {
    let mut dirs: Vec<String> = find_stdout
        .lines()
        .filter_map(|line| {
            let path = std::path::Path::new(line);
            path.parent().map(|p| p.to_string_lossy().to_string())
        })
        .collect();
    dirs.sort();
    dirs.dedup();

    match dirs.len() {
        0 => (None, Vec::new()),
        1 => (Some(dirs.remove(0)), Vec::new()),
        _ => (None, dirs),
    }
}

/// Can't guess a Docker container's worlds path from its mount *destination* path the
/// way an earlier version of this tried to (checking for "unity3d"/".config" in the
/// destination) — that only happened to work because the first Docker deployment this
/// was built against bind-mounted Unity3D's own per-user config dir 1:1. Other images
/// lay out their internal filesystem completely differently: `ich777/steamcmd:
/// corekeeper` (seen running on Unraid) bind-mounts a flat `/serverdata/serverfiles`
/// and nests the actual world files two levels down, under `Save/worlds/`, inside that
/// — no naming convention to pattern-match on, and a container can have several
/// unrelated mounts (that image also separately mounts `/serverdata/steamcmd`, which
/// has nothing to do with worlds). So: search inside *every* one of the container's
/// host-side mount sources for `*.world.gzip`, same as the plain-host search above,
/// just scoped to the container's own mounts instead of `/home` — searching the whole
/// host wouldn't help, since a bind-mount source is rarely under `/home` itself (e.g.
/// Unraid's are under `/mnt/user/appdata/`).
async fn probe_docker_worlds_path(
    session: &SshSession,
    container: &str,
) -> (Option<String>, Vec<String>) {
    let Some(mount_sources) = docker_mount_sources(session, container).await else {
        return (None, Vec::new());
    };

    let mut all_matches = String::new();
    for source in &mount_sources {
        if let Ok(out) = session
            .exec(&format!("find {} -name '*.world.gzip' 2>/dev/null", shq(source)))
            .await
        {
            all_matches.push_str(&out.stdout);
            if !out.stdout.ends_with('\n') {
                all_matches.push('\n');
            }
        }
    }
    dedupe_parent_dirs(&all_matches)
}

/// Every mount's host-side source path, `bind` or named `volume` alike — both are, in
/// the end, just a real directory on the host filesystem that plain `find`/SFTP over
/// this same SSH session can already read and write.
async fn docker_mount_sources(session: &SshSession, container: &str) -> Option<Vec<String>> {
    // The Go-template format string contains a space ("json .Mounts") — unquoted, the
    // remote shell word-splits it into two argv entries before `docker` ever sees it,
    // breaking `--format`'s single-argument parsing. Bug predates this function (the
    // `probe_docker_bind_mount` this replaced had the same unquoted string); it was
    // silently masked there because that deployment's real worlds path happened to
    // live under `/home`, where the fallback `find` still found it regardless of
    // whether this mount lookup actually worked.
    let out = session
        .exec(&format!(
            "docker inspect {} --format {} 2>/dev/null",
            shq(container),
            shq("{{json .Mounts}}")
        ))
        .await
        .ok()?;
    let mounts: serde_json::Value = serde_json::from_str(out.stdout.trim()).ok()?;
    let mounts = mounts.as_array()?;
    Some(
        mounts
            .iter()
            .filter_map(|m| m.get("Source").and_then(|s| s.as_str()).map(String::from))
            .collect(),
    )
}
