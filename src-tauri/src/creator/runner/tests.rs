//! The runner against the fake Breeze site and its fake Ele API, in a bare
//! test engine (`Bare`), on a temp store.

use std::sync::atomic::{AtomicBool, AtomicU32};

use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};

use serde_json::Value;

use super::*;
use crate::creator::store::MailMessage;
use crate::creator::store::{temp_db, TempFile};
use crate::creator::testsite::{Knobs, OutlookToken, Site, Turnstile, VERIFY_TOKEN};
use crate::creator::web::testkit::{Bare, FixtureServer};
use crate::creator::web::Session;

const ELE_KEY: &str = "ELEKEY-secret-123";
const PROXY_PW: &str = "pxSecret99";
const OUTLOOK_RT: &str = "RT0-secret-refresh";
const OUTLOOK_CID: &str = "CID-secret-client";

/// The fake site serves plain http on 127.0.0.1; `link_pick` takes https only.
/// The test inbox shows such a link to the pick as https on a marker host and
/// hands the http one back. Real https links pass unchanged.
pub(super) fn loopback_http(
    pick: impl Fn(&MailMessage) -> Option<String> + Send + Sync,
) -> impl Fn(&MailMessage) -> Option<String> + Send + Sync {
    move |m: &MailMessage| {
        let mut m = m.clone();
        m.html = m
            .html
            .map(|h| h.replace("http://127.0.0.1", "https://loopback.test"));
        pick(&m).map(|l| l.replacen("https://loopback.test", "http://127.0.0.1", 1))
    }
}

/// Like Shard: once stopped (or closed by the person: `gone`), the browser
/// reads as gone, so a failure text read after our own stop would be
/// "Interrupted".
struct TestBrowsers {
    ws: String,
    /// Opens asked for (a pre-flight failure launches nothing).
    opens: AtomicU32,
    fail_open: AtomicBool,
    gone: AtomicBool,
    stopped: AtomicBool,
}

impl Browsers for TestBrowsers {
    fn open<'a>(
        &'a self,
        _db: &'a Db,
        _browser_id: i64,
        _proxy: Option<&'a Proxy>,
    ) -> BoxFuture<'a, Result<Opened>> {
        Box::pin(async move {
            self.opens.fetch_add(1, SeqCst);
            if self.fail_open.load(SeqCst) {
                bail!("ShardBrowser did not open");
            }
            self.stopped.store(false, SeqCst);
            let session = Session::connect(&self.ws).await?;
            let page = session.page().await?;
            Ok(Opened {
                session,
                page,
                profile_id: "test".into(),
            })
        })
    }

    fn failure_text(&self, _profile_id: &str, error: String) -> String {
        if self.gone.load(SeqCst) || self.stopped.load(SeqCst) {
            INTERRUPTED.into()
        } else {
            error
        }
    }

    fn stop<'a>(&'a self, _profile_id: &'a str) -> BoxFuture<'a, ()> {
        self.stopped.store(true, SeqCst);
        Box::pin(async {})
    }
}

struct Env {
    cx: Ctx<TestBrowsers>,
    site: Site,
    _bare: Option<Bare>,
    _f: TempFile,
}

impl Drop for Env {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.cx.screens);
    }
}

fn ctx(ele_base: &str, ws: String, urls: Urls) -> (Ctx<TestBrowsers>, TempFile) {
    ctx_with(ele_base, OutlookUrls::on("http://127.0.0.1:9"), ws, urls)
}

fn ctx_with(
    ele_base: &str,
    outlook: OutlookUrls,
    ws: String,
    urls: Urls,
) -> (Ctx<TestBrowsers>, TempFile) {
    let (db, f) = temp_db();
    let db = Arc::new(db);
    db.settings_set("ele_api_key", ELE_KEY).unwrap();
    let ele = EleClient::new(ele_base, ELE_KEY, db.clone()).unwrap();
    let outlook = OutlookClient::new(db.clone(), outlook).unwrap();
    let cx = Ctx {
        db,
        ele: Some(ele),
        outlook,
        browsers: TestBrowsers {
            ws,
            opens: AtomicU32::new(0),
            fail_open: AtomicBool::new(false),
            gone: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
        },
        urls,
        tm: Timings::fast(),
        screens: std::env::temp_dir().join(format!("creator-shots-{}", uuid::Uuid::new_v4())),
    };
    (cx, f)
}

