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
/// couldn't be auto-selected because it wasn't the only one. Carries enough of its own
/// `systemctl cat` output for a human to tell "corekeeper" apart from, say, an unrelated
/// "corekeeper-api" sidecar — real ambiguity seen on the reference LXC (SPEC's `Process`/
/// `Docker` variants don't need this yet since only the systemd probe has hit it).
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

/// Step B (SPEC §8): systemd, then docker, then a raw process, in that order — whichever
/// actually serves Core Keeper wins if more than one is present. A `grep -i core` can
/// still match more than one unrelated service (confirmed on the reference LXC: a
/// `corekeeper-api` sidecar alongside the real `corekeeper` unit) — when that happens we
/// refuse to guess and surface every match as a candidate instead.
async fn probe_service_kind(session: &SshSession) -> ServiceKindProbe {
    // 1) systemd units
    if let Ok(out) = session
        .exec("systemctl list-units --type=service 2>/dev/null | grep -i core")
        .await
    {
        let units: Vec<String> = out
            .stdout
            .lines()
            .filter_map(|line| line.split_whitespace().next())
            .map(|u| u.trim_end_matches(".service").to_string())
            .collect();

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

    // 2) docker container
    if let Ok(out) = session
        .exec("docker ps --format '{{.Names}} {{.Image}}' 2>/dev/null | grep -i core")
        .await
    {
        if let Some(line) = out.stdout.lines().next() {
            if let Some(container) = line.split_whitespace().next() {
                return ServiceKindProbe {
                    service_kind: Some(ServiceKind::Docker {
                        container: container.to_string(),
                    }),
                    ..ServiceKindProbe::none()
                };
            }
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

/// Step C (SPEC §8): locate the worlds dir via `find`. Docker's bind-mount source is
/// preferred when a docker container was found, since that's the host path (not the
/// in-container path).
async fn probe_worlds_path(
    session: &SshSession,
    service_kind: &Option<ServiceKind>,
) -> (Option<String>, Vec<String>) {
    if let Some(ServiceKind::Docker { container }) = service_kind {
        if let Some(path) = probe_docker_bind_mount(session, container).await {
            return (Some(path), Vec::new());
        }
    }

    let Ok(out) = session
        .exec("find /home -name '*.world.gzip' 2>/dev/null")
        .await
    else {
        return (None, Vec::new());
    };

    let mut dirs: Vec<String> = out
        .stdout
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

async fn probe_docker_bind_mount(session: &SshSession, container: &str) -> Option<String> {
    let out = session
        .exec(&format!(
            "docker inspect {} --format {{{{json .Mounts}}}} 2>/dev/null",
            shq(container)
        ))
        .await
        .ok()?;
    let mounts: serde_json::Value = serde_json::from_str(out.stdout.trim()).ok()?;
    let mounts = mounts.as_array()?;

    // Prefer a bind mount whose destination looks like the Unity3D config dir; fall
    // back to the first bind mount otherwise.
    let mut first_bind = None;
    for mount in mounts {
        if mount.get("Type").and_then(|t| t.as_str()) != Some("bind") {
            continue;
        }
        let source = mount.get("Source").and_then(|s| s.as_str())?.to_string();
        let dest = mount.get("Destination").and_then(|d| d.as_str()).unwrap_or("");
        if dest.contains("unity3d") || dest.contains(".config") {
            return Some(source);
        }
        first_bind.get_or_insert(source);
    }
    first_bind
}
