//! Stop/start-and-wait polling shared by the push and pull state machines
//! (`push.rs`/`pull.rs`). Both need identical "did it actually settle" semantics —
//! target-agnostic, since it's written purely against `ServerTarget::status`.

use std::time::{Duration, Instant};

use anyhow::bail;

use crate::target::{RunState, ServerTarget, ServiceAction};

pub const STOP_START_TIMEOUT: Duration = Duration::from_secs(30);
pub const POLL_INTERVAL: Duration = Duration::from_millis(1500);

pub async fn stop_and_wait(target: &dyn ServerTarget) -> anyhow::Result<()> {
    target.service_control(ServiceAction::Stop).await?;
    wait_for_run_state(target, RunState::Stopped, STOP_START_TIMEOUT).await
}

pub async fn start_and_wait(target: &dyn ServerTarget) -> anyhow::Result<()> {
    target.service_control(ServiceAction::Start).await?;
    wait_for_run_state(target, RunState::Running, STOP_START_TIMEOUT).await
}

pub async fn wait_for_run_state(
    target: &dyn ServerTarget,
    want: RunState,
    timeout: Duration,
) -> anyhow::Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        let state = target.status().await.unwrap_or(RunState::Unknown);
        if state == want {
            return Ok(());
        }
        if state == RunState::Failed && want == RunState::Running {
            let tail = target.logs_tail(50).await.unwrap_or_default();
            bail!("service entered a failed state; recent log:\n{tail}");
        }
        if Instant::now() >= deadline {
            let tail = target.logs_tail(50).await.unwrap_or_default();
            bail!("timed out waiting for service to reach {want:?} (still {state:?}); recent log:\n{tail}");
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}