async fn env(k: Knobs) -> Option<Env> {
    let bare = Bare::launch().await?;
    let site = Site::start(k).await;
    let (cx, f) = ctx_with(
        &site.ele(),
        site.outlook_urls(),
        bare.ws.clone(),
        site.urls.clone(),
    );
    Some(Env {
        cx,
        site,
        _bare: Some(bare),
        _f: f,
    })
}

macro_rules! env {
    ($k:expr) => {
        match env($k).await {
            Some(e) => e,
            None => return,
        }
    };
}

fn site_port(e: &Env) -> i64 {
    let u = url::Url::parse(&e.site.urls.login).unwrap();
    u.port().unwrap().into()
}

fn only_account(e: &Env) -> store::Account {
    let rows = e.cx.db.accounts_recent(KEY, 10).unwrap();
    assert_eq!(rows.len(), 1);
    rows.into_iter().next().unwrap()
}

/// No secret in any event, error or step note.
fn assert_no_secrets(e: &Env, id: i64) {
    let db = &e.cx.db;
    let a = db.account_get(id).unwrap().unwrap();
    let mb = db.mailbox_get(a.mailbox_id).unwrap().unwrap();
    let mut secrets = vec![
        mb.email.clone(),
        mb.token.clone(),
        OUTLOOK_RT.to_string(),
        OUTLOOK_CID.to_string(),
        db.account_secret(id, "password").unwrap().unwrap(),
        VERIFY_TOKEN.to_string(),
        ELE_KEY.to_string(),
        PROXY_PW.to_string(),
    ];
    secrets.extend(e.site.seen(|s| s.key.clone()));
    secrets.extend(mb.client_id.clone());
    secrets.extend(e.site.seen(|s| s.outlook_issued.clone()));
    let mut texts: Vec<String> = db
        .events_of(id)
        .unwrap()
        .into_iter()
        .map(|e| e.message)
        .collect();
    texts.extend(a.error.clone());
    texts.extend(a.step_note.clone());
    assert!(texts.len() > 5, "{texts:?}");
    for t in &texts {
        for s in &secrets {
            assert!(!t.contains(s.as_str()), "a secret in {t:?}");
        }
    }
}

#[tokio::test]
async fn runner_happy_path_persists_linked_with_key() {
    let e = env!(Knobs::default());
    // The proxy is the fake site itself: it answers the key test's proxied GET.
    let px =
        e.cx.db
            .proxy_add(
                "127.0.0.1",
                site_port(&e),
                Some("pxuser"),
                Some(PROXY_PW),
                "static",
            )
            .unwrap();
    let out = make_breeze(&e.cx, 1, "rotate", "ele", &RunState::default()).await;
    assert_eq!(
        out,
        [Outcome {
            status: "linked",
            error: None
        }]
    );

    let a = only_account(&e);
    assert_eq!((a.status.as_str(), a.route.as_str()), ("linked", "email"));
    assert_eq!((a.error, a.error_code), (None, None));
    assert!(a.finished_at.is_some() && a.started_at.is_some() && a.job_id.is_some());
    let key = e.site.seen(|s| s.key.clone()).unwrap();
    assert_eq!(e.cx.db.account_secret(a.id, "api_key").unwrap(), Some(key));
    let extra: Value = serde_json::from_str(&a.extra).unwrap();
    assert_eq!(extra["api_key_untested"], false);
    assert!(extra["api_key_at"].is_string());
    let p = e.cx.db.proxy_get(px).unwrap().unwrap();
    assert_eq!((p.signups_ok, p.signups_total), (1, 1));
    let br = e.cx.db.browser_of_mailbox(a.mailbox_id).unwrap().unwrap();
    assert_eq!(br.proxy_id, Some(px));

    let ev = e.cx.db.events_of(a.id).unwrap();
    assert!(ev.iter().all(|x| x.browser_id == Some(br.id)));
    let msgs: Vec<&str> = ev.iter().map(|x| x.message.as_str()).collect();
    let at = |m: &str| {
        msgs.iter()
            .position(|x| *x == m)
            .unwrap_or_else(|| panic!("{m} not in {msgs:?}"))
    };
    assert_eq!(at("browser opened"), 0);
    assert!(at("breezeblue: sign up by email") < at("breezeblue: verify mail"));
    assert!(at("breezeblue: verify mail") < at("breezeblue: signed in"));
    assert_eq!(*msgs.last().unwrap(), "breezeblue: linked");
    assert_no_secrets(&e, a.id);
}

