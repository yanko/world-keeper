//! The target-agnostic seam (SPEC.md §4).
//!
//! `ServerTarget` is the only thing the (future) push/pull state machine is allowed to
//! call. Transport is always SSH in v1, but *how* the service is stopped/started and
//! where the world files live varies by `ServiceKind`, which is discovered on connect
//! (see `discovery.rs`) rather than declared by the user. All of that branching lives in
//! the concrete impl (`ssh.rs`); this module only defines the contract.

use std::path::Path;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// How the game server is controlled on the target. Discovered, not configured.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ServiceKind {
    Systemd {
        unit: String,
    },
    Docker {
        container: String,
    },
    // `rename_all` on the enum only renames variant tags, not struct-variant fields —
    // this override keeps Process's fields camelCase like everything else in the app.
    #[serde(rename_all = "camelCase")]
    Process {
        stop_cmd: String,
        start_cmd: String,
        log_path: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceAction {
    Status,
    Stop,
    Start,
}

/// Result of running a command on the target (SPEC's `Command`/`Output` are collapsed
/// into a plain shell string in and this struct out — every discovery/service-control
/// command in SPEC §8/§4 is already a shell one-liner, so a typed command builder would
/// buy nothing here).
#[derive(Debug, Clone, Default)]
pub struct ExecOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_status: u32,
}

impl ExecOutput {
    pub fn success(&self) -> bool {
        self.exit_status == 0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
    /// Unix epoch seconds, when available.
    pub modified: Option<i64>,
}

/// Transport + control surface the state machine is written against. SSH is the only
/// impl in v1 (`ssh::SshTarget`); a Windows target would be a second impl of this same
/// trait, added without touching the state machine.
#[async_trait]
pub trait ServerTarget: Send + Sync {
    async fn exec(&self, cmd: &str) -> anyhow::Result<ExecOutput>;
    async fn push_file(&self, local: &Path, remote: &str) -> anyhow::Result<u64>;
    async fn pull_file(&self, remote: &str, local: &Path) -> anyhow::Result<u64>;
    /// List files under `dir` matching a shell glob (e.g. `*.world.gzip`).
    async fn stat(&self, dir: &str, glob: &str) -> anyhow::Result<Vec<FileEntry>>;
    /// Tail of recent log output (journalctl / docker logs / file tail depending on
    /// ServiceKind). Live `-f` streaming into the log drawer (SPEC §10) is wired up
    /// alongside the log drawer UI, not here.
    async fn logs_tail(&self, lines: u32) -> anyhow::Result<String>;
    async fn service_control(&self, action: ServiceAction) -> anyhow::Result<ExecOutput>;
    /// When the service last (re)started, if the ServiceKind can report it (systemd's
    /// `ActiveEnterTimestamp`, docker's `State.StartedAt`); `None` for Process targets.
    async fn active_since(&self) -> anyhow::Result<Option<String>>;
    async fn fix_perms(&self, path: &str) -> anyhow::Result<()>;
    fn worlds_path(&self) -> &str;
    fn service_kind(&self) -> &ServiceKind;
}
