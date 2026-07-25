//! Interactive terminal (SPEC.md §16) — a persistent PTY session over the SSH
//! connection. Architecturally distinct from everything else in this app: every other
//! SSH interaction is request/response (`exec` sends one command, waits for exit,
//! returns output); this is a standing bidirectional stream.
//!
//! The channel splits into independent read/write halves, so a background task can
//! stream output as Tauri events while writes/resizes go straight to the write half —
//! both concurrent with, and independent of, the app's normal `exec()` traffic (stats
//! polling, etc.) over the same authenticated connection.
//!
//! Security note: this opens a real shell as whatever user the profile connects as
//! (`root` on the reference LXC) — the same trust level as `ssh root@host` today, not a
//! new privilege, but it is a real, unsandboxed shell, not a toy.

use anyhow::Context;
use russh::client::Msg;
use russh::{ChannelMsg, ChannelWriteHalf};
use tauri::{AppHandle, Emitter};
use tokio::task::JoinHandle;

use crate::ssh::SshSession;

pub struct TerminalSession {
    write: ChannelWriteHalf<Msg>,
    reader_task: JoinHandle<()>,
}

impl TerminalSession {
    pub async fn open(app: AppHandle, session: &SshSession) -> anyhow::Result<Self> {
        let channel = session.open_channel().await?;
        channel
            .request_pty(true, "xterm-256color", 80, 24, 0, 0, &[])
            .await
            .context("requesting pty")?;
        channel.request_shell(true).await.context("requesting shell")?;

        let (mut read, write) = channel.split();

        let reader_task = tokio::spawn(async move {
            loop {
                match read.wait().await {
                    Some(ChannelMsg::Data { data }) | Some(ChannelMsg::ExtendedData { data, .. }) => {
                        let _ = app.emit("terminal-output", String::from_utf8_lossy(&data).into_owned());
                    }
                    Some(ChannelMsg::Close) | Some(ChannelMsg::Eof) | None => break,
                    Some(ChannelMsg::ExitStatus { .. }) => break,
                    _ => {}
                }
            }
            let _ = app.emit("terminal-closed", ());
        });

        Ok(Self { write, reader_task })
    }

    pub async fn write(&self, data: &str) -> anyhow::Result<()> {
        self.write
            .data_bytes(data.as_bytes().to_vec())
            .await
            .context("writing to terminal")
    }

    pub async fn resize(&self, cols: u32, rows: u32) -> anyhow::Result<()> {
        self.write
            .window_change(cols, rows, 0, 0)
            .await
            .context("resizing terminal")
    }

    /// Stops the background reader; the write half (and the channel machinery it holds)
    /// drops along with `self`, which tears down the remote side too.
    pub fn close(self) {
        self.reader_task.abort();
    }
}