#[tokio::test]
async fn runner_failure_persists_code_and_screenshot() {
    let e = env!(Knobs {
        turnstile: Turnstile::FailAlways,
        ..Knobs::default()
    });
    let out = make_breeze(&e.cx, 1, "none", "ele", &RunState::default()).await;
    // The failure text is read before our own stop (after it, it would be
    // "Interrupted").
    assert_eq!(out, [failed(bb::CAPTCHA_UNSOLVED.into())]);
    assert!(e.cx.browsers.stopped.load(SeqCst), "stopped at the end");
    let a = only_account(&e);
    assert_eq!(a.status, "failed");
    assert_eq!(a.error_code.as_deref(), Some("CAPTCHA_UNSOLVED"));
    let shot = a.screenshot.expect("a screenshot");
    assert!(shot.ends_with(&format!("{}.png", a.id)));
    assert!(std::fs::metadata(&shot).unwrap().len() > 0);
    assert_no_secrets(&e, a.id);
}

#[tokio::test]
async fn a_browser_closed_mid_run_is_interrupted_without_a_screenshot() {
    let e = env!(Knobs {
        turnstile: Turnstile::FailAlways,
        ..Knobs::default()
    });
    e.cx.browsers.gone.store(true, SeqCst);
    let out = make_breeze(&e.cx, 1, "none", "ele", &RunState::default()).await;
    assert_eq!(out, [failed(INTERRUPTED.into())]);
    let a = only_account(&e);
    assert_eq!(a.error_code.as_deref(), Some("INTERRUPTED"));
    assert_eq!(a.step_note.as_deref(), Some(INTERRUPTED));
    assert_eq!(a.screenshot, None);
}

#[tokio::test]
async fn password_saved_before_page_opens() {
    let e = env!(Knobs::default());
    e.cx.browsers.fail_open.store(true, SeqCst);
    let out = make_breeze(&e.cx, 1, "none", "ele", &RunState::default()).await;
    assert_eq!(out, [failed("ShardBrowser did not open".into())]);
    let a = only_account(&e);
    assert_eq!(a.error_code.as_deref(), Some("OTHER"));
    let saved =
        e.cx.db
            .account_secret(a.id, "password")
            .unwrap()
            .expect("saved");

    // The rerun reuses it, and a linked run clears the error and its code.
    e.cx.browsers.fail_open.store(false, SeqCst);
    let out = run_account(&e.cx, a.id).await;
    assert_eq!(out.status, "linked", "{:?}", out.error);
    assert_eq!(
        e.site.seen(|s| s.typed_password.clone()),
        Some(saved.clone())
    );
    assert_eq!(
        e.cx.db.account_secret(a.id, "password").unwrap(),
        Some(saved)
    );
    let a = e.cx.db.account_get(a.id).unwrap().unwrap();
    assert_eq!((a.error, a.error_code, a.screenshot), (None, None, None));
}

