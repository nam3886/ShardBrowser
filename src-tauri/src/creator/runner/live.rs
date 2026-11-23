//! Live Breeze Blue accounts on the real site and the real Ele API, through the
//! app's own run path (`begin_job` + `make_breeze` on Shard's profiles), for
//! the M2 live gate. By hand, inside a sandboxed home, never on the owner's
//! data:
//!
//! ```text
//! CREATOR_TEST_BROWSER=<engine>                 # the ShardX engine to launch
//! CREATOR_LIVE_FINGERPRINTS=<dir>               # fingerprint library JSONs (copied in)
//! CREATOR_LIVE_IMPORT=<copy of ghidorah.sqlite> # proxies, blocked domains, Ele settings
//! CREATOR_LIVE_OUT=<dir>                        # our store (kept between runs) + screenshots
//! CREATOR_LIVE_COUNT=1 CREATOR_LIVE_PROXY_MODE=rotate
//! CREATOR_LIVE_MAILBOX=ele                      # or outlook: the store's free ones, oldest first
//! cargo test --lib creator::runner::live::live_breeze -- --ignored --nocapture
//! ```
//!
//! `live_outlook_test` (M3 live gate) pastes Outlook combo lines from a file
//! into the same store and tests each new mailbox once, 5 at a time, as the
//! Emails page's Test does:
//!
//! ```text
//! CREATOR_LIVE_OUT=<dir> CREATOR_LIVE_OUTLOOK=<file of combo lines>
//! cargo test --lib creator::runner::live::live_outlook_test -- --ignored --nocapture
//! ```
//!
//! Prints per account its status, error code, seconds and events (stored
//! scrubbed: no address, password, link or key).

use std::path::{Path, PathBuf};

use super::*;

fn env(k: &str) -> String {
    std::env::var(k).unwrap_or_else(|_| panic!("{k} not set"))
}

/// Seconds of day of a `now()` stamp (`...THH:MM:SS.ffffff+00:00`).
fn day_secs(t: &str) -> Option<f64> {
    let hms = t.split_once('T')?.1.get(..15)?;
    let mut p = hms.split(':');
    let (h, m, s): (f64, f64, f64) = (
        p.next()?.parse().ok()?,
        p.next()?.parse().ok()?,
        p.next()?.parse().ok()?,
    );
    Some(h * 3600.0 + m * 60.0 + s)
}

#[tokio::test]
#[ignore]
async fn live_breeze() {
    let _serial = crate::migrate::serial();
    let _root = crate::store::test_root("creator-live").with_config();
    let mut settings = crate::settings::load().unwrap();
    settings.browser_path = Some(env("CREATOR_TEST_BROWSER"));
    crate::settings::save(&settings).unwrap();
    let lib = crate::store::fingerprints_dir().unwrap();
    for e in std::fs::read_dir(env("CREATOR_LIVE_FINGERPRINTS")).unwrap() {
        let p = e.unwrap().path();
        if p.extension().is_some_and(|x| x == "json") {
            std::fs::copy(&p, lib.join(p.file_name().unwrap())).unwrap();
        }
    }

    let out = PathBuf::from(env("CREATOR_LIVE_OUT"));
    std::fs::create_dir_all(out.join("screens")).unwrap();
    let db = Arc::new(Db::open(&out.join("creator.sqlite")).unwrap());
    if db.proxies_list().unwrap().is_empty() {
        let n = store::import_basics(&db, Path::new(&env("CREATOR_LIVE_IMPORT"))).unwrap();
        eprintln!("imported {n:?}");
    }
    let count: u32 = std::env::var("CREATOR_LIVE_COUNT").map_or(1, |v| v.parse().unwrap());
    let mode = std::env::var("CREATOR_LIVE_PROXY_MODE").unwrap_or_else(|_| "rotate".into());
    let mailbox = std::env::var("CREATOR_LIVE_MAILBOX").unwrap_or_else(|_| "ele".into());
    let cx = Ctx {
        ele: match mailbox.as_str() {
            "ele" => Some(ele_client(&db).unwrap()),
            _ => None,
        },
        outlook: OutlookClient::new(db.clone(), OutlookUrls::live()).unwrap(),
        db: db.clone(),
        browsers: Shard,
        urls: Urls::live(),
        tm: Timings::default(),
        screens: out.join("screens"),
    };
    let run = RunState::default();
    let job = begin_job(&db, &run, count, &mode).unwrap();
    make_breeze(&cx, count, &mode, &mailbox, &run).await;
    run.end();

    let mut accts: Vec<_> = db
        .accounts_recent(KEY, 1000)
        .unwrap()
        .into_iter()
        .filter(|a| a.job_id == Some(job))
        .collect();
    accts.sort_by_key(|a| a.id);
    for a in &accts {
        let secs = match (
            a.started_at.as_deref().and_then(day_secs),
            a.finished_at.as_deref().and_then(day_secs),
        ) {
            (Some(s), Some(f)) => format!("{:.0}", (f - s).rem_euclid(86400.0)),
            _ => "?".into(),
        };
        eprintln!(
            "ACCOUNT {} status={} code={} secs={} error={}",
            a.id,
            a.status,
            a.error_code.as_deref().unwrap_or("-"),
            secs,
            a.error.as_deref().unwrap_or("-")
        );
        for e in db.events_of(a.id).unwrap() {
            eprintln!(
                "  {} {:5} {:12} {}",
                e.created_at.get(11..19).unwrap_or(""),
                e.level,
                e.step.as_deref().unwrap_or(""),
                e.message
            );
        }
    }
    eprintln!(
        "JOB {job}: {} accounts, {} linked",
        accts.len(),
        accts.iter().filter(|a| a.status == "linked").count()
    );
}
