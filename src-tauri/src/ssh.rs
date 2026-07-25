//! SSH transport (`russh`/`russh-sftp`) and the SSH+ServiceKind `ServerTarget` impl.
//!
//! `SshSession` is the raw connect/exec/sftp primitive — discovery (`discovery.rs`) runs
//! against it directly, before a `ServiceKind` is even known. Once discovery confirms a
//! `ServiceKind` + worlds path, that session is wrapped in `SshTarget`, which is where
//! every bit of systemd/docker/process-specific behaviour lives (SPEC.md §4).

use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use russh::client::{self, Handle};
use russh::ChannelMsg;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::OpenFlags;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::target::{ExecOutput, FileEntry, ServerTarget, ServiceAction, ServiceKind};

/// Wraps a string for safe interpolation into a remote shell command (single-quoted,
/// with embedded quotes escaped). Needed because real paths here contain spaces, e.g.
/// `.../Pugstorm/Core Keeper/DedicatedServer/worlds`.
pub(crate) fn shq(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

struct Client;

impl client::Handler for Client {
    type Error = anyhow::Error;

    // Homelab/LAN target, no known-hosts store in v1 — accept any host key. Revisit
    // (TOFU + pinned fingerprint in the profile) before this is ever exposed off-LAN.
    async fn check_server_key(
        &mut self,
        _server_public_key: &russh::keys::ssh_key::PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }
}

/// A live SSH connection. Exposes only transport-level primitives — no knowledge of
/// systemd/docker/world paths belongs here.
pub struct SshSession {
    handle: Arc<Handle<Client>>,
    pub host: String,
    pub port: u16,
    pub user: String,
}

impl SshSession {
    pub async fn connect_password(
        host: &str,
        port: u16,
        user: &str,
        password: &str,
    ) -> Result<Self> {
        let config = Arc::new(client::Config::default());
        let mut handle = client::connect(config, (host, port), Client)
            .await
            .with_context(|| format!("connecting to {host}:{port}"))?;

        let auth = handle
            .authenticate_password(user, password)
            .await
            .context("password authentication")?;
        if !auth.success() {
            bail!("authentication rejected for {user}@{host}");
        }

        Ok(Self {
            handle: Arc::new(handle),
            host: host.to_string(),
            port,
            user: user.to_string(),
        })
    }

    pub async fn exec(&self, cmd: &str) -> Result<ExecOutput> {
        let mut channel = self
            .handle
            .channel_open_session()
            .await
            .context("opening SSH channel")?;
        channel
            .exec(true, cmd)
            .await
            .with_context(|| format!("exec: {cmd}"))?;

        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let mut exit_status = 0u32;

        while let Some(msg) = channel.wait().await {
            match msg {
                ChannelMsg::Data { data } => stdout.extend_from_slice(&data),
                ChannelMsg::ExtendedData { data, .. } => stderr.extend_from_slice(&data),
                ChannelMsg::ExitStatus { exit_status: code } => exit_status = code,
                _ => {}
            }
        }

        Ok(ExecOutput {
            stdout: String::from_utf8_lossy(&stdout).into_owned(),
            stderr: String::from_utf8_lossy(&stderr).into_owned(),
            exit_status,
        })
    }

    async fn open_sftp(&self) -> Result<SftpSession> {
        let channel = self
            .handle
            .channel_open_session()
            .await
            .context("opening SSH channel for sftp")?;
        channel
            .request_subsystem(true, "sftp")
            .await
            .context("requesting sftp subsystem")?;
        SftpSession::new(channel.into_stream())
            .await
            .context("starting sftp session")
    }

    pub async fn push_file(&self, local: &Path, remote: &str) -> Result<u64> {
        let data = tokio::fs::read(local)
            .await
            .with_context(|| format!("reading local file {}", local.display()))?;
        let sftp = self.open_sftp().await?;
        let mut file = sftp
            .open_with_flags(
                remote,
                OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE,
            )
            .await
            .with_context(|| format!("opening remote file {remote} for write"))?;
        file.write_all(&data).await.context("writing remote file")?;
        file.flush().await.context("flushing remote file")?;
        file.shutdown().await.ok();
        Ok(data.len() as u64)
    }

    pub async fn pull_file(&self, remote: &str, local: &Path) -> Result<u64> {
        let sftp = self.open_sftp().await?;
        let mut file = sftp
            .open_with_flags(remote, OpenFlags::READ)
            .await
            .with_context(|| format!("opening remote file {remote} for read"))?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)
            .await
            .context("reading remote file")?;
        if let Some(parent) = local.parent() {
            tokio::fs::create_dir_all(parent).await.ok();
        }
        tokio::fs::write(local, &buf)
            .await
            .with_context(|| format!("writing local file {}", local.display()))?;
        Ok(buf.len() as u64)
    }
}

/// The SSH-transport `ServerTarget`. All ServiceKind-specific behaviour (how to
/// stop/start, where perms need fixing) branches inside this impl only — callers
/// (state machine, discovery confirmation) only ever see the trait.
pub struct SshTarget {
    session: SshSession,
    service_kind: ServiceKind,
    worlds_path: String,
    steam_user: Option<String>,
}