#[tokio::test]
async fn no_secret_in_events_or_errors() {
    // A key test that says nothing (HTTP 500): the key is kept, untested.
    let e = env!(Knobs {
        balance_status: 500,
        ..Knobs::default()
    });
    e.cx.db
        .proxy_add(
            "127.0.0.1",
            site_port(&e),
            Some("pxuser"),
            Some(PROXY_PW),
            "static",
        )
        .unwrap();
    let out = make_breeze(&e.cx, 1, "rotate", "ele", &RunState::default()).await;
    assert_eq!(out, [failed("API key test failed: HTTP 500".into())]);
    let a = only_account(&e);
    assert_eq!(a.error_code.as_deref(), Some("API_KEY_FAILED"));
    assert_eq!(
        e.cx.db.account_secret(a.id, "api_key").unwrap(),
        e.site.seen(|s| s.key.clone())
    );
    let extra: Value = serde_json::from_str(&a.extra).unwrap();
    assert_eq!(extra["api_key_untested"], true);
    let p = e.cx.db.proxies_list().unwrap().remove(0);
    assert_eq!((p.signups_ok, p.signups_total), (0, 1));
    assert_no_secrets(&e, a.id);
}

#[test]
fn one_run_at_a_time() {
    let run = RunState::default();
    run.begin(3).unwrap();
    assert_eq!(
        run.begin(1).unwrap_err().to_string(),
        "a run is in progress"
    );
    assert_eq!(
        run.json(),
        json!({ "running": true, "done": 0, "total": 3 })
    );
    run.stop.store(true, SeqCst);
    run.end();
    run.begin(1).unwrap();
    assert!(!run.stop.load(SeqCst), "a new run starts unstopped");
}

#[tokio::test]
async fn stop_is_checked_between_accounts() {
    let (cx, _f) = ctx("http://127.0.0.1:9/", String::new(), Urls::live());
    let run = RunState::default();
    run.stop.store(true, SeqCst);
    assert!(make_breeze(&cx, 3, "none", "ele", &run).await.is_empty());
}

#[tokio::test]
async fn ele_down_fails_without_rows_and_no_proxy_fails_the_account() {
    let (cx, _f) = ctx("http://127.0.0.1:9/", String::new(), Urls::live());
    let out = make_breeze(&cx, 1, "none", "ele", &RunState::default()).await;
    assert!(out[0]
        .error
        .as_deref()
        .unwrap()
        .starts_with("Ele API unavailable"));
    assert!(cx.db.accounts_recent(KEY, 10).unwrap().is_empty());

    let ele = fake_ele(200, vec![]).await;
    let (cx, _f) = ctx(&ele.site, String::new(), Urls::live());
    // The first NO_PROXY ends the run: no more mailboxes for an empty pool.
    let run = RunState::default();
    let out = make_breeze(&cx, 3, "rotate", "ele", &run).await;
    assert_eq!(out, [failed(NO_PROXY.into())]);
    assert_eq!(run.json()["done"], 1);
    let a = cx.db.accounts_recent(KEY, 10).unwrap().remove(0);
    assert_eq!(
        (a.status.as_str(), a.error_code.as_deref()),
        ("failed", Some("NO_PROXY"))
    );
}

/// An Ele API answering `status` with `emails` for every inbox.
async fn fake_ele(status: u16, emails: Vec<Value>) -> FixtureServer {
    let app = Router::new()
        .route(
            "/mail-domains",
            get(|| async {
                Json(json!({ "domains": [{ "domain": "d.test", "is_active": true }] }))
            }),
        )
        .route(
            "/inbox",
            post(|| async {
                (
                    StatusCode::CREATED,
                    Json(json!({ "email": "u@d.test", "token": "TOK-secret-1" })),
                )
            }),
        )
        .route(
            "/inbox/:token/emails",
            get(move || async move {
                (
                    StatusCode::from_u16(status).unwrap(),
                    Json(json!({ "emails": emails })),
                )
            }),
        );
    FixtureServer::start_with(app).await
}

async fn inbox_of(ele: &FixtureServer) -> (Ctx<TestBrowsers>, TempFile, Mailbox) {
    let (cx, f) = ctx(&ele.site, String::new(), Urls::live());
    let mb = cx.ele.as_ref().unwrap().create_mailbox().await.unwrap();
    (cx, f, mb)
}

