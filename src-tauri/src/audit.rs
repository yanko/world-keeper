//! SQLite audit log (SPEC.md §9) — a forensic trail of every session, connection,
//! state transition, file op, and backup. Secrets are never logged, only *which*
//! key/user was used.
//!
//! Uses `sqlx` directly against its own connection pool rather than going through
//! `tauri-plugin-sql`'s IPC-oriented commands: writes here originate from Rust-side
//! backend logic (SSH connects, the future state machine), not from the frontend, and
//! `tauri-plugin-sql`'s `execute`/`select` are only reachable via its `invoke` layer.
//! The plugin stays registered so the frontend can still run ad-hoc investigative
//! queries ("what happened to world 0 on the 24th") straight from the store.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqliteRow};
use sqlx::{QueryBuilder, Row, Sqlite};
use tauri::{AppHandle, Manager, Runtime};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
  id INTEGER PRIMARY KEY,
  started_at TEXT NOT NULL,
  app_version TEXT,
  profile_id TEXT,
  host TEXT,
  ssh_user TEXT,
  auth_method TEXT
);

CREATE TABLE IF NOT EXISTS events (
  id INTEGER PRIMARY KEY,
  session_id INTEGER REFERENCES sessions(id),
  ts TEXT NOT NULL,
  kind TEXT NOT NULL,
  state TEXT,
  detail_json TEXT,
  ok INTEGER
);

CREATE TABLE IF NOT EXISTS file_ops (
  id INTEGER PRIMARY KEY,
  event_id INTEGER REFERENCES events(id),
  op TEXT,
  src TEXT,
  dst TEXT,
  bytes INTEGER,
  slot INTEGER
);

CREATE TABLE IF NOT EXISTS backups (
  id INTEGER PRIMARY KEY,
  session_id INTEGER REFERENCES sessions(id),
  ts TEXT NOT NULL,
  slot INTEGER,
  lxc_path TEXT,
  bytes INTEGER,
  game_id_before TEXT,
  note TEXT
);

CREATE TABLE IF NOT EXISTS ssh_commands (
  id INTEGER PRIMARY KEY,
  session_id INTEGER REFERENCES sessions(id),
  ts TEXT NOT NULL,
  command TEXT NOT NULL,
  exit_code INTEGER,
  stdout TEXT,
  stderr TEXT,
  duration_ms INTEGER
);

CREATE TABLE IF NOT EXISTS errors (
  id INTEGER PRIMARY KEY,
  session_id INTEGER REFERENCES sessions(id),
  ts TEXT NOT NULL,
  context TEXT NOT NULL,
  message TEXT NOT NULL,
  detail_json TEXT
);
"#;

/// Stdout/stderr captured verbatim from `ssh_commands` — a `journalctl -n 200` or
/// `docker logs` dump would otherwise bloat the db unboundedly on every 1.5s poll.
const CAPTURE_TRUNCATE_LEN: usize = 4000;

fn truncate_capture(s: &str) -> String {
    if s.len() <= CAPTURE_TRUNCATE_LEN {
        return s.to_string();
    }
    // Back off to the nearest char boundary — `s` may not be ASCII-only.
    let mut end = CAPTURE_TRUNCATE_LEN;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}… [truncated, {} bytes total]", &s[..end], s.len())
}

#[derive(Clone)]
pub struct AuditDb {
    pool: SqlitePool,
    db_path: std::path::PathBuf,
}

impl AuditDb {
    pub async fn init<R: Runtime>(app: &AppHandle<R>) -> anyhow::Result<Self> {
        let dir = app
            .path()
            .app_data_dir()
            .map_err(|e| anyhow::anyhow!("resolving app data dir: {e}"))?;
        std::fs::create_dir_all(&dir)?;
        let db_path = dir.join("ckmanager.db");

        let options = SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(options).await?;
        sqlx::query(SCHEMA).execute(&pool).await?;

        Ok(Self { pool, db_path })
    }

