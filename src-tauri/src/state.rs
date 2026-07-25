//! Tauri-managed application state: the audit DB handle + the single active SSH
//! connection (SPEC §2: one profile connected at a time).

use tokio::sync::Mutex;

use crate::audit::AuditDb;
use crate::ssh::{SshSession, SshTarget};

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
}