#[tokio::test]
async fn ele_inbox_picks_the_verify_link_from_the_sender() {
    let link = "https://breezeblue.ai/auth/verify?token=abc";
    let ele = fake_ele(
        200,
        vec![
            json!({ "id": "1", "from": "someone@else.com", "subject": "x",
                    "html": "<a href=\"https://else.com/verify?t=1\">v</a>" }),
            json!({ "id": "2", "from": "Breeze Blue <noreply@breezeblue.ai>", "subject": "Verify",
                    "html": format!("<a href=\"https://breezeblue.ai/home\">home</a><a href=\"{link}\">Verify</a>") }),
        ],
    )
    .await;
    let (cx, _f, mb) = inbox_of(&ele).await;
    let scrub = Scrub::default();
    let inbox = MailInbox {
        client: cx.mail_for(&mb).unwrap(),
        mailbox: mb,
        scrub: &scrub,
    };
    let never = || false;
    let got = inbox
        .wait_link(bb::VERIFY_SENDER, bb::VERIFY_ACCEPT, Duration::ZERO, &never)
        .await;
    assert_eq!(got, Ok(Some(link.to_string())));
    // Handed out: masked from then on.
    assert_eq!(scrub.clean(&format!("open {link}")), "open ***");
}

#[tokio::test]
async fn ele_inbox_errors_hold_no_url_token_or_key() {
    let ele = fake_ele(500, vec![]).await;
    let (cx, _f, mb) = inbox_of(&ele).await;
    let scrub = Scrub::default();
    let inbox = MailInbox {
        client: cx.mail_for(&mb).unwrap(),
        mailbox: mb.clone(),
        scrub: &scrub,
    };
    let never = || false;
    let errs = [
        inbox.reuse().await.unwrap_err(),
        // Past the blip grace: a failed poll is an error at once.
        {
            let mut ele = EleClient::new(&ele.site, ELE_KEY, cx.db.clone()).unwrap();
            ele.grace.unavailable_after = Duration::ZERO;
            let inbox = MailInbox {
                client: MailClient::Ele(&ele),
                mailbox: mb.clone(),
                scrub: &scrub,
            };
            inbox
                .wait_link(bb::VERIFY_SENDER, bb::VERIFY_ACCEPT, Duration::ZERO, &never)
                .await
                .unwrap_err()
        },
    ];
    for e in errs {
        assert!(e.starts_with("Ele API unavailable"), "{e}");
        for bad in ["http", "127.0.0.1", "TOK-secret-1", ELE_KEY, "u@d.test"] {
            assert!(!e.contains(bad), "{bad} in {e}");
        }
    }
}

#[test]
fn scrub_masks_quoted_forms_and_skips_short_values() {
    let s = Scrub::default();
    s.add("abc");
    s.add("p@ss w/rd");
    let p = Proxy {
        id: 1,
        host: "1.2.3.4".into(),
        port: 8080,
        username: Some("user".into()),
        password: Some("p@ss w/rd".into()),
        active: true,
        status: "unknown".into(),
        last_used_at: None,
        refused_at: None,
        kind: "static".into(),
        signups_ok: 0,
        signups_total: 0,
    };
    let url = proxy_url(&p);
    assert!(url.contains("p%40ss"), "{url}");
    assert_eq!(
        s.clean(&format!("via {url}")),
        "via http://user:***@1.2.3.4:8080/"
    );
    assert_eq!(s.clean("abc p@ss w/rd p%40ss+w%2Frd"), "abc *** ***");
}

#[tokio::test]
async fn the_job_is_seen_from_the_start_and_its_accounts_join_it() {
    let ele = fake_ele(200, vec![]).await;
    let (cx, _f) = ctx(&ele.site, String::new(), Urls::live());
    let run = RunState::default();
    let job = begin_job(&cx.db, &run, 1, "rotate").unwrap();
    assert_eq!(
        run.job(),
        Some((job, 1)),
        "live before the spawned run starts"
    );
    assert!(begin_job(&cx.db, &run, 1, "rotate").is_err());
    make_breeze(&cx, 1, "rotate", "ele", &run).await;
    let a = cx.db.accounts_recent(KEY, 10).unwrap().remove(0);
    assert_eq!(a.job_id, Some(job));
}

