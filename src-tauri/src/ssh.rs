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

use crate::audit::AuditDb;
use crate::target::{ExecOutput, FileEntry, RunState, ServerTarget, ServiceAction, ServiceKind};

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
///
/// `Clone` is cheap and intentional: `handle` is an `Arc`, so cloning shares the same
/// underlying multiplexed connection rather than opening a new one. This is what lets a
/// long-running push/pull/restore, the Terminal, and Stats polling all use their own
/// independent SSH channels concurrently instead of contending over one shared borrow
/// (see `commands.rs` — command handlers clone the target out of `state.connection`
/// under a brief lock, rather than holding that lock for the operation's whole
/// duration).
#[derive(Clone)]
pub struct SshSession {
    handle: Arc<Handle<Client>>,
    pub host: String,
    /// Set once, right after a successful connect (see `commands::connect`) — every
    /// `exec` from that point on is recorded to the `ssh_commands` audit table. `None`
    /// briefly between `connect_password`/`connect_key` returning and that attach call.
    audit: Option<(AuditDb, i64)>,
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
            audit: None,
        })
    }

    /// SPEC §6: key auth, primary once set up. `key_path` is an OpenSSH-format private
    /// key file — either one the user already had (`profile.keyPath`) or one this app
    /// generated itself (`keygen.rs`).
    pub async fn connect_key(host: &str, port: u16, user: &str, key_path: &Path) -> Result<Self> {
        let config = Arc::new(client::Config::default());
        let mut handle = client::connect(config, (host, port), Client)
            .await
            .with_context(|| format!("connecting to {host}:{port}"))?;

        let key_pair = russh::keys::load_secret_key(key_path, None)
            .with_context(|| format!("loading private key {}", key_path.display()))?;
        let hash_alg = handle.best_supported_rsa_hash().await?.flatten();
        let auth = handle
            .authenticate_publickey(
                user,
                russh::keys::PrivateKeyWithHashAlg::new(Arc::new(key_pair), hash_alg),
            )
            .await
            .context("public-key authentication")?;
        if !auth.success() {
            bail!("public-key authentication rejected for {user}@{host}");
        }

        Ok(Self {
            handle: Arc::new(handle),
            host: host.to_string(),
            audit: None,
        })
    }

    /// Appends a public key line to `~/.ssh/authorized_keys` on this session's host —
    /// the "one-time setup helper" from SPEC §6. Idempotent: safe to run again with the
    /// same key.
    pub async fn install_public_key(&self, public_key_line: &str) -> Result<()> {
        let line = public_key_line.trim();
        let cmd = format!(
            "mkdir -p ~/.ssh && chmod 700 ~/.ssh && touch ~/.ssh/authorized_keys && \
             chmod 600 ~/.ssh/authorized_keys && (grep -qF {} ~/.ssh/authorized_keys || echo {} >> ~/.ssh/authorized_keys)",
            shq(line),
            shq(line)
        );
        let out = self.exec(&cmd).await?;
        if !out.success() {
            bail!("installing public key failed: {}", out.stderr.trim());
        }
        Ok(())
    }

    /// A raw, unconfigured channel over this connection — used by `terminal.rs` for an
    /// interactive PTY session, which needs to drive `request_pty`/`request_shell`/
    /// `split` itself rather than the request/response shape `exec` provides.
    pub async fn open_channel(&self) -> Result<russh::Channel<client::Msg>> {
        self.handle
            .channel_open_session()
            .await
            .context("opening SSH channel")
    }

    /// Wires up the audit trail (`ssh_commands` table) for this session — called once,
    /// right after a successful connect (`commands::connect`), before the session is
    /// ever handed to discovery or a `ServiceKind`. Every `exec` from then on, whether
    /// called directly or through `SshTarget`'s internal `self.session.exec` calls, is
    /// logged — this is the single choke point every remote command passes through.
    pub fn attach_audit(&mut self, audit: AuditDb, session_id: i64) {
        self.audit = Some((audit, session_id));
    }

    pub async fn exec(&self, cmd: &str) -> Result<ExecOutput> {
        let started = std::time::Instant::now();
        let result = self.exec_inner(cmd).await;
        let elapsed_ms = started.elapsed().as_millis() as i64;

        if let Some((audit, session_id)) = &self.audit {
            match &result {
                Ok(out) => {
                    audit
                        .log_ssh_command(
                            *session_id,
                            cmd,
                            Some(out.exit_status as i64),
                            &out.stdout,
                            &out.stderr,
                            elapsed_ms,
                        )
                        .await
                        .ok();
                }
                Err(e) => {
                    audit
                        .log_ssh_command(*session_id, cmd, None, "", "", elapsed_ms)
                        .await
                        .ok();
                    audit
                        .log_error(*session_id, "ssh_exec", &format!("{e:#}"), None)
                        .await
                        .ok();
                }
            }
        }

        result
    }

    async fn exec_inner(&self, cmd: &str) -> Result<ExecOutput> {
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
#[derive(Clone)]
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

    async fn status(&self) -> Result<RunState> {
        match &self.service_kind {
            ServiceKind::Systemd { unit } => {
                let out = self.session.exec(&format!("systemctl is-active {}", shq(unit))).await?;
                Ok(match out.stdout.trim() {
                    "active" => RunState::Running,
                    "inactive" => RunState::Stopped,
                    "failed" => RunState::Failed,
                    _ => RunState::Unknown,
                })
            }
            ServiceKind::Docker { container } => {
                let out = self
                    .session
                    .exec(&format!(
                        "docker inspect -f {{{{.State.Status}}}} {}",
                        shq(container)
                    ))
                    .await?;
                Ok(match out.stdout.trim() {
                    "running" => RunState::Running,
                    "exited" | "created" | "dead" | "paused" => RunState::Stopped,
                    _ => RunState::Unknown,
                })
            }
            ServiceKind::Process { start_cmd, .. } => {
                let out = self.session.exec(&format!("pgrep -f {}", shq(start_cmd))).await?;
                Ok(if out.stdout.trim().is_empty() {
                    RunState::Stopped
                } else {
                    RunState::Running
                })
            }
        }
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

    async fn udp_listening(&self) -> Result<Vec<String>> {
        match &self.service_kind {
            ServiceKind::Systemd { .. } | ServiceKind::Process { .. } => {
                let out = self
                    .session
                    .exec("ss -ulnp 2>/dev/null | grep -i corekeeper")
                    .await?;
                Ok(out.stdout.lines().map(str::to_string).collect())
            }
            ServiceKind::Docker { container } => {
                // Try the cheap host-side check first: under `--network host` (common
                // for game servers, to dodge NAT overhead) the process runs directly in
                // the host's network namespace and shows up under its own name, exactly
                // like the systemd/process case — confirmed on a real deployment where
                // `ss` on the host showed `users:(("CoreKeeperServe",...))` directly.
                let host_out = self
                    .session
                    .exec("ss -ulnp 2>/dev/null | grep -i corekeeper")
                    .await?;
                if !host_out.stdout.trim().is_empty() {
                    return Ok(host_out.stdout.lines().map(str::to_string).collect());
                }

                // Bridge networking with published ports: the host only ever sees
                // `docker-proxy`, never a process literally named "corekeeper," so
                // resolve the container's published host port(s) and check those
                // directly instead.
                let ports_out = self
                    .session
                    .exec(&format!("docker port {} 2>/dev/null | grep -i udp", shq(container)))
                    .await?;
                let host_ports: Vec<String> = ports_out
                    .stdout
                    .lines()
                    .filter_map(|line| {
                        line.split("->")
                            .nth(1)?
                            .trim()
                            .rsplit(':')
                            .next()
                            .map(|p| p.trim().to_string())
                    })
                    .collect();

                if !host_ports.is_empty() {
                    let pattern = format!(":({})\\b", host_ports.join("|"));
                    let out = self
                        .session
                        .exec(&format!("ss -uln 2>/dev/null | grep -E {}", shq(&pattern)))
                        .await?;
                    return Ok(out.stdout.lines().map(str::to_string).collect());
                }

                let out = self
                    .session
                    .exec(&format!("docker exec {} ss -uln 2>/dev/null", shq(container)))
                    .await?;
                if out.success() {
                    return Ok(out
                        .stdout
                        .lines()
                        .filter(|l| !l.trim_start().starts_with("State"))
                        .map(str::to_string)
                        .collect());
                }
                // `ss` missing inside a minimal container image is common, and some
                // Docker/OCI runtime versions print the resulting "exec failed" error to
                // stdout despite the redirect above — `out.success()` above is what keeps
                // that error text from ever being treated as literal listener data.

                // Last resort: no host-side visibility (no --network host, no published
                // ports) and no `ss` in the image — read the UDP socket tables straight
                // out of /proc for the container's own network namespace. `cat`/`sh` are
                // close to universal even in minimal images, unlike `ss`. Checks both v4
                // and v6: .NET/Unity on Linux commonly binds a dual-stack IPv6 socket,
                // which would only ever show up in udp6. Can't match an exact port here —
                // confirmed against the reference LXC that Core Keeper binds an
                // OS-assigned ephemeral UDP port (56967), not the commonly-assumed 27015
                // — so "any bound UDP socket" is treated as good enough evidence for a
                // container that only ever runs this one game server process.
                let proc_out = self
                    .session
                    .exec(&format!(
                        "docker exec {} sh -c 'cat /proc/net/udp /proc/net/udp6 2>/dev/null'",
                        shq(container)
                    ))
                    .await?;
                if proc_out.success() {
                    let listeners: Vec<String> = proc_out
                        .stdout
                        .lines()
                        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with("sl"))
                        .map(str::to_string)
                        .collect();
                    if !listeners.is_empty() {
                        return Ok(listeners);
                    }
                }

                Ok(Vec::new())
            }
        }
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
