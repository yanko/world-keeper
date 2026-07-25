//! Read-only stats dashboard (SPEC.md §10) — cheap, safe, no-write SSH one-liners run
//! entirely through `ServerTarget`, so this module knows nothing about systemd vs
//! docker vs process. The Steam build-id badge is an explicit stretch goal in the spec
//! and isn't implemented.

use serde::Serialize;

use crate::ssh::shq;
use crate::target::{ServerTarget, ServiceAction};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStats {
    pub service_status: String,
    pub active_since: Option<String>,
    /// Raw `ss -ulnp` lines that mention corekeeper (best-effort; empty if none found).
    pub udp_listening: Vec<String>,
    pub mem_total_kb: Option<u64>,
    pub mem_available_kb: Option<u64>,
    pub disk_size: Option<String>,
    pub disk_used: Option<String>,
    pub disk_avail: Option<String>,
    pub disk_use_pct: Option<String>,
    pub uptime_seconds: Option<u64>,
    pub recent_log_tail: String,
}

pub async fn collect(target: &dyn ServerTarget) -> anyhow::Result<ServerStats> {
    let service_status = target
        .service_control(ServiceAction::Status)
        .await?
        .stdout
        .trim()
        .to_string();
    let active_since = target.active_since().await.unwrap_or(None);

    let udp_listening = udp_listening(target).await;

    let (mem_total_kb, mem_available_kb) = match target.exec("cat /proc/meminfo 2>/dev/null").await
    {
        Ok(out) => parse_meminfo(&out.stdout),
        Err(_) => (None, None),
    };

    let (disk_size, disk_used, disk_avail, disk_use_pct) = match target
        .exec(&format!(
            "df -h --output=size,used,avail,pcent {} 2>/dev/null | tail -n 1",
            shq(target.worlds_path())
        ))
        .await
    {
        Ok(out) => parse_df(out.stdout.trim()),
        Err(_) => (None, None, None, None),
    };

    let uptime_seconds = match target.exec("cat /proc/uptime 2>/dev/null").await {
        Ok(out) => out
            .stdout
            .split_whitespace()
            .next()
            .and_then(|s| s.parse::<f64>().ok())
            .map(|f| f as u64),
        Err(_) => None,
    };

    let recent_log_tail = target.logs_tail(80).await.unwrap_or_default();

    Ok(ServerStats {
        service_status,
        active_since,
        udp_listening,
        mem_total_kb,
        mem_available_kb,
        disk_size,
        disk_used,
        disk_avail,
        disk_use_pct,
        uptime_seconds,
        recent_log_tail,
    })
}

/// Raw `ss -ulnp` lines mentioning corekeeper (best-effort). Shared with the push
/// state machine's Verifying step, which needs the same "is the port up" check.
pub async fn udp_listening(target: &dyn ServerTarget) -> Vec<String> {
    target.udp_listening().await.unwrap_or_default()
}

fn parse_meminfo(text: &str) -> (Option<u64>, Option<u64>) {
    let mut total = None;
    let mut available = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            total = rest.split_whitespace().next().and_then(|v| v.parse().ok());
        } else if let Some(rest) = line.strip_prefix("MemAvailable:") {
            available = rest.split_whitespace().next().and_then(|v| v.parse().ok());
        }
    }
    (total, available)
}

type DfFields = (Option<String>, Option<String>, Option<String>, Option<String>);

fn parse_df(line: &str) -> DfFields {
    let mut parts = line.split_whitespace();
    (
        parts.next().map(str::to_string),
        parts.next().map(str::to_string),
        parts.next().map(str::to_string),
        parts.next().map(str::to_string),
    )
}