// ---- Outlook mailboxes (M3)

/// An Outlook mailbox as pasted (`a@outlook.test`), its free id.
fn outlook_mailbox(db: &Db, n: u32) -> i64 {
    let line = format!("o{n}@outlook.test|pw|{OUTLOOK_RT}|{OUTLOOK_CID}|r");
    assert_eq!(db.import_outlook(&line).unwrap().added, 1);
    db.breeze_free_outlook(0, 1000)
        .unwrap()
        .into_iter()
        .max()
        .unwrap()
}

/// An account row (no run yet) on a new Outlook mailbox with its browser.
fn outlook_account(db: &Db) -> (i64, i64) {
    let mb = outlook_mailbox(db, 0);
    db.browser_create(mb, None, "none").unwrap();
    (db.account_create(mb, KEY, "email", None).unwrap(), mb)
}

/// The fake site without a browser: for runs that end before one opens.
async fn site_ctx(k: Knobs) -> (Ctx<TestBrowsers>, TempFile, Site) {
    let site = Site::start(k).await;
    let (cx, f) = ctx_with(
        &site.ele(),
        site.outlook_urls(),
        String::new(),
        site.urls.clone(),
    );
    (cx, f, site)
}

#[tokio::test]
async fn outlook_breeze_happy_path_persists_linked() {
    let e = env!(Knobs::default());
    let mb = outlook_mailbox(&e.cx.db, 1);
    let out = make_breeze(&e.cx, 1, "none", "outlook", &RunState::default()).await;
    assert_eq!(
        out,
        [Outcome {
            status: "linked",
            error: None
        }]
    );
    let a = only_account(&e);
    assert_eq!((a.mailbox_id, a.status.as_str()), (mb, "linked"));
    let row = e.cx.db.mailbox_get(mb).unwrap().unwrap();
    assert_ne!(row.token, OUTLOOK_RT, "rotated");
    assert_eq!(
        Some(row.token),
        e.site.seen(|s| s.outlook_issued.last().cloned())
    );
    assert_no_secrets(&e, a.id);
}

#[tokio::test]
async fn gone_mailbox_fails_before_the_browser_opens() {
    let (cx, _f, _site) = site_ctx(Knobs::default()).await;
    let (id, mb) = outlook_account(&cx.db);
    cx.db.mailbox_set_status(mb, "gone").unwrap();
    let out = run_account(&cx, id).await;
    assert_eq!(out, failed("mailbox gone".into()));
    assert_eq!(cx.browsers.opens.load(SeqCst), 0);
    let a = cx.db.account_get(id).unwrap().unwrap();
    assert_eq!(a.error_code.as_deref(), Some("MAILBOX_GONE"));
}

#[tokio::test]
async fn dead_grant_at_preflight_is_mailbox_gone_without_a_browser() {
    let (cx, _f, _site) = site_ctx(Knobs {
        outlook_token: OutlookToken::InvalidGrant,
        ..Knobs::default()
    })
    .await;
    let (id, mb) = outlook_account(&cx.db);
    let out = run_account(&cx, id).await;
    assert_eq!(out, failed("mailbox gone".into()));
    assert_eq!(cx.browsers.opens.load(SeqCst), 0);
    assert_eq!(cx.db.mailbox_get(mb).unwrap().unwrap().status, "gone");
    let code = cx.db.account_get(id).unwrap().unwrap().error_code.unwrap();
    assert_eq!(code, "MAILBOX_GONE");
    assert!(!crate::creator::errors::fail_info(&code).1);
}

#[tokio::test]
async fn outlook_down_at_preflight_is_mail_api_down_and_retryable() {
    let (cx, _f, _site) = site_ctx(Knobs {
        outlook_token: OutlookToken::Http(503),
        ..Knobs::default()
    })
    .await;
    let (id, mb) = outlook_account(&cx.db);
    let out = run_account(&cx, id).await;
    assert_eq!(
        out,
        failed("Outlook unavailable: inbox token HTTP 503".into())
    );
    assert_eq!(cx.browsers.opens.load(SeqCst), 0);
    assert_eq!(cx.db.mailbox_get(mb).unwrap().unwrap().status, "active");
    let code = cx.db.account_get(id).unwrap().unwrap().error_code.unwrap();
    assert_eq!(code, "MAIL_API_DOWN");
    assert!(crate::creator::errors::fail_info(&code).1);
}

