//! The runner against the fake Breeze site and its fake Ele API, in a bare
//! test engine (`Bare`), on a temp store.

use std::sync::atomic::AtomicBool;

use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};

use super::*;
use crate::creator::store::MailMessage;
use crate::creator::store::{temp_db, TempFile};
use crate::creator::testsite::{Knobs, Site, Turnstile, VERIFY_TOKEN};
use crate::creator::web::testkit::{Bare, FixtureServer};
use crate::creator::web::Session;

const ELE_KEY: &str = "ELEKEY-secret-123";
const PROXY_PW: &str = "pxSecret99";

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

struct TestBrowsers {
    ws: String,
    fail_open: AtomicBool,
}

impl Browsers for TestBrowsers {
    fn open<'a>(
        &'a self,
        _db: &'a Db,
        _browser_id: i64,
        _proxy: Option<&'a Proxy>,
    ) -> BoxFuture<'a, Result<Opened>> {
        Box::pin(async move {
            if self.fail_open.load(SeqCst) {
                bail!("ShardBrowser did not open");
            }
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
        error
    }

    fn stop<'a>(&'a self, _profile_id: &'a str) -> BoxFuture<'a, ()> {
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
    let (db, f) = temp_db();
    let db = Arc::new(db);
    db.settings_set("ele_api_key", ELE_KEY).unwrap();
    let ele = EleClient::new(ele_base, ELE_KEY, db.clone()).unwrap();
    let cx = Ctx {
        db,
        ele,
        browsers: TestBrowsers {
            ws,
            fail_open: AtomicBool::new(false),
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
    let (cx, f) = ctx(&site.ele(), bare.ws.clone(), site.urls.clone());
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
        db.account_secret(id, "password").unwrap().unwrap(),
        VERIFY_TOKEN.to_string(),
        ELE_KEY.to_string(),
        PROXY_PW.to_string(),
    ];
    secrets.extend(e.site.seen(|s| s.key.clone()));
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
    let out = make_breeze(&e.cx, 1, "rotate", &RunState::default()).await;
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
    let out = make_breeze(&e.cx, 1, "none", &RunState::default()).await;
    assert_eq!(out, [failed(bb::CAPTCHA_UNSOLVED.into())]);
    let a = only_account(&e);
    assert_eq!(a.status, "failed");
    assert_eq!(a.error_code.as_deref(), Some("CAPTCHA_UNSOLVED"));
    let shot = a.screenshot.expect("a screenshot");
    assert!(shot.ends_with(&format!("{}.png", a.id)));
    assert!(std::fs::metadata(&shot).unwrap().len() > 0);
    assert_no_secrets(&e, a.id);
}

#[tokio::test]
async fn password_saved_before_page_opens() {
    let e = env!(Knobs::default());
    e.cx.browsers.fail_open.store(true, SeqCst);
    let out = make_breeze(&e.cx, 1, "none", &RunState::default()).await;
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
    let out = make_breeze(&e.cx, 1, "rotate", &RunState::default()).await;
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
    assert!(make_breeze(&cx, 3, "none", &run).await.is_empty());
}

#[tokio::test]
async fn ele_down_fails_without_rows_and_no_proxy_fails_the_account() {
    let (cx, _f) = ctx("http://127.0.0.1:9/", String::new(), Urls::live());
    let out = make_breeze(&cx, 1, "none", &RunState::default()).await;
    assert!(out[0]
        .error
        .as_deref()
        .unwrap()
        .starts_with("Ele API unavailable"));
    assert!(cx.db.accounts_recent(KEY, 10).unwrap().is_empty());

    let ele = fake_ele(200, vec![]).await;
    let (cx, _f) = ctx(&ele.site, String::new(), Urls::live());
    let out = make_breeze(&cx, 1, "rotate", &RunState::default()).await;
    assert_eq!(out, [failed(NO_PROXY.into())]);
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
    let mb = cx.ele.create_mailbox().await.unwrap();
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
    let inbox = EleInbox {
        ele: &cx.ele,
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
    let inbox = EleInbox {
        ele: &cx.ele,
        mailbox: mb.clone(),
        scrub: &scrub,
    };
    let never = || false;
    let errs = [
        inbox.reuse().await.unwrap_err(),
        // Past the blip grace: a failed poll is an error at once.
        {
            let mut ele = EleClient::new(&ele.site, ELE_KEY, cx.db.clone()).unwrap();
            ele.unavailable_after = Duration::ZERO;
            let inbox = EleInbox {
                ele: &ele,
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
