//! Static reference documentation for every remote shell command this app can issue over
//! SSH — backs the About dialog's "command reference" list (brainstormed and requested
//! 2026-07-26). Nothing here executes anything; each entry mirrors a real command string
//! built in `discovery.rs`/`ssh.rs`/`push.rs`/`pull.rs`/`restore.rs`/`stats.rs`, grouped
//! and ordered the same way the actual operation runs them, so the About dialog can show
//! a user exactly what "Push", "Pull", "Restore", etc. do against their server —
//! placeholders like `<worlds_path>` when nothing is known yet, real discovered values
//! once connected.
//!
//! Deliberately a static Rust list, not a DB table: this documents the code, not user
//! data, so it needs no migration and is naturally unaffected by "Clear audit log" (it
//! isn't stored in that database at all).

use serde::Serialize;

use crate::target::ServiceKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TargetKind {
    Systemd,
    Docker,
    Process,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandRefEntry {
    pub label: String,
    pub command: String,
    /// `None` when the command runs the same way regardless of `ServiceKind` (e.g. `df
    /// -h`); otherwise which kind's variant this entry documents. Filtered down to just
    /// the discovered kind once one is known — see `build`.
    pub target_kind: Option<TargetKind>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandRefGroup {
    pub group: String,
    pub entries: Vec<CommandRefEntry>,
}

/// Connection facts known so far — every field `None` when fully disconnected. Mirrors
/// `Connection::Raw`/`Target` from `state.rs`: a raw connection knows `host` but not yet
/// a `ServiceKind` or worlds path.
#[derive(Debug, Clone, Default)]
pub struct KnownValues {
    pub worlds_path: Option<String>,
    pub service_kind: Option<ServiceKind>,
}

fn kind_tag(k: &ServiceKind) -> TargetKind {
    match k {
        ServiceKind::Systemd { .. } => TargetKind::Systemd,
        ServiceKind::Docker { .. } => TargetKind::Docker,
        ServiceKind::Process { .. } => TargetKind::Process,
    }
}

/// Real value if known, else an `<placeholder>` for display.
fn known_or(value: &Option<String>, placeholder: &str) -> String {
    value.clone().unwrap_or_else(|| format!("<{placeholder}>"))
}

pub fn build(known: &KnownValues) -> Vec<CommandRefGroup> {
    let worlds_path = known_or(&known.worlds_path, "worlds_path");
    let connected_kind = known.service_kind.as_ref().map(kind_tag);

    // Steam user isn't threaded through to this dialog (it only ever feeds one
    // best-effort `chown`) — always shown as a placeholder rather than adding a getter
    // to `ServerTarget` just for this cosmetic list.
    let (unit, container, start_cmd, stop_cmd, log_path) = match &known.service_kind {
        Some(ServiceKind::Systemd { unit }) => (
            unit.clone(),
            "<container>".to_string(),
            "<start_cmd>".to_string(),
            "<stop_cmd>".to_string(),
            "<log_path>".to_string(),
        ),
        Some(ServiceKind::Docker { container }) => (
            "<unit>".to_string(),
            container.clone(),
            "<start_cmd>".to_string(),
            "<stop_cmd>".to_string(),
            "<log_path>".to_string(),
        ),
        Some(ServiceKind::Process { start_cmd, stop_cmd, log_path }) => (
            "<unit>".to_string(),
            "<container>".to_string(),
            start_cmd.clone(),
            stop_cmd.clone(),
            log_path.clone().unwrap_or_else(|| "<log_path>".to_string()),
        ),
        None => (
            "<unit>".to_string(),
            "<container>".to_string(),
            "<start_cmd>".to_string(),
            "<stop_cmd>".to_string(),
            "<log_path>".to_string(),
        ),
    };

    vec![
        discovery_group(),
        stats_group(&worlds_path, &unit, &container, &start_cmd, &log_path, connected_kind),
        push_group(&worlds_path, &unit, &container, &start_cmd, &stop_cmd, connected_kind),
        pull_group(&worlds_path, &unit, &container, &start_cmd, &stop_cmd, connected_kind),
        restore_group(&worlds_path, &unit, &container, &start_cmd, &stop_cmd, connected_kind),
        terminal_group(),
        setup_group(),
    ]
    .into_iter()
    .map(|(group, items)| filter_group(group, items, connected_kind))
    .collect()
}

type Item = (Option<TargetKind>, &'static str, String);

fn filter_group(group: &str, items: Vec<Item>, connected_kind: Option<TargetKind>) -> CommandRefGroup {
    let entries = items
        .into_iter()
        .filter(|(kind, _, _)| match (kind, connected_kind) {
            (Some(k), Some(ck)) => *k == ck,
            _ => true,
        })
        .map(|(kind, label, command)| CommandRefEntry {
            label: label.to_string(),
            command,
            target_kind: kind,
        })
        .collect();
    CommandRefGroup { group: group.to_string(), entries }
}

fn discovery_group() -> (&'static str, Vec<Item>) {
    (
        "Discovery",
        vec![
            (None, "Identify the OS", "cat /etc/os-release 2>/dev/null".into()),
            (
                Some(TargetKind::Systemd),
                "Find Core Keeper systemd units",
                "systemctl list-units --type=service 2>/dev/null | grep -i core".into(),
            ),
            (
                Some(TargetKind::Systemd),
                "Inspect a matched unit",
                "systemctl cat <unit> 2>/dev/null | grep -iE 'ExecStart=|User=|WorkingDirectory='".into(),
            ),
            (
                Some(TargetKind::Docker),
                "Find Core Keeper docker containers",
                "docker ps --format '{{.Names}} {{.Image}}' 2>/dev/null | grep -i core".into(),
            ),
            (
                Some(TargetKind::Docker),
                "List a matched container's mounts",
                "docker inspect <container> --format '{{json .Mounts}}' 2>/dev/null".into(),
            ),
            (
                Some(TargetKind::Docker),
                "Search each mount for world files",
                "find <mount_source> -name '*.world.gzip' 2>/dev/null".into(),
            ),
            (
                Some(TargetKind::Process),
                "Find a bare Core Keeper process",
                "pgrep -af _launch.sh 2>/dev/null || pgrep -af CoreKeeperServer 2>/dev/null".into(),
            ),
            (
                None,
                "Search /home for world files (non-Docker targets)",
                "find /home -name '*.world.gzip' 2>/dev/null".into(),
            ),
        ],
    )
}

#[allow(clippy::too_many_arguments)]
fn stats_group(
    worlds_path: &str,
    unit: &str,
    container: &str,
    start_cmd: &str,
    log_path: &str,
    _connected_kind: Option<TargetKind>,
) -> (&'static str, Vec<Item>) {
    (
        "Server Stats",
        vec![
            (Some(TargetKind::Systemd), "Check status", format!("systemctl is-active {unit}")),
            (
                Some(TargetKind::Docker),
                "Check status",
                format!("docker inspect -f {{{{.State.Status}}}} {container}"),
            ),
            (Some(TargetKind::Process), "Check status", format!("pgrep -af {start_cmd}")),
            (
                Some(TargetKind::Systemd),
                "Since when it's been running",
                format!("systemctl show -p ActiveEnterTimestamp --value {unit}"),
            ),
            (
                Some(TargetKind::Docker),
                "Since when it's been running",
                format!("docker inspect -f {{{{.State.StartedAt}}}} {container}"),
            ),
            (None, "Server wall-clock time", "date -u +%Y-%m-%dT%H:%M:%SZ 2>/dev/null".into()),
            (
                Some(TargetKind::Systemd),
                "UDP port listening?",
                "ss -ulnp 2>/dev/null | grep -i corekeeper".into(),
            ),
            (
                Some(TargetKind::Process),
                "UDP port listening?",
                "ss -ulnp 2>/dev/null | grep -i corekeeper".into(),
            ),
            (
                Some(TargetKind::Docker),
                "UDP port listening? (host network mode)",
                "ss -ulnp 2>/dev/null | grep -i corekeeper".into(),
            ),
            (
                Some(TargetKind::Docker),
                "UDP port listening? (published ports)",
                format!("docker port {container} 2>/dev/null | grep -i udp"),
            ),
            (
                Some(TargetKind::Docker),
                "UDP port listening? (inside the container)",
                format!("docker exec {container} ss -uln 2>/dev/null"),
            ),
            (
                Some(TargetKind::Docker),
                "UDP port listening? (last resort, /proc)",
                format!("docker exec {container} sh -c 'cat /proc/net/udp /proc/net/udp6 2>/dev/null'"),
            ),
            (None, "Memory usage", "cat /proc/meminfo 2>/dev/null".into()),
            (
                None,
                "Disk usage of the worlds path",
                format!("df -h --output=size,used,avail,pcent {worlds_path} 2>/dev/null | tail -n 1"),
            ),
            (None, "Host uptime", "cat /proc/uptime 2>/dev/null".into()),
            (
                Some(TargetKind::Systemd),
                "Recent log tail (game id + live log drawer)",
                format!("journalctl -u {unit} -n <lines> --no-pager"),
            ),
            (
                Some(TargetKind::Docker),
                "Recent log tail (game id + live log drawer)",
                format!("docker logs --tail <lines> {container}"),
            ),
            (
                Some(TargetKind::Process),
                "Recent log tail (game id + live log drawer)",
                format!("tail -n <lines> {log_path}"),
            ),
        ],
    )
}

#[allow(clippy::too_many_arguments)]
fn push_group(
    worlds_path: &str,
    unit: &str,
    container: &str,
    start_cmd: &str,
    stop_cmd: &str,
    _connected_kind: Option<TargetKind>,
) -> (&'static str, Vec<Item>) {
    (
        "Push a World",
        vec![
            (None, "Create a staging directory", format!("mkdir -p {worlds_path}/.staging")),
            (
                None,
                "Upload the world file(s) to staging",
                "(via SFTP — binary transfer, not a shell command)".into(),
            ),
            (
                None,
                "Verify the upload's checksum",
                format!("sha256sum {worlds_path}/.staging/<slot>.world.gzip"),
            ),
            (Some(TargetKind::Systemd), "Check status", format!("systemctl is-active {unit}")),
            (
                Some(TargetKind::Docker),
                "Check status",
                format!("docker inspect -f {{{{.State.Status}}}} {container}"),
            ),
            (Some(TargetKind::Process), "Check status", format!("pgrep -af {start_cmd}")),
            (Some(TargetKind::Systemd), "Stop the service", format!("systemctl stop {unit}")),
            (Some(TargetKind::Docker), "Stop the service", format!("docker stop {container}")),
            (Some(TargetKind::Process), "Stop the service", stop_cmd.to_string()),
            (
                None,
                "List the slot's existing world file, if any",
                format!("find {worlds_path} -maxdepth 1 -name '<slot>.world.gzip' -printf '%s %T@ %p\\n' 2>/dev/null"),
            ),
            (
                None,
                "Create a dated backup directory",
                format!("mkdir -p {worlds_path}/.backups/<slot>.<timestamp>"),
            ),
            (
                None,
                "Copy the existing world file(s) into the backup",
                format!("cp -a {worlds_path}/<slot>.world.gzip {worlds_path}/.backups/<slot>.<timestamp>/"),
            ),
            (
                None,
                "Move the staged upload into place",
                format!("mv {worlds_path}/.staging/<slot>.world.gzip {worlds_path}/<slot>.world.gzip"),
            ),
            (
                Some(TargetKind::Systemd),
                "Fix ownership (only when a Steam user is configured)",
                format!("chown -R <user>:<user> {worlds_path}"),
            ),
            (Some(TargetKind::Systemd), "Start the service", format!("systemctl start {unit}")),
            (Some(TargetKind::Docker), "Start the service", format!("docker start {container}")),
            (Some(TargetKind::Process), "Start the service", start_cmd.to_string()),
            (
                Some(TargetKind::Systemd),
                "Capture the game id (polls the log tail)",
                format!("journalctl -u {unit} -n 200 --no-pager"),
            ),
            (
                Some(TargetKind::Docker),
                "Capture the game id (polls the log tail)",
                format!("docker logs --tail 200 {container}"),
            ),
            (
                None,
                "Verify: re-check status + UDP listening (reuses Server Stats' checks)",
                "(see Server Stats group)".into(),
            ),
            (
                None,
                "On failure — roll back: restore the safety backup",
                format!("cp -a {worlds_path}/.backups/<slot>.<timestamp>/. {worlds_path}/"),
            ),
        ],
    )
}

#[allow(clippy::too_many_arguments)]
fn pull_group(
    worlds_path: &str,
    unit: &str,
    container: &str,
    start_cmd: &str,
    stop_cmd: &str,
    _connected_kind: Option<TargetKind>,
) -> (&'static str, Vec<Item>) {
    (
        "Pull a World",
        vec![
            (
                None,
                "List worlds available to pull",
                format!("find {worlds_path} -maxdepth 1 -name '*.world.gzip' -printf '%s %T@ %p\\n' 2>/dev/null"),
            ),
            (Some(TargetKind::Systemd), "Check status", format!("systemctl is-active {unit}")),
            (
                Some(TargetKind::Docker),
                "Check status",
                format!("docker inspect -f {{{{.State.Status}}}} {container}"),
            ),
            (Some(TargetKind::Process), "Check status", format!("pgrep -af {start_cmd}")),
            (Some(TargetKind::Systemd), "Stop the service", format!("systemctl stop {unit}")),
            (Some(TargetKind::Docker), "Stop the service", format!("docker stop {container}")),
            (Some(TargetKind::Process), "Stop the service", stop_cmd.to_string()),
            (
                None,
                "Download the chosen slot's world file(s)",
                "(via SFTP — binary transfer, not a shell command)".into(),
            ),
            (Some(TargetKind::Systemd), "Start the service", format!("systemctl start {unit}")),
            (Some(TargetKind::Docker), "Start the service", format!("docker start {container}")),
            (Some(TargetKind::Process), "Start the service", start_cmd.to_string()),
            (
                None,
                "Package locally as a folder or zip",
                "(local file I/O only — nothing runs on the server)".into(),
            ),
        ],
    )
}

#[allow(clippy::too_many_arguments)]
fn restore_group(
    worlds_path: &str,
    unit: &str,
    container: &str,
    start_cmd: &str,
    stop_cmd: &str,
    _connected_kind: Option<TargetKind>,
) -> (&'static str, Vec<Item>) {
    (
        "Restore from Backup (World Library)",
        vec![
            (
                None,
                "List backups",
                format!("find {worlds_path}/.backups -maxdepth 1 -name '*' -printf '%s %T@ %p\\n' 2>/dev/null"),
            ),
            (
                None,
                "Size a backup's contents",
                "find <backup_path> -maxdepth 1 -name '*' -printf '%s %T@ %p\\n' 2>/dev/null".into(),
            ),
            (Some(TargetKind::Systemd), "Check status", format!("systemctl is-active {unit}")),
            (
                Some(TargetKind::Docker),
                "Check status",
                format!("docker inspect -f {{{{.State.Status}}}} {container}"),
            ),
            (Some(TargetKind::Process), "Check status", format!("pgrep -af {start_cmd}")),
            (Some(TargetKind::Systemd), "Stop the service", format!("systemctl stop {unit}")),
            (Some(TargetKind::Docker), "Stop the service", format!("docker stop {container}")),
            (Some(TargetKind::Process), "Stop the service", stop_cmd.to_string()),
            (
                None,
                "Back up the currently-live world first (same as Push's backup step)",
                format!("cp -a {worlds_path}/<slot>.world.gzip {worlds_path}/.backups/<slot>.<timestamp>/"),
            ),
            (
                None,
                "Copy the chosen backup's world file into place",
                format!("cp -a <backup_path>/<slot>.world.gzip {worlds_path}/"),
            ),
            (
                None,
                "Copy the chosen backup's pugbackup into place, if present",
                format!("cp -a <backup_path>/<slot>.world.gzip.pugbackup {worlds_path}/"),
            ),
            (
                Some(TargetKind::Systemd),
                "Fix ownership (only when a Steam user is configured)",
                format!("chown -R <user>:<user> {worlds_path}"),
            ),
            (Some(TargetKind::Systemd), "Start the service", format!("systemctl start {unit}")),
            (Some(TargetKind::Docker), "Start the service", format!("docker start {container}")),
            (Some(TargetKind::Process), "Start the service", start_cmd.to_string()),
            (
                None,
                "Capture the game id + verify (reuses Push's steps)",
                "(see Push a World group)".into(),
            ),
            (
                None,
                "On failure — roll back: restore the safety backup",
                format!("cp -a {worlds_path}/.backups/<slot>.<timestamp>/. {worlds_path}/"),
            ),
        ],
    )
}

fn terminal_group() -> (&'static str, Vec<Item>) {
    (
        "Terminal",
        vec![(
            None,
            "Open an interactive shell",
            "(opens a real PTY over SSH — whatever you type is sent as-is, exactly like `ssh user@host`; there's no fixed command string)".into(),
        )],
    )
}

fn setup_group() -> (&'static str, Vec<Item>) {
    (
        "One-Time Setup",
        vec![(
            None,
            "Install this app's generated public key (idempotent)",
            "mkdir -p ~/.ssh && chmod 700 ~/.ssh && touch ~/.ssh/authorized_keys && chmod 600 ~/.ssh/authorized_keys && (grep -qF '<public_key>' ~/.ssh/authorized_keys || echo '<public_key>' >> ~/.ssh/authorized_keys)".into(),
        )],
    )
}