#[tokio::test]
async fn outlook_make_takes_free_mailboxes_oldest_first_and_skips_taken() {
    let (cx, _f, _site) = site_ctx(Knobs::default()).await;
    let db = &*cx.db;
    let ids: Vec<i64> = (1..=12).map(|n| outlook_mailbox(db, n)).collect();
    db.mailbox_create("ele", "e@ele.test", "ele.test", "tok")
        .unwrap();
    let account = |mb: i64, platform: &str, status: &str, code: Option<&str>| {
        let id = db.account_create(mb, platform, "email", None).unwrap();
        let p = AccountPatch {
            status: Some(status.into()),
            error_code: Some(code.map(str::to_string)),
            ..Default::default()
        };
        db.account_update(id, &p).unwrap();
        id
    };
    account(ids[0], KEY, "linked", None);
    account(ids[1], KEY, "running", None);
    let frozen = account(ids[2], KEY, "failed", Some("CAPTCHA_UNSOLVED"));
    db.exec_for_test(&format!("UPDATE accounts SET frozen=1 WHERE id={frozen}"));
    account(ids[3], KEY, "failed", Some("ACCOUNT_EXISTS"));
    db.mailbox_set_status(ids[4], "gone").unwrap();
    db.exec_for_test(&format!(
        "UPDATE mailboxes SET domain='Blocked.test' WHERE id={}",
        ids[5]
    ));
    db.blocked_domain_add("blocked.test", Some("x")).unwrap();
    db.exec_for_test(&format!(
        "UPDATE mailboxes SET client_id='' WHERE id={}",
        ids[6]
    ));
    // Breeze Blue mail in the inbox: used elsewhere. Another platform's: free.
    let mail = |mb: i64, sender: &str| {
        let m = store::NewMail {
            sender: sender.into(),
            ..Default::default()
        };
        db.mail_create(mb, &m).unwrap();
    };
    mail(ids[10], "Breeze Blue <noreply@breezeblue.ai>");
    mail(ids[11], "no-reply@fish.audio");
    // Free: a retryable failure, an account on another platform, a fresh one.
    account(ids[7], KEY, "failed", Some("CAPTCHA_UNSOLVED"));
    account(ids[8], "google", "linked", None);
    assert_eq!(
        db.breeze_free_outlook(0, 100).unwrap(),
        [ids[7], ids[8], ids[9], ids[11]]
    );
    assert_eq!(db.breeze_free_outlook(ids[8], 1).unwrap(), [ids[9]]);
    assert_eq!(db.breeze_free_outlook_count().unwrap(), 4);

    // A run of 2 takes the two oldest; the browser cannot open here.
    cx.browsers.fail_open.store(true, SeqCst);
    let out = make_breeze(&cx, 2, "none", "outlook", &RunState::default()).await;
    assert_eq!(out.len(), 2);
    let mut made: Vec<i64> = db
        .accounts_recent(KEY, 100)
        .unwrap()
        .into_iter()
        .filter(|a| a.job_id.is_some())
        .map(|a| a.mailbox_id)
        .collect();
    made.sort_unstable();
    assert_eq!(made, [ids[7], ids[8]]);
    // Asking for more than are free makes what there is.
    let out = make_breeze(&cx, 5, "none", "outlook", &RunState::default()).await;
    assert_eq!(
        out.len(),
        4,
        "the two retryable ones and the two fresh ones"
    );
}

#[test]
fn outlook_route_refuses_more_than_free() {
    let (db, _f) = temp_db();
    outlook_mailbox(&db, 1);
    assert_eq!(
        check_start(&db, 1, "none", "email:outlook").unwrap(),
        "outlook"
    );
    assert_eq!(
        check_start(&db, 2, "none", "email:outlook")
            .unwrap_err()
            .to_string(),
        "Only 1 free emails on this route"
    );
    assert_eq!(check_start(&db, 2, "none", "email:ele").unwrap(), "ele");
    assert_eq!(
        check_start(&db, 1, "none", "google")
            .unwrap_err()
            .to_string(),
        "unknown route: google"
    );
}

