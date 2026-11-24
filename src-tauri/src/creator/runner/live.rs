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
//! CREATOR_LIVE_MAILBOX_ID=<id>                  # or exactly this free Outlook mailbox (count 1)
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
//! With `CREATOR_LIVE_SAMPLE=N` and no `CREATOR_LIVE_OUTLOOK` it tests N random
//! active Outlook mailboxes of the store that have no account instead.
//!
//! M4's store checks (counts and timings only):
//!
//! ```text
//! CREATOR_LIVE_OUT=<dir>                                         live_perf (release build)
//! CREATOR_LIVE_OUT=<dir> CREATOR_LIVE_SHARD_PROFILES=<profiles>  live_profiles_check
//! CREATOR_TEST_BROWSER CREATOR_LIVE_FINGERPRINTS CREATOR_LIVE_OUT
//! CREATOR_LIVE_SHARD_COPY=<dir with profiles/ and user-data/> CREATOR_LIVE_BROWSER_ID=<id>
//!                                                                live_open_imported
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
    let only: Option<i64> = std::env::var("CREATOR_LIVE_MAILBOX_ID")
        .ok()
        .map(|v| v.parse().unwrap());
    if let Some(id) = only {
        // The first free one after id-1 is id itself only when id is free.
        if db.breeze_free_outlook(id - 1, 1).unwrap().first() != Some(&id) {
            panic!("mailbox {id} is not free for Breeze Blue");
        }
    }
    let count: u32 = match only {
        Some(_) => 1,
        None => std::env::var("CREATOR_LIVE_COUNT").map_or(1, |v| v.parse().unwrap()),
    };
    let mode = std::env::var("CREATOR_LIVE_PROXY_MODE").unwrap_or_else(|_| "rotate".into());
    let mailbox = match only {
        Some(_) => "outlook".into(),
        None => std::env::var("CREATOR_LIVE_MAILBOX").unwrap_or_else(|_| "ele".into()),
    };
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
    match only {
        Some(id) => {
            if new_account(&cx, job, &mode, Some(id)).await.is_none() {
                eprintln!("SKIPPED mailbox {id}: its inbox holds Breeze Blue mail");
            }
        }
        None => {
            make_breeze(&cx, count, &mode, &mailbox, &run).await;
        }
    }
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

/// `n` random active Outlook mailboxes of the store that have no account.
fn sample_outlook(db: &Db, n: usize) -> Vec<i64> {
    use rand::seq::SliceRandom;
    let q = store::EmailQuery {
        kind: Some("outlook".into()),
        status: Some("active".into()),
        ..Default::default()
    };
    let mut ids = db.email_ids(&q).unwrap();
    ids.shuffle(&mut rand::thread_rng());
    let ids: Vec<i64> = ids
        .into_iter()
        .filter(|id| {
            db.email_row(*id)
                .unwrap()
                .is_some_and(|r| r["accounts"].as_array().is_none_or(|a| a.is_empty()))
        })
        .take(n)
        .collect();
    eprintln!("SAMPLE {} mailboxes", ids.len());
    ids
}

/// Pastes `CREATOR_LIVE_OUTLOOK`'s combo lines; the new Outlook mailboxes.
fn paste_outlook(db: &Db) -> Vec<i64> {
    let text = std::fs::read_to_string(env("CREATOR_LIVE_OUTLOOK")).unwrap();
    let outlook_ids = || {
        let q = store::EmailQuery {
            kind: Some("outlook".into()),
            ..Default::default()
        };
        db.email_ids(&q).unwrap()
    };
    let before = outlook_ids();
    let r = db.import_outlook(&text).unwrap();
    eprintln!(
        "IMPORT added={} duplicates={} invalid={}",
        r.added,
        r.duplicates.len(),
        r.invalid.len()
    );
    outlook_ids()
        .into_iter()
        .filter(|id| !before.contains(id))
        .collect()
}

#[tokio::test]
#[ignore]
async fn live_outlook_test() {
    let out = PathBuf::from(env("CREATOR_LIVE_OUT"));
    std::fs::create_dir_all(&out).unwrap();
    let db = Arc::new(Db::open(&out.join("creator.sqlite")).unwrap());
    let sample = std::env::var("CREATOR_LIVE_SAMPLE")
        .ok()
        .filter(|_| std::env::var("CREATOR_LIVE_OUTLOOK").is_err());
    let new: Vec<i64> = match sample {
        Some(n) => sample_outlook(&db, n.parse().unwrap()),
        None => paste_outlook(&db),
    };
    let outlook = OutlookClient::new(db.clone(), OutlookUrls::live()).unwrap();
    use futures_util::StreamExt;
    let states: Vec<&str> = futures_util::stream::iter(new.iter().copied())
        .map(|id| crate::creator::pages::test_one(&db, None, &outlook, id))
        .buffered(5)
        .collect()
        .await;
    for (id, st) in new.iter().zip(&states) {
        eprintln!("MAILBOX {id} {st}");
    }
    for s in ["active", "gone", "failed"] {
        eprintln!("TOTAL {s}={}", states.iter().filter(|x| **x == s).count());
    }
}

