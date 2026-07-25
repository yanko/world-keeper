//! Tauri-managed application state: the audit DB handle + the single active SSH
//! connection (SPEC §2: one profile connected at a time).

use tokio::sync::Mutex;

use crate::audit::AuditDb;
use crate::ssh::{SshSession, SshTarget};
use crate::terminal::TerminalSession;

/// A connection moves from `Raw` (just authenticated, `ServiceKind` not yet known/
/// confirmed) to `Target` (confirmed, ready for the future push/pull state machine).
pub enum Connection {
    Raw {
        profile_id: String,
        session: SshSession,
    },
    Target {
        profile_id: String,
        target: SshTarget,
    },
}

impl Connection {
    pub fn profile_id(&self) -> &str {
        match self {
            Connection::Raw { profile_id, .. } => profile_id,
            Connection::Target { profile_id, .. } => profile_id,
        }
    }

    /// The raw SSH session, available regardless of whether discovery has been
    /// confirmed yet — used for (re-)running the discovery probe.
    pub fn session(&self) -> &SshSession {
        match self {
            Connection::Raw { session, .. } => session,
            Connection::Target { target, .. } => target.session(),
        }
    }
}

pub struct AppState {
    pub audit: AuditDb,
    pub session_id: i64,
    pub connection: Mutex<Option<Connection>>,
    /// One terminal session at a time (mirrors "one connection at a time," §2).
    pub terminal: Mutex<Option<TerminalSession>>,
}
