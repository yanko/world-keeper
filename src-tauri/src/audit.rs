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
"#;

pub struct AuditDb {
    pool: SqlitePool,
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
            .filename(db_path)
            .create_if_missing(true);
        let pool = SqlitePool::connect_with(options).await?;
        sqlx::query(SCHEMA).execute(&pool).await?;

        Ok(Self { pool })
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