/// Task 5's dev run on the Windows test box: our commands (`pages::call`) on a
/// fresh sandboxed store, over HTTP on 127.0.0.1:4795, for `npm run dev:mock`
/// with `VITE_CREATOR_BRIDGE=http://127.0.0.1:4795` (see `src/creator/api.ts`).
/// Our built app cannot run there: its WebView2 needs the desktop session,
/// where stock ShardBrowser holds the single instance. Prints `DB <path>`;
/// stops after `CREATOR_BRIDGE_SECS` (1800).
#[tokio::test]
#[ignore]
async fn dev_bridge() {
    use axum::http::header;
    use axum::routing::post;
    /// The dev server's page (`dev:mock --port 4796`): the only origin answered.
    const DEV_ORIGIN: &str = "http://127.0.0.1:4796";
    let _serial = crate::migrate::serial();
    let _root = crate::store::test_root("creator-bridge").with_config();
    eprintln!("DB {}", store::db_path().unwrap().display());
    // A text/plain body keeps the page's fetch a simple request (no preflight).
    let app = axum::Router::new().route(
        "/call",
        post(|body: String| async move {
            let b: serde_json::Value = serde_json::from_str(&body).unwrap_or_default();
            let cmd = b["cmd"].as_str().unwrap_or("");
            let r = match crate::creator::pages::call(cmd, &b["args"]).await {
                Some(Ok(v)) => json!({ "ok": v }),
                Some(Err(e)) => json!({ "err": format!("{e:#}") }),
                None => json!({ "err": format!("unknown creator command: {cmd}") }),
            };
            (
                [(header::ACCESS_CONTROL_ALLOW_ORIGIN, DEV_ORIGIN)],
                axum::Json(r),
            )
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:4795")
        .await
        .unwrap();
    let secs = std::env::var("CREATOR_BRIDGE_SECS").map_or(1800, |v| v.parse().unwrap());
    eprintln!("BRIDGE up");
    let _ = tokio::time::timeout(
        std::time::Duration::from_secs(secs),
        axum::serve(listener, app),
    )
    .await;
}

/// Median of 5 runs of `f`, against the 300 ms budget.
fn perf(name: &str, mut f: impl FnMut()) {
    let mut ms: Vec<f64> = (0..5)
        .map(|_| {
            let t = std::time::Instant::now();
            f();
            t.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    ms.sort_by(f64::total_cmp);
    let m = ms[2];
    let verdict = if m > 300.0 { "SLOW" } else { "OK" };
    eprintln!("PERF {name} median_ms={m:.0} {verdict}");
}

/// M4: the pages' queries on the store in `CREATOR_LIVE_OUT` (a release
/// build for real numbers), and the free count's query plan.
#[test]
#[ignore]
fn live_perf() {
    let path = PathBuf::from(env("CREATOR_LIVE_OUT")).join("creator.sqlite");
    assert!(path.is_file(), "no store in CREATOR_LIVE_OUT");
    let t = std::time::Instant::now();
    let db = Db::open(&path).unwrap();
    eprintln!("OPEN ms={:.0}", t.elapsed().as_secs_f64() * 1000.0);
    let window = |f: &dyn Fn(&mut store::EmailQuery)| {
        let mut q = store::EmailQuery {
            limit: 100,
            ..Default::default()
        };
        f(&mut q);
        q
    };
    let emails: [(&str, store::EmailQuery); 6] = [
        ("emails_all", window(&|_| {})),
        (
            "emails_outlook",
            window(&|q| q.kind = Some("outlook".into())),
        ),
        (
            "emails_active",
            window(&|q| q.status = Some("active".into())),
        ),
        ("emails_gone", window(&|q| q.status = Some("gone".into()))),
        (
            "emails_free_breezeblue",
            window(&|q| q.free_for = Some(KEY.into())),
        ),
        ("emails_search_a", window(&|q| q.q = "a".into())),
    ];
    for (name, q) in &emails {
        perf(name, || {
            db.email_list(q).unwrap();
        });
    }
    perf("breeze_free_outlook_count", || {
        db.breeze_free_outlook_count().unwrap();
    });
    let newest = db
        .email_list(&window(&|q| q.kind = Some("outlook".into())))
        .unwrap()["items"][0]["id"]
        .as_i64();
    match newest {
        Some(id) => perf("has_platform_mail_newest_outlook", || {
            db.has_platform_mail(id, KEY, None).unwrap();
        }),
        None => eprintln!("PERF has_platform_mail_newest_outlook skipped: no Outlook mailbox"),
    }
    let accounts = store::AccountQuery {
        limit: 100,
        ..Default::default()
    };
    perf("accounts_window", || {
        db.account_list(&accounts).unwrap();
    });
    perf("nav_counts", || {
        db.nav_counts().unwrap();
    });
    let (w, args) = Db::breeze_free_outlook_where();
    let c =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let mut st = c
        .prepare(&format!(
            "EXPLAIN QUERY PLAN SELECT COUNT(*) FROM mailboxes m WHERE {w}"
        ))
        .unwrap();
    let plan: Vec<String> = st
        .query_map(rusqlite::params_from_iter(&args), |r| r.get(3))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    for line in plan {
        eprintln!("PLAN {line}");
    }
}

/// The store's browser rows' Shard profile ids, read-only.
fn profile_ids(path: &Path) -> Vec<String> {
    let c = rusqlite::Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .unwrap();
    let mut st = c
        .prepare("SELECT path FROM browsers WHERE COALESCE(path,'')<>''")
        .unwrap();
    let ids = st
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    ids
}

/// M4: how many of the store's profile ids Shard's `profiles` folder
/// (`CREATOR_LIVE_SHARD_PROFILES`, only listed) still holds.
#[test]
#[ignore]
fn live_profiles_check() {
    let path = PathBuf::from(env("CREATOR_LIVE_OUT")).join("creator.sqlite");
    let dir = PathBuf::from(env("CREATOR_LIVE_SHARD_PROFILES"));
    assert!(dir.is_dir(), "CREATOR_LIVE_SHARD_PROFILES is not a folder");
    let ids = profile_ids(&path);
    let found = ids
        .iter()
        .filter(|id| dir.join(format!("{id}.json")).is_file())
        .count();
    eprintln!(
        "PROFILES browsers_with_profile={} found={found} missing={}",
        ids.len(),
        ids.len() - found
    );
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let p = e.path();
        if e.file_type().unwrap().is_dir() {
            copy_dir(&p, &to.join(e.file_name()));
        } else {
            std::fs::copy(&p, to.join(e.file_name())).unwrap();
        }
    }
}

/// The domains whose cookies show an imported account still signed in.
fn cookie_domains(platform: &str) -> &'static [&'static str] {
    match platform {
        "breezeblue" => &["breezeblue.ai"],
        "fishaudio" | "fish" => &["fish.audio"],
        "noiz" => &["noiz.ai"],
        "grok" => &["grok.com", "x.ai"],
        "qwen" => &["qwen.ai"],
        "chatgpt" => &["chatgpt.com", "openai.com"],
        "elevenlabs" => &["elevenlabs.io"],
        "google" => &["google.com"],
        _ => &[],
    }
}

/// M4 (D-M4-4): one imported account's profile, copied from stock Shard into
/// the sandbox, opened through our run path on its row's proxy. It never
/// navigates: a visit could rotate the session and sign out the production
/// copy, so the copy's saved tabs are dropped and only the fresh blank tab is
/// read (timezone, and the cookies of the account's platforms).
#[tokio::test]
#[ignore]
async fn live_open_imported() {
    let _serial = crate::migrate::serial();
    let _root = crate::store::test_root("creator-live-open").with_config();
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
    let shard = PathBuf::from(env("CREATOR_LIVE_SHARD_COPY"));
    copy_dir(
        &shard.join("profiles"),
        &crate::store::profiles_dir().unwrap(),
    );
    copy_dir(
        &shard.join("user-data"),
        &crate::store::user_data_root().unwrap(),
    );

    let path = PathBuf::from(env("CREATOR_LIVE_OUT")).join("creator.sqlite");
    let db = Db::open(&path).unwrap();
    let id: i64 = env("CREATOR_LIVE_BROWSER_ID").parse().unwrap();
    let row = db.browser_get(id).unwrap().expect("no such browser row");
    let profile = row.path.clone().expect("the row has no profile");
    // No restored tabs: the last session would load the platform's pages.
    let udd = crate::store::user_data_root().unwrap().join(&profile);
    for d in [udd.join("Sessions"), udd.join("Default").join("Sessions")] {
        let _ = std::fs::remove_dir_all(d);
    }
    for f in [
        "Current Session",
        "Current Tabs",
        "Last Session",
        "Last Tabs",
    ] {
        let _ = std::fs::remove_file(udd.join("Default").join(f));
    }
    let proxy = row.proxy_id.and_then(|p| db.proxy_get(p).unwrap());
    let platforms: Vec<String> = db
        .account_list(&store::AccountQuery {
            limit: 100_000,
            ..Default::default()
        })
        .unwrap()["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|a| a["browser_id"] == id)
        .filter_map(|a| a["platform"].as_str().map(str::to_string))
        .collect();

    let opened = shard::open_session(&db, id, proxy.as_ref()).await.unwrap();
    let tz: String = opened
        .page
        .eval(
            "() => Intl.DateTimeFormat().resolvedOptions().timeZone",
            crate::creator::web::World::Isolated,
        )
        .await
        .unwrap_or_default();
    let mut cookies = 0;
    for p in &platforms {
        for d in cookie_domains(p) {
            cookies += opened.page.cookies(d).await.unwrap().len();
        }
    }
    eprintln!(
        "OPENED timezone={tz} cookies={cookies} platforms={}",
        platforms.join(",")
    );
    drop(opened);
    shard::stop(&profile).await.unwrap();
}
