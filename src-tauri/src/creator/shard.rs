//! Profiles in the launcher: open one with CDP (attach, restart or start) and
//! stop it; open an account's browser for a run (`open_session`).

use std::time::{Duration, Instant};

use super::store::{Db, Proxy};
use super::web::{Page, Session, World};
use crate::process::{CdpInfo, Tracker};

/// Python `GEO_FAILED`: behind a proxy, the launcher fell back to UTC.
pub const GEO_FAILED: &str = "proxy location lookup failed";
/// Python `account_service.INTERRUPTED`: Shard closed the browser (Close all,
/// quit, a crash) or the person closed its window. Retryable.
pub const INTERRUPTED: &str = "Interrupted";

/// An account's browser, open for a run.
pub struct Opened {
    pub session: Session,
    pub page: Page,
    pub profile_id: String,
}

/// Open browser row `browser_id` for a run (Python `open_shard_profile`):
/// its profile (made on first use, `profile::ensure_profile`) is stopped if a
/// crash left it running, gets `proxy` inline (or none), starts with CDP and
/// gets one fresh tab. Behind a proxy, a page on `UTC` fails `GEO_FAILED`.
/// Any failure after the start stops the browser.
pub async fn open_session(
    db: &Db,
    browser_id: i64,
    proxy: Option<&Proxy>,
) -> anyhow::Result<Opened> {
    let profile_id = super::profile::ensure_profile(db, browser_id).await?;
    if Tracker::shared().is_running(&profile_id) {
        // A crash may have left it running; it must stop, or we would attach
        // to a browser still on its old proxy.
        stop(&profile_id).await?;
    }
    super::profile::set_proxy(&profile_id, proxy)?;
    let started = async {
        let session = Session::from_profile(&profile_id).await?;
        let page = session.page().await?;
        if proxy.is_some() && timezone(&page).await == "UTC" {
            anyhow::bail!(GEO_FAILED);
        }
        Ok((session, page))
    };
    match started.await {
        Ok((session, page)) => Ok(Opened {
            session,
            page,
            profile_id,
        }),
        Err(e) => {
            // Never leave a profile running that no session owns.
            let _ = stop(&profile_id).await;
            Err(e)
        }
    }
}

async fn timezone(page: &Page) -> String {
    page.eval::<String>(
        "() => Intl.DateTimeFormat().resolvedOptions().timeZone",
        World::Isolated,
    )
    .await
    .unwrap_or_default()
}

/// A run that did not link: `INTERRUPTED` when Shard no longer runs its
/// browser, else its own error (Python `account_service`: `not sess.is_open()`).
pub fn failure_text(profile_id: &str, error: String) -> String {
    if Tracker::shared().is_running(profile_id) {
        error
    } else {
        INTERRUPTED.to_string()
    }
}

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
    fn closed_by_shard_is_interrupted() {
        // Not tracked: Shard closed it (Close all, quit, a crash).
        let gone = format!("closed-{}", uuid::Uuid::new_v4().simple());
        assert_eq!(failure_text(&gone, "No API key shown".into()), INTERRUPTED);
    }

    /// Live: our profile starts on the engine at `CREATOR_TEST_BROWSER`, direct
    /// and, with `CREATOR_TEST_PROXY` (`host:port[:user:pass]`, never printed),
    /// behind that proxy, where the page must take the proxy's timezone.
    #[tokio::test]
    #[ignore]
    async fn open_session_with_proxy_reports_proxy_timezone() {
        let _serial = crate::migrate::serial();
        let _root = crate::creator::profile::tests::shard_root("creator-live");
        let engine = std::env::var("CREATOR_TEST_BROWSER").expect("CREATOR_TEST_BROWSER");
        let mut settings = crate::settings::load().unwrap();
        settings.browser_path = Some(engine);
        crate::settings::save(&settings).unwrap();
        let (db, _f) = crate::creator::store::temp_db();
        let mb = db.mailbox_create("ele", "u@a.com", "a.com", "t").unwrap();
        let br = db.browser_create(mb.id, None, "none").unwrap();

        let o = open_session(&db, br, None).await.expect("open direct");
        assert!(!timezone(&o.page).await.is_empty());
        assert!(crate::is_profile_running(&o.profile_id));
        let id = o.profile_id.clone();
        drop(o);
        stop(&id).await.unwrap();
        assert_eq!(failure_text(&id, "x".into()), INTERRUPTED);

        let Ok(line) = std::env::var("CREATOR_TEST_PROXY") else {
            eprintln!("CREATOR_TEST_PROXY not set: the proxy half did not run");
            return;
        };
        let p: Vec<&str> = line.trim().split(':').collect();
        let pid = db
            .proxy_add(
                p[0],
                p[1].parse().unwrap(),
                p.get(2).copied(),
                p.get(3).copied(),
                "static",
            )
            .unwrap();
        let proxy = db.proxy_get(pid).unwrap().unwrap();
        let o = open_session(&db, br, Some(&proxy))
            .await
            .map_err(|e| e.to_string());
        match o {
            Ok(o) => {
                let tz = timezone(&o.page).await;
                eprintln!("timezone behind the proxy: {tz}");
                assert!(!tz.is_empty() && tz != "UTC");
                let id = o.profile_id.clone();
                drop(o);
                stop(&id).await.unwrap();
            }
            // The one failure that still proves the check: Shard fell back to UTC.
            Err(e) => assert_eq!(e, GEO_FAILED),
        }
    }

    #[test]
    fn busy_matches_the_launchers_wording() {
        assert!(busy("profile abc is already running"));
        assert!(busy("the profile is still opening or being saved"));
        assert!(!busy("no CDP endpoint"));
    }
}