#[tokio::test]
async fn outlook_no_secret_in_events_or_errors() {
    let e = env!(Knobs {
        balance_status: 500,
        ..Knobs::default()
    });
    outlook_mailbox(&e.cx.db, 1);
    let out = make_breeze(&e.cx, 1, "none", "outlook", &RunState::default()).await;
    assert_eq!(out, [failed("API key test failed: HTTP 500".into())]);
    let a = only_account(&e);
    assert_no_secrets(&e, a.id);
}

#[tokio::test]
async fn a_mailbox_whose_fresh_inbox_has_breeze_mail_is_skipped_without_rows() {
    let (cx, _f, _site) = site_ctx(Knobs {
        outlook_old_breeze_mail: true,
        ..Knobs::default()
    })
    .await;
    let mb = outlook_mailbox(&cx.db, 1);
    let out = make_breeze(&cx, 1, "none", "outlook", &RunState::default()).await;
    assert!(out.is_empty(), "{out:?}");
    assert_eq!(cx.browsers.opens.load(SeqCst), 0);
    assert!(cx.db.accounts_recent(KEY, 10).unwrap().is_empty());
    assert!(cx.db.browser_of_mailbox(mb).unwrap().is_none());
    assert_eq!(cx.db.breeze_free_outlook_count().unwrap(), 0);
    let ev = cx.db.events_of_mailbox_for_test(mb);
    assert_eq!(ev, ["has Breeze Blue mail, skipped"]);
}

#[tokio::test]
async fn a_retry_on_a_mailbox_with_older_breeze_mail_is_account_exists() {
    let (cx, _f, _site) = site_ctx(Knobs::default()).await;
    let (id, mb) = outlook_account(&cx.db);
    let m = store::NewMail {
        sender: "Breeze Blue <noreply@breezeblue.ai>".into(),
        received_at: Some("2026-01-01T00:00:00Z".into()),
        ..Default::default()
    };
    cx.db.mail_create(mb, &m).unwrap();
    let out = run_account(&cx, id).await;
    let msg = "This email already has Breeze Blue mail, likely registered elsewhere";
    assert_eq!(out, failed(msg.into()));
    assert_eq!(cx.browsers.opens.load(SeqCst), 0);
    let code = cx.db.account_get(id).unwrap().unwrap().error_code.unwrap();
    assert_eq!(code, "ACCOUNT_EXISTS");
    // Breeze Blue mail that came after the account was made is its own run's.
    assert!(!cx
        .db
        .has_platform_mail(mb, KEY, Some("2025-01-01T00:00:00Z"))
        .unwrap());
    assert!(cx.db.has_platform_mail(mb, KEY, None).unwrap());
    assert!(!cx.db.has_platform_mail(mb, "fish", None).unwrap());
}

#[tokio::test]
async fn a_retry_on_an_ele_mailbox_is_not_judged_by_its_mail_dates() {
    let (cx, _f, _site) = site_ctx(Knobs::default()).await;
    let mb = cx.db.mailbox_create("ele", "e@ele.test", "ele.test", "tok").unwrap();
    cx.db.browser_create(mb.id, None, "none").unwrap();
    let id = cx.db.account_create(mb.id, KEY, "email", None).unwrap();
    let m = store::NewMail {
        sender: "Breeze Blue <noreply@breezeblue.ai>".into(),
        received_at: Some("2026-01-01T00:00:00".into()),
        ..Default::default()
    };
    cx.db.mail_create(mb.id, &m).unwrap();
    cx.browsers.fail_open.store(true, SeqCst);
    // Past the mail rule: it reaches the browser.
    let out = run_account(&cx, id).await;
    assert_eq!(out, failed("ShardBrowser did not open".into()));
    assert_eq!(cx.browsers.opens.load(SeqCst), 1);
}