impl SshTarget {
    pub fn new(
        session: SshSession,
        service_kind: ServiceKind,
        worlds_path: String,
        steam_user: Option<String>,
    ) -> Self {
        Self {
            session,
            service_kind,
            worlds_path,
            steam_user,
        }
    }

    /// Escape hatch back to the raw session, e.g. for re-running discovery.
    pub fn session(&self) -> &SshSession {
        &self.session
    }

    /// Reclaim the underlying session, e.g. when a re-scan changes the ServiceKind and
    /// the target needs rebuilding.
    pub fn into_session(self) -> SshSession {
        self.session
    }
}

#[async_trait]
impl ServerTarget for SshTarget {
    async fn exec(&self, cmd: &str) -> Result<ExecOutput> {
        self.session.exec(cmd).await
    }

    async fn push_file(&self, local: &Path, remote: &str) -> Result<u64> {
        self.session.push_file(local, remote).await
    }

    async fn pull_file(&self, remote: &str, local: &Path) -> Result<u64> {
        self.session.pull_file(remote, local).await
    }

    async fn stat(&self, dir: &str, glob: &str) -> Result<Vec<FileEntry>> {
        // Shell-side glob via `find`, not sftp readdir + a hand-rolled matcher — every
        // other target-specific command here is already a shell one-liner.
        let cmd = format!(
            "find {} -maxdepth 1 -name {} -printf '%s %T@ %p\\n' 2>/dev/null",
            shq(dir),
            shq(glob)
        );
        let out = self.session.exec(&cmd).await?;
        let mut entries = Vec::new();
        for line in out.stdout.lines() {
            let mut parts = line.splitn(3, ' ');
            let (Some(size), Some(mtime), Some(path)) =
                (parts.next(), parts.next(), parts.next())
            else {
                continue;
            };
            entries.push(FileEntry {
                path: path.to_string(),
                size: size.parse().unwrap_or(0),
                modified: mtime.parse::<f64>().ok().map(|f| f as i64),
            });
        }
        Ok(entries)
    }

    async fn logs_tail(&self, lines: u32) -> Result<String> {
        let cmd = match &self.service_kind {
            ServiceKind::Systemd { unit } => {
                format!("journalctl -u {} -n {lines} --no-pager", shq(unit))
            }
            ServiceKind::Docker { container } => {
                format!("docker logs --tail {lines} {}", shq(container))
            }
            ServiceKind::Process { log_path, .. } => match log_path {
                Some(p) => format!("tail -n {lines} {}", shq(p)),
                None => bail!("this target has no log_path to tail"),
            },
        };
        Ok(self.session.exec(&cmd).await?.stdout)
    }

    async fn service_control(&self, action: ServiceAction) -> Result<ExecOutput> {
        let cmd = match (&self.service_kind, action) {
            (ServiceKind::Systemd { unit }, ServiceAction::Status) => {
                format!("systemctl is-active {}", shq(unit))
            }
            (ServiceKind::Systemd { unit }, ServiceAction::Stop) => {
                format!("systemctl stop {}", shq(unit))
            }
            (ServiceKind::Systemd { unit }, ServiceAction::Start) => {
                format!("systemctl start {}", shq(unit))
            }
            (ServiceKind::Docker { container }, ServiceAction::Status) => {
                format!("docker inspect -f {{{{.State.Status}}}} {}", shq(container))
            }
            (ServiceKind::Docker { container }, ServiceAction::Stop) => {
                format!("docker stop {}", shq(container))
            }
            (ServiceKind::Docker { container }, ServiceAction::Start) => {
                format!("docker start {}", shq(container))
            }
            (ServiceKind::Process { start_cmd, .. }, ServiceAction::Status) => {
                format!("pgrep -af {}", shq(start_cmd))
            }
            (ServiceKind::Process { stop_cmd, .. }, ServiceAction::Stop) => stop_cmd.clone(),
            (ServiceKind::Process { start_cmd, .. }, ServiceAction::Start) => start_cmd.clone(),
        };
        self.session.exec(&cmd).await
    }

    async fn active_since(&self) -> Result<Option<String>> {
        let cmd = match &self.service_kind {
            ServiceKind::Systemd { unit } => {
                format!("systemctl show -p ActiveEnterTimestamp --value {}", shq(unit))
            }
            ServiceKind::Docker { container } => {
                format!("docker inspect -f {{{{.State.StartedAt}}}} {}", shq(container))
            }
            ServiceKind::Process { .. } => return Ok(None),
        };
        let out = self.session.exec(&cmd).await?;
        let value = out.stdout.trim();
        Ok(if value.is_empty() { None } else { Some(value.to_string()) })
    }

    async fn fix_perms(&self, path: &str) -> Result<()> {
        match (&self.service_kind, &self.steam_user) {
            (ServiceKind::Systemd { .. }, Some(user)) => {
                let cmd = format!("chown -R {}:{} {}", shq(user), shq(user), shq(path));
                let out = self.session.exec(&cmd).await?;
                if !out.success() {
                    bail!("chown failed: {}", out.stderr.trim());
                }
                Ok(())
            }
            // Docker/process targets: no-op or best-effort per SPEC §4 — the container's
            // own UID mapping usually already owns these files.
            _ => Ok(()),
        }
    }

    fn worlds_path(&self) -> &str {
        &self.worlds_path
    }

    fn service_kind(&self) -> &ServiceKind {
        &self.service_kind
    }
}