    pub async fn start_session(&self, app_version: &str) -> anyhow::Result<i64> {
        let now = now_iso();
        let row = sqlx::query(
            "INSERT INTO sessions (started_at, app_version) VALUES (?1, ?2) RETURNING id",
        )
        .bind(&now)
        .bind(app_version)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.try_get("id")?)
    }

    pub async fn record_connect(
        &self,
        session_id: i64,
        profile_id: &str,
        host: &str,
        ssh_user: &str,
        auth_method: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE sessions SET profile_id = ?1, host = ?2, ssh_user = ?3, auth_method = ?4 \
             WHERE id = ?5",
        )
        .bind(profile_id)
        .bind(host)
        .bind(ssh_user)
        .bind(auth_method)
        .bind(session_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn log_event(
        &self,
        session_id: i64,
        kind: &str,
        state: Option<&str>,
        detail: Option<&JsonValue>,
        ok: Option<bool>,
    ) -> anyhow::Result<i64> {
        let now = now_iso();
        let detail_json = detail.map(|d| d.to_string());
        let row = sqlx::query(
            "INSERT INTO events (session_id, ts, kind, state, detail_json, ok) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6) RETURNING id",
        )
        .bind(session_id)
        .bind(&now)
        .bind(kind)
        .bind(state)
        .bind(detail_json)
        .bind(ok.map(|b| b as i64))
        .fetch_one(&self.pool)
        .await?;
        Ok(row.try_get("id")?)
    }

    // Not called yet — these mirror the `file_ops`/`backups` tables ahead of the push/pull
    // state machine (SPEC §13 build order item 4), which is the first real caller.
    #[allow(dead_code)]
    pub async fn log_file_op(
        &self,
        event_id: i64,
        op: &str,
        src: Option<&str>,
        dst: Option<&str>,
        bytes: Option<i64>,
        slot: Option<i64>,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO file_ops (event_id, op, src, dst, bytes, slot) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .bind(event_id)
        .bind(op)
        .bind(src)
        .bind(dst)
        .bind(bytes)
        .bind(slot)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn log_backup(
        &self,
        session_id: i64,
        slot: i64,
        lxc_path: &str,
        bytes: i64,
        game_id_before: Option<&str>,
        note: Option<&str>,
    ) -> anyhow::Result<()> {
        let now = now_iso();
        sqlx::query(
            "INSERT INTO backups (session_id, ts, slot, lxc_path, bytes, game_id_before, note) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind(session_id)
        .bind(&now)
        .bind(slot)
        .bind(lxc_path)
        .bind(bytes)
        .bind(game_id_before)
        .bind(note)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Records one remote command execution — every `SshSession::exec` call, success or
    /// failure, fires this (see `ssh.rs`). This is the trail that answers "what exactly
    /// did the app run on the server, and what did it get back" without needing to ask
    /// the user to reproduce it manually.
    pub async fn log_ssh_command(
        &self,
        session_id: i64,
        command: &str,
        exit_code: Option<i64>,
        stdout: &str,
        stderr: &str,
        duration_ms: i64,
    ) -> anyhow::Result<()> {
        let now = now_iso();
        sqlx::query(
            "INSERT INTO ssh_commands (session_id, ts, command, exit_code, stdout, stderr, duration_ms) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        )
        .bind(session_id)
        .bind(&now)
        .bind(command)
        .bind(exit_code)
        .bind(truncate_capture(stdout))
        .bind(truncate_capture(stderr))
        .bind(duration_ms)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Records an operation-level failure (connect, discovery, push/pull/restore, key
    /// setup, terminal) — `context` names which operation, `message` is the
    /// `anyhow::Error`'s `{:#}` rendering. Kept separate from the general `events` table
    /// (which already gets an `"error"` kind entry at the same call sites) so failures
    /// are their own scannable, filterable list rather than mixed in with every state
    /// transition.
    pub async fn log_error(
        &self,
        session_id: i64,
        context: &str,
        message: &str,
        detail: Option<&JsonValue>,
    ) -> anyhow::Result<()> {
        let now = now_iso();
        let detail_json = detail.map(|d| d.to_string());
        sqlx::query(
            "INSERT INTO errors (session_id, ts, context, message, detail_json) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .bind(session_id)
        .bind(&now)
        .bind(context)
        .bind(message)
        .bind(detail_json)
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

// ---- read side (SPEC §16 Audit tab) --------------------------------------------
//
// Everything above this point has been writing to these tables since the very first
// build phase; nothing has read them back until now. Kept as typed Rust queries
// (`sqlx::QueryBuilder`, parameterized) rather than handing raw SQL to the frontend via
// `tauri-plugin-sql`'s generic `select()` — consistent with "all real logic in Rust."

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventRow {
    pub id: i64,
    pub session_id: Option<i64>,
    pub ts: String,
    pub kind: String,
    pub state: Option<String>,
    pub detail_json: Option<JsonValue>,
    pub ok: Option<bool>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EventFilter {
    pub kind: Option<String>,
    pub session_id: Option<i64>,
    pub ok: Option<bool>,
    /// Free-text search over `kind`, `state`, and `detail_json`.
    pub search: Option<String>,
    /// Inclusive ISO8601 bounds on `ts`.
    pub since: Option<String>,
    pub until: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionRow {
    pub id: i64,
    pub started_at: String,
    pub app_version: Option<String>,
    pub profile_id: Option<String>,
    pub host: Option<String>,
    pub ssh_user: Option<String>,
    pub auth_method: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandRow {
    pub id: i64,
    pub session_id: Option<i64>,
    pub ts: String,
    pub command: String,
    pub exit_code: Option<i64>,
    pub stdout: Option<String>,
    pub stderr: Option<String>,
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommandFilter {
    pub session_id: Option<i64>,
    /// `true` = only nonzero exits, `false` = only exit 0.
    pub failed_only: Option<bool>,
    /// Free-text search over `command`, `stdout`, and `stderr`.
    pub search: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorRow {
    pub id: i64,
    pub session_id: Option<i64>,
    pub ts: String,
    pub context: String,
    pub message: String,
    pub detail_json: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorFilter {
    pub session_id: Option<i64>,
    pub context: Option<String>,
    /// Free-text search over `context`, `message`, and `detail_json`.
    pub search: Option<String>,
    pub since: Option<String>,
    pub until: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupRow {
    pub id: i64,
    pub session_id: Option<i64>,
    pub ts: String,
    pub slot: Option<i64>,
    pub lxc_path: Option<String>,
    pub bytes: Option<i64>,
    pub game_id_before: Option<String>,
    pub note: Option<String>,
}

impl AuditDb {
    pub async fn query_events(&self, filter: &EventFilter) -> anyhow::Result<Vec<EventRow>> {
        let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
            "SELECT id, session_id, ts, kind, state, detail_json, ok FROM events WHERE 1=1",
        );

        if let Some(kind) = &filter.kind {
            qb.push(" AND kind = ").push_bind(kind.clone());
        }
        if let Some(session_id) = filter.session_id {
            qb.push(" AND session_id = ").push_bind(session_id);
        }
        if let Some(ok) = filter.ok {
            qb.push(" AND ok = ").push_bind(ok as i64);
        }
        if let Some(since) = &filter.since {
            qb.push(" AND ts >= ").push_bind(since.clone());
        }
        if let Some(until) = &filter.until {
            qb.push(" AND ts <= ").push_bind(until.clone());
        }
        if let Some(search) = filter.search.as_deref().filter(|s| !s.is_empty()) {
            let pattern = format!("%{search}%");
            qb.push(" AND (kind LIKE ")
                .push_bind(pattern.clone())
                .push(" OR state LIKE ")
                .push_bind(pattern.clone())
                .push(" OR detail_json LIKE ")
                .push_bind(pattern)
                .push(")");
        }

        qb.push(" ORDER BY id DESC LIMIT ")
            .push_bind(filter.limit.unwrap_or(200).clamp(1, 1000));
        qb.push(" OFFSET ").push_bind(filter.offset.unwrap_or(0).max(0));

        let rows = qb.build().fetch_all(&self.pool).await?;
        rows.into_iter().map(row_to_event).collect()
    }

    pub async fn list_event_kinds(&self) -> anyhow::Result<Vec<String>> {
        let rows = sqlx::query("SELECT DISTINCT kind FROM events ORDER BY kind")
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(|r| Ok(r.try_get::<String, _>("kind")?)).collect()
    }

    pub async fn list_sessions(&self) -> anyhow::Result<Vec<SessionRow>> {
        let rows = sqlx::query(
            "SELECT id, started_at, app_version, profile_id, host, ssh_user, auth_method \
             FROM sessions ORDER BY id DESC LIMIT 100",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(SessionRow {
                    id: row.try_get("id")?,
                    started_at: row.try_get("started_at")?,
                    app_version: row.try_get("app_version")?,
                    profile_id: row.try_get("profile_id")?,
                    host: row.try_get("host")?,
                    ssh_user: row.try_get("ssh_user")?,
                    auth_method: row.try_get("auth_method")?,
                })
            })
            .collect()
    }

    pub async fn query_ssh_commands(&self, filter: &CommandFilter) -> anyhow::Result<Vec<CommandRow>> {
        let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
            "SELECT id, session_id, ts, command, exit_code, stdout, stderr, duration_ms \
             FROM ssh_commands WHERE 1=1",
        );

        if let Some(session_id) = filter.session_id {
            qb.push(" AND session_id = ").push_bind(session_id);
        }
        if let Some(failed_only) = filter.failed_only {
            if failed_only {
                qb.push(" AND exit_code IS NOT NULL AND exit_code != 0");
            } else {
                qb.push(" AND (exit_code IS NULL OR exit_code = 0)");
            }
        }
        if let Some(since) = &filter.since {
            qb.push(" AND ts >= ").push_bind(since.clone());
        }
        if let Some(until) = &filter.until {
            qb.push(" AND ts <= ").push_bind(until.clone());
        }
        if let Some(search) = filter.search.as_deref().filter(|s| !s.is_empty()) {
            let pattern = format!("%{search}%");
            qb.push(" AND (command LIKE ")
                .push_bind(pattern.clone())
                .push(" OR stdout LIKE ")
                .push_bind(pattern.clone())
                .push(" OR stderr LIKE ")
                .push_bind(pattern)
                .push(")");
        }

        qb.push(" ORDER BY id DESC LIMIT ")
            .push_bind(filter.limit.unwrap_or(200).clamp(1, 1000));
        qb.push(" OFFSET ").push_bind(filter.offset.unwrap_or(0).max(0));

        let rows = qb.build().fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                Ok(CommandRow {
                    id: row.try_get("id")?,
                    session_id: row.try_get("session_id")?,
                    ts: row.try_get("ts")?,
                    command: row.try_get("command")?,
                    exit_code: row.try_get("exit_code")?,
                    stdout: row.try_get("stdout")?,
                    stderr: row.try_get("stderr")?,
                    duration_ms: row.try_get("duration_ms")?,
                })
            })
            .collect()
    }

    pub async fn query_errors(&self, filter: &ErrorFilter) -> anyhow::Result<Vec<ErrorRow>> {
        let mut qb: QueryBuilder<Sqlite> =
            QueryBuilder::new("SELECT id, session_id, ts, context, message, detail_json FROM errors WHERE 1=1");

        if let Some(session_id) = filter.session_id {
            qb.push(" AND session_id = ").push_bind(session_id);
        }
        if let Some(context) = &filter.context {
            qb.push(" AND context = ").push_bind(context.clone());
        }
        if let Some(since) = &filter.since {
            qb.push(" AND ts >= ").push_bind(since.clone());
        }
        if let Some(until) = &filter.until {
            qb.push(" AND ts <= ").push_bind(until.clone());
        }
        if let Some(search) = filter.search.as_deref().filter(|s| !s.is_empty()) {
            let pattern = format!("%{search}%");
            qb.push(" AND (context LIKE ")
                .push_bind(pattern.clone())
                .push(" OR message LIKE ")
                .push_bind(pattern.clone())
                .push(" OR detail_json LIKE ")
                .push_bind(pattern)
                .push(")");
        }

        qb.push(" ORDER BY id DESC LIMIT ")
            .push_bind(filter.limit.unwrap_or(200).clamp(1, 1000));
        qb.push(" OFFSET ").push_bind(filter.offset.unwrap_or(0).max(0));

        let rows = qb.build().fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                Ok(ErrorRow {
                    id: row.try_get("id")?,
                    session_id: row.try_get("session_id")?,
                    ts: row.try_get("ts")?,
                    context: row.try_get("context")?,
                    message: row.try_get("message")?,
                    detail_json: row.try_get("detail_json")?,
                })
            })
            .collect()
    }

    pub async fn list_error_contexts(&self) -> anyhow::Result<Vec<String>> {
        let rows = sqlx::query("SELECT DISTINCT context FROM errors ORDER BY context")
            .fetch_all(&self.pool)
            .await?;
        rows.iter().map(|r| Ok(r.try_get::<String, _>("context")?)).collect()
    }

    /// Every dated backup this app has created (SPEC §11 "World Library" screen),
    /// newest first — populated by `push::back_up`/`restore::back_up`'s pre-swap
    /// safety copy.
    /// Scoped to `host` when given — otherwise a backup made against one server (with
    /// a path that only exists on *that* host) can show up while connected to a
    /// completely different one, and "Restore" fails confusingly (or, worse, restores
    /// into the wrong path if one happens to exist).
    pub async fn list_backups(&self, host: Option<&str>) -> anyhow::Result<Vec<BackupRow>> {
        let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
            "SELECT b.id, b.session_id, b.ts, b.slot, b.lxc_path, b.bytes, b.game_id_before, b.note \
             FROM backups b",
        );
        if let Some(host) = host {
            qb.push(" JOIN sessions s ON s.id = b.session_id WHERE s.host = ")
                .push_bind(host.to_string());
        }
        qb.push(" ORDER BY b.id DESC LIMIT 200");

        let rows = qb.build().fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                Ok(BackupRow {
                    id: row.try_get("id")?,
                    session_id: row.try_get("session_id")?,
                    ts: row.try_get("ts")?,
                    slot: row.try_get("slot")?,
                    lxc_path: row.try_get("lxc_path")?,
                    bytes: row.try_get("bytes")?,
                    game_id_before: row.try_get("game_id_before")?,
                    note: row.try_get("note")?,
                })
            })
            .collect()
    }

    /// Every event, unfiltered, oldest first — the full export, not the paginated/
    /// filtered view the Audit tab queries day to day.
    pub async fn all_events(&self) -> anyhow::Result<Vec<EventRow>> {
        let rows = sqlx::query(
            "SELECT id, session_id, ts, kind, state, detail_json, ok FROM events ORDER BY id ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_event).collect()
    }

    /// Wipes the audit trail (SPEC §16 "Delete") except the row for `keep_session_id` —
    /// the session currently running, so `AppState::session_id` (a plain, immutable
    /// `i64`, not behind a lock) never ends up pointing at a session that no longer
    /// exists. Records its own `audit_cleared` event afterward: that the log was
    /// cleared is itself worth a trace.
    pub async fn clear_audit_log(&self, keep_session_id: i64) -> anyhow::Result<()> {
        sqlx::query("DELETE FROM file_ops").execute(&self.pool).await?;
        sqlx::query("DELETE FROM backups").execute(&self.pool).await?;
        sqlx::query("DELETE FROM ssh_commands").execute(&self.pool).await?;
        sqlx::query("DELETE FROM errors").execute(&self.pool).await?;
        sqlx::query("DELETE FROM events").execute(&self.pool).await?;
        sqlx::query("DELETE FROM sessions WHERE id != ?1")
            .bind(keep_session_id)
            .execute(&self.pool)
            .await?;
        self.log_event(keep_session_id, "audit_cleared", None, None, Some(true))
            .await?;
        Ok(())
    }

    /// Exports the full event history as a CSV plus a copy of the raw SQLite file,
    /// zipped together (SPEC §16) — human-readable and raw, in one file.
    pub async fn export_zip(&self, destination: &std::path::Path) -> anyhow::Result<()> {
        let events = self.all_events().await?;
        let commands = self
            .query_ssh_commands(&CommandFilter {
                limit: Some(1_000_000),
                ..Default::default()
            })
            .await?;
        let errors = self
            .query_errors(&ErrorFilter {
                limit: Some(1_000_000),
                ..Default::default()
            })
            .await?;
        let db_bytes = tokio::fs::read(&self.db_path)
            .await
            .with_context(|| format!("reading {}", self.db_path.display()))?;

        let destination = destination.to_path_buf();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            use std::io::Write;

            let mut events_csv = csv::Writer::from_writer(vec![]);
            events_csv.write_record(["id", "session_id", "ts", "kind", "state", "detail_json", "ok"])?;
            for e in &events {
                events_csv.write_record([
                    e.id.to_string(),
                    e.session_id.map(|v| v.to_string()).unwrap_or_default(),
                    e.ts.clone(),
                    e.kind.clone(),
                    e.state.clone().unwrap_or_default(),
                    e.detail_json.as_ref().map(|v| v.to_string()).unwrap_or_default(),
                    e.ok.map(|v| v.to_string()).unwrap_or_default(),
                ])?;
            }
            let events_bytes = events_csv.into_inner().context("finalizing events CSV")?;

            let mut commands_csv = csv::Writer::from_writer(vec![]);
            commands_csv.write_record([
                "id",
                "session_id",
                "ts",
                "command",
                "exit_code",
                "stdout",
                "stderr",
                "duration_ms",
            ])?;
            for c in &commands {
                commands_csv.write_record([
                    c.id.to_string(),
                    c.session_id.map(|v| v.to_string()).unwrap_or_default(),
                    c.ts.clone(),
                    c.command.clone(),
                    c.exit_code.map(|v| v.to_string()).unwrap_or_default(),
                    c.stdout.clone().unwrap_or_default(),
                    c.stderr.clone().unwrap_or_default(),
                    c.duration_ms.map(|v| v.to_string()).unwrap_or_default(),
                ])?;
            }
            let commands_bytes = commands_csv.into_inner().context("finalizing ssh_commands CSV")?;

            let mut errors_csv = csv::Writer::from_writer(vec![]);
            errors_csv.write_record(["id", "session_id", "ts", "context", "message", "detail_json"])?;
            for e in &errors {
                errors_csv.write_record([
                    e.id.to_string(),
                    e.session_id.map(|v| v.to_string()).unwrap_or_default(),
                    e.ts.clone(),
                    e.context.clone(),
                    e.message.clone(),
                    e.detail_json.clone().unwrap_or_default(),
                ])?;
            }
            let errors_bytes = errors_csv.into_inner().context("finalizing errors CSV")?;

            let file = std::fs::File::create(&destination)
                .with_context(|| format!("creating {}", destination.display()))?;
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default();

            zip.start_file("events.csv", options)?;
            zip.write_all(&events_bytes)?;

            zip.start_file("ssh_commands.csv", options)?;
            zip.write_all(&commands_bytes)?;

            zip.start_file("errors.csv", options)?;
            zip.write_all(&errors_bytes)?;

            zip.start_file("ckmanager.db", options)?;
            zip.write_all(&db_bytes)?;

            zip.finish()?;
            Ok(())
        })
        .await?
    }
}

fn row_to_event(row: SqliteRow) -> anyhow::Result<EventRow> {
    let detail_str: Option<String> = row.try_get("detail_json")?;
    let detail_json = detail_str.and_then(|s| serde_json::from_str(&s).ok());
    Ok(EventRow {
        id: row.try_get("id")?,
        session_id: row.try_get("session_id")?,
        ts: row.try_get("ts")?,
        kind: row.try_get("kind")?,
        state: row.try_get("state")?,
        detail_json,
        ok: row.try_get::<Option<i64>, _>("ok")?.map(|v| v != 0),
    })
}
