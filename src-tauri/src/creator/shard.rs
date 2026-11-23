//! Profiles in the launcher: open one with CDP (attach, restart or start) and stop it.

use std::time::{Duration, Instant};

use crate::process::{CdpInfo, Tracker};

/// How long a start with CDP may take to publish its endpoint (launch.rs `CDP_WAIT`).
const CDP_WAIT: Duration = Duration::from_secs(30);

/// What opening a profile with CDP takes, given what the launcher knows about it.
#[derive(Debug, Clone)]
pub enum OpenPlan {
    /// Running with CDP: reuse its endpoint.
    Attach(CdpInfo),
    /// Running, no CDP yet, but young enough that a start with CDP may still publish it.
    Wait,
    /// Running without CDP (started from the UI): stop it and start it again (D8).
    Restart,
    /// Not running.
    Start,
}

/// `running`: the tracked browser's CDP endpoint (if any) and its uptime; None when not running.
pub fn plan_open(running: Option<(Option<CdpInfo>, Duration)>) -> OpenPlan {
    match running {
        Some((Some(cdp), _)) => OpenPlan::Attach(cdp),
        Some((None, uptime)) if uptime < CDP_WAIT => OpenPlan::Wait,
        Some((None, _)) => OpenPlan::Restart,
        None => OpenPlan::Start,
    }
}

fn tracked(profile_id: &str) -> Option<(Option<CdpInfo>, Duration)> {
    Tracker::shared()
        .running()
        .into_iter()
        .find(|r| r.profile_id == profile_id)
        .map(|r| (r.cdp, Duration::from_millis(r.uptime_ms)))
}

/// The profile running with CDP on, started or restarted as needed. Every
/// step goes back through `plan_open`, so a start that loses a race with
/// another one attaches to it instead of launching twice.
pub async fn open_cdp(profile_id: &str) -> anyhow::Result<CdpInfo> {
    let deadline = Instant::now() + 2 * CDP_WAIT;
    let mut retried = false;
    loop {
        if Instant::now() >= deadline {
            anyhow::bail!("profile {profile_id} did not open with CDP in time");
        }
        match plan_open(tracked(profile_id)) {
            OpenPlan::Attach(cdp) => return Ok(cdp),
            OpenPlan::Wait => tokio::time::sleep(Duration::from_millis(250)).await,
            OpenPlan::Restart => stop(profile_id).await?,
            OpenPlan::Start => match start(profile_id).await {
                Ok(cdp) => return Ok(cdp),
                // The launcher still holds the profile (Python's BUSY): look again once, after 1 s.
                Err(e) if !retried && busy(&format!("{e:#}")) => {
                    retried = true;
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                Err(e) => return Err(e),
            },
        }
    }
}

/// Stop the profile and wait (at most 10 s) until the launcher sees it gone.
pub async fn stop(profile_id: &str) -> anyhow::Result<()> {
    let tracker = Tracker::shared();
    tracker.kill(profile_id).await?;
    let deadline = Instant::now() + Duration::from_secs(10);
    while tracker.is_running(profile_id) {
        if Instant::now() >= deadline {
            anyhow::bail!("profile {profile_id} did not stop within 10 s");
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    Ok(())
}

/// Start with CDP (the local API's /start path).
async fn start(profile_id: &str) -> anyhow::Result<CdpInfo> {
    let bus = crate::bus().await.map_err(anyhow::Error::msg)?;
    let outcome =
        crate::launch::launch_profile_unlocked(profile_id, true, false, bus.port, &bus.token, None)
            .await?;
    outcome.cdp.ok_or_else(|| {
        anyhow::anyhow!("no CDP endpoint: {}", outcome.cdp_error.unwrap_or_default())
    })
}

fn busy(error: &str) -> bool {
    error.contains("is already running") || error.contains("still opening or being saved")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cdp() -> CdpInfo {
        CdpInfo {
            port: 9222,
            http_url: "http://127.0.0.1:9222".into(),
            web_socket_debugger_url: "ws://127.0.0.1:9222/devtools/browser/x".into(),
        }
    }

    const OLD: Duration = Duration::from_secs(60);

    #[test]
    fn plan_open_attaches_when_cdp_known() {
        assert!(
            matches!(plan_open(Some((Some(cdp()), OLD))), OpenPlan::Attach(c) if c.port == 9222)
        );
    }

    #[test]
    fn plan_open_restarts_a_ui_started_profile() {
        assert!(matches!(plan_open(Some((None, OLD))), OpenPlan::Restart));
    }

    #[test]
    fn plan_open_waits_while_a_cdp_start_may_still_publish() {
        assert!(matches!(
            plan_open(Some((None, Duration::from_secs(2)))),
            OpenPlan::Wait
        ));
    }

    #[test]
    fn plan_open_starts_when_stopped() {
        assert!(matches!(plan_open(None), OpenPlan::Start));
    }

    #[test]
    fn busy_matches_the_launchers_wording() {
        assert!(busy("profile abc is already running"));
        assert!(busy("the profile is still opening or being saved"));
        assert!(!busy("no CDP endpoint"));
    }
}
