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

use serde_json::Value as JsonValue;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool};
use sqlx::Row;
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
