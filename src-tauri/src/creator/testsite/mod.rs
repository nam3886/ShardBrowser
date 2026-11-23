//! Test-only: a fake Breeze Blue in-process, at the real site's paths, with
//! knobs per test. One account (the test's email); the session is a cookie.
//! The Turnstile-like widget is served on the `other` origin (cross-site, its
//! own process), like Cloudflare's frame. A fake Ele API under `/ele/` holds
//! the verify mail once the account exists (`Site::ele`).

use std::sync::{Arc, Mutex};

use axum::extract::{Query, State as Ax};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;

use crate::creator::providers::breezeblue::Urls;
use crate::creator::web::testkit::FixtureServer;

const LOGIN: &str = include_str!("breeze/login.html");
const WELCOME: &str = include_str!("breeze/welcome.html");
const KEYS: &str = include_str!("breeze/keys.html");
const WIDGET: &str = include_str!("breeze/widget.html");
pub const VERIFY_TOKEN: &str = "TOKEN123";
const SESSION: &str = "s=ok";

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Turnstile {
    /// Passes by itself at once.
    #[default]
    Pass,
    /// Fails; "Try again" passes.
    FailOnce,
    FailAlways,
    /// Fails; after "Try again" the old failure text stays this many ms, then it passes.
    Linger(u64),
    /// A drawn widget that passes `delay_ms` after its `presses`-th trusted press.
    NeedsClick {
        presses: u32,
        delay_ms: u64,
    },
    /// A drawn widget that has passed already; the page moves on `delay_ms` later.
    Passed {
        delay_ms: u64,
    },
    /// "Complete this check" forever, nothing to press.
    Never,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Noop {
    #[default]
    No,
    /// The form resets under the first "Continue"; the next one goes through.
    Once,
    Always,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Survey {
    /// The verify link lands on /welcome.
    #[default]
    AfterVerify,
    /// The verify link lands on /app/home; the key page sends to /welcome.
    AtKeyPage,
    /// The key page loads, then the app sends to /welcome.
    AfterKeyPageLoads,
}

#[derive(Clone, Debug)]
pub struct Knobs {
    pub turnstile: Turnstile,
    /// An account already exists for the email, with this password.
    pub existing_password: Option<String>,
    pub config_load_ms: u64,
    pub continue_noop: Noop,
    pub continue_disabled: bool,
    pub logged_in_after_verify: bool,
    /// The first visit of the key page finds the session gone.
    pub keys_signed_out_once: bool,
    pub survey: Survey,
    /// After the last skip the survey saves this long ("Skip question" disabled).
    pub survey_busy_ms: u64,
    pub balance_status: u16,
}

impl Default for Knobs {
    fn default() -> Self {
        Knobs {
            turnstile: Turnstile::Pass,
            existing_password: None,
            config_load_ms: 0,
            continue_noop: Noop::No,
            continue_disabled: false,
            logged_in_after_verify: true,
            keys_signed_out_once: false,
            survey: Survey::AfterVerify,
            survey_busy_ms: 0,
            balance_status: 200,
        }
    }
}

/// What the site saw.
#[derive(Debug, Default)]
pub struct Seen {
    /// Page events in order ("click Continue", "cf press", …) plus the test's own.
    pub events: Vec<String>,
    pub keys_visits: u32,
    pub key: Option<String>,
    pub key_name: Option<String>,
    /// `xi-api-key` of each /v1/balance call.
    pub balance_keys: Vec<String>,
    pub typed_password: Option<String>,
}

struct St {
    k: Knobs,
    site: String,
    other: String,
    password: Option<String>,
    /// When the account was made: the verify mail's date.
    mail_at: Option<String>,
    survey_done: bool,
    seen: Seen,
}

type Shared = Arc<Mutex<St>>;

pub struct Site {
    pub urls: Urls,
    pub link: String,
    st: Shared,
    _server: FixtureServer,
}

impl Site {
    pub async fn start(k: Knobs) -> Site {
        let st: Shared = Arc::new(Mutex::new(St {
            password: k.existing_password.clone(),
            k,
            site: String::new(),
            other: String::new(),
            mail_at: None,
            survey_done: false,
            seen: Seen::default(),
        }));
        let routes = Router::new()
            .route("/auth/login", get(login))
            .route("/api/continue", post(continue_))
            .route("/verify", get(verify))
            .route("/welcome", get(welcome))
            .route("/api/survey-done", post(survey_done))
            .route("/app/home", get(home))
            .route("/app/developer/api-keys", get(keys))
            .route("/api/keys", post(make_key))
            .route("/v1/balance", get(balance))
            .route("/api/ev", post(event))
            .route("/cf/widget.html", get(|| async { html(WIDGET.into()) }))
            .route("/ele/mail-domains", get(ele_domains))
            .route("/ele/inbox", post(ele_inbox))
            .route("/ele/inbox/:token/emails", get(ele_emails))
            .with_state(st.clone());
        let server = FixtureServer::start_with(routes).await;
        {
            let mut st = st.lock().unwrap();
            st.other = server.other.clone();
            st.site = server.site.clone();
        }
        let s = &server.site;
        let urls = Urls {
            login: format!("{s}/auth/login?next=/app/home"),
            keys: format!("{s}/app/developer/api-keys"),
            welcome: format!("{s}/welcome"),
            key_test: format!("{s}/v1/balance"),
            host_suffix: "127.0.0.1".into(),
            cf_host: "localhost".into(),
        };
        Site {
            urls,
            link: format!("{s}/verify?t={VERIFY_TOKEN}"),
            st,
            _server: server,
        }
    }

    /// The fake Ele API's base URL (`EleClient::new`).
    pub fn ele(&self) -> String {
        format!("{}/ele/", self.urls.login.split("/auth/").next().unwrap())
    }

    pub fn seen<T>(&self, f: impl FnOnce(&Seen) -> T) -> T {
        f(&self.st.lock().unwrap().seen)
    }

    /// Lets the test's own fakes (the inbox) add events in order with the page's.
    pub fn recorder(&self) -> Recorder {
        Recorder(self.st.clone())
    }

    pub fn count(&self, e: &str) -> usize {
        self.seen(|s| s.events.iter().filter(|x| *x == e).count())
    }
}

#[derive(Clone)]
pub struct Recorder(Shared);

impl Recorder {
    pub fn event(&self, e: &str) {
        self.0.lock().unwrap().seen.events.push(e.to_string());
    }
}

fn html(body: String) -> Response {
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], body).into_response()
}

fn page(tpl: &str, knobs: serde_json::Value) -> Response {
    html(tpl.replace("/*KNOBS*/{}", &knobs.to_string()))
}

fn to(path: &str) -> Response {
    (StatusCode::FOUND, [(header::LOCATION, path.to_string())]).into_response()
}

fn signed(h: &HeaderMap) -> bool {
    h.get_all(header::COOKIE)
        .iter()
        .any(|v| v.to_str().unwrap_or("").split("; ").any(|c| c == SESSION))
}

fn session_cookie() -> (header::HeaderName, String) {
    (header::SET_COOKIE, format!("{SESSION}; Path=/"))
}

const LOGIN_PATH: &str = "/auth/login?next=/app/home";

async fn login(Ax(st): Ax<Shared>, h: HeaderMap) -> Response {
    let mut st = st.lock().unwrap();
    st.seen.events.push("visit login".into());
    if signed(&h) {
        return to("/app/home");
    }
    let (mode, linger, presses, delay) = match st.k.turnstile {
        Turnstile::Pass => ("pass", 0, 0, 0),
        Turnstile::FailOnce => ("fail_once", 0, 0, 0),
        Turnstile::FailAlways => ("fail", 0, 0, 0),
        Turnstile::Linger(ms) => ("linger", ms, 0, 0),
        Turnstile::NeedsClick { presses, delay_ms } => ("click", 0, presses, delay_ms),
        Turnstile::Passed { delay_ms } => ("passed", 0, 0, delay_ms),
        Turnstile::Never => ("never", 0, 0, 0),
    };
    page(
        LOGIN,
        json!({
            "other": st.other, "turnstile": mode, "linger_ms": linger, "presses": presses,
            "pass_delay_ms": delay, "config_load_ms": st.k.config_load_ms,
            "continue_disabled": st.k.continue_disabled,
        }),
    )
}

#[derive(serde::Deserialize)]
struct Form {
    email: String,
    password: String,
}

async fn continue_(Ax(st): Ax<Shared>, Json(f): Json<Form>) -> Response {
    let mut st = st.lock().unwrap();
    let r = match st.k.continue_noop {
        Noop::Always => "noop",
        Noop::Once => {
            st.k.continue_noop = Noop::No;
            "reset"
        }
        Noop::No if f.email.is_empty() => "empty",
        Noop::No => {
            st.seen.typed_password = Some(f.password.clone());
            match &st.password {
                Some(p) if *p == f.password => {
                    return ([session_cookie()], Json(json!({"r": "in"}))).into_response()
                }
                Some(_) => "wrong",
                None => {
                    st.password = Some(f.password); // the account now exists (unverified)
                    st.mail_at = Some(crate::creator::store::now());
                    "new"
                }
            }
        }
    };
    Json(json!({ "r": r })).into_response()
}

#[derive(serde::Deserialize)]
struct Token {
    t: String,
}

async fn verify(Ax(st): Ax<Shared>, Query(q): Query<Token>) -> Response {
    let st = st.lock().unwrap();
    if q.t != VERIFY_TOKEN || !st.k.logged_in_after_verify {
        return to(LOGIN_PATH);
    }
    let next = if st.k.survey == Survey::AfterVerify {
        "/welcome"
    } else {
        "/app/home"
    };
    (
        StatusCode::FOUND,
        [(header::LOCATION, next.to_string()), session_cookie()],
    )
        .into_response()
}

async fn welcome(Ax(st): Ax<Shared>, h: HeaderMap) -> Response {
    let mut st = st.lock().unwrap();
    st.seen.events.push("visit welcome".into());
    if !signed(&h) {
        return to(LOGIN_PATH);
    }
    if st.survey_done {
        return to("/app/home");
    }
    page(WELCOME, json!({ "survey_busy_ms": st.k.survey_busy_ms }))
}

async fn survey_done(Ax(st): Ax<Shared>) -> StatusCode {
    st.lock().unwrap().survey_done = true;
    StatusCode::NO_CONTENT
}

async fn home(h: HeaderMap) -> Response {
    if !signed(&h) {
        return to(LOGIN_PATH);
    }
    html("<!doctype html><title>Home - Breeze Blue</title><h1>Home</h1>".into())
}

async fn keys(Ax(st): Ax<Shared>, h: HeaderMap) -> Response {
    let mut st = st.lock().unwrap();
    st.seen.keys_visits += 1;
    st.seen.events.push("visit keys".into());
    if st.k.keys_signed_out_once {
        st.k.keys_signed_out_once = false;
        return (
            StatusCode::FOUND,
            [
                (header::LOCATION, LOGIN_PATH.to_string()),
                (header::SET_COOKIE, "s=; Path=/; Max-Age=0".to_string()),
            ],
        )
            .into_response();
    }
    if !signed(&h) {
        return to(LOGIN_PATH);
    }
    if st.k.survey == Survey::AtKeyPage && !st.survey_done {
        return to("/welcome");
    }
    let after_load = st.k.survey == Survey::AfterKeyPageLoads && !st.survey_done;
    page(KEYS, json!({ "survey_after_load": after_load }))
}

async fn make_key(Ax(st): Ax<Shared>, name: String) -> Json<serde_json::Value> {
    // 32 key characters, one of them a hyphen (keys can hold one).
    let mut tail = uuid::Uuid::new_v4().simple().to_string();
    tail.replace_range(5..6, "-");
    let key = format!("brz_{tail}");
    let mut st = st.lock().unwrap();
    st.seen.key = Some(key.clone());
    st.seen.key_name = Some(name);
    Json(json!({ "key": key }))
}

async fn balance(Ax(st): Ax<Shared>, h: HeaderMap) -> Response {
    let mut st = st.lock().unwrap();
    let got = h
        .get("xi-api-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    st.seen.balance_keys.push(got.clone());
    let status = if st.seen.key.as_deref() == Some(got.as_str()) {
        st.k.balance_status
    } else {
        401
    };
    (StatusCode::from_u16(status).unwrap(), "{}").into_response()
}

async fn event(Ax(st): Ax<Shared>, e: String) -> StatusCode {
    st.lock().unwrap().seen.events.push(e);
    StatusCode::NO_CONTENT
}

async fn ele_domains() -> Json<serde_json::Value> {
    Json(json!({ "domains": [{ "domain": "mail.test", "is_active": true }] }))
}

async fn ele_inbox() -> Response {
    let n = uuid::Uuid::new_v4().simple().to_string();
    let body = json!({ "email": format!("u{}@mail.test", &n[..8]), "token": format!("tok{n}") });
    (StatusCode::CREATED, Json(body)).into_response()
}

/// The verify mail, once the account exists. The link is the fake site's
/// (http: the runner's test inbox takes it, see `runner::EleInbox`).
async fn ele_emails(Ax(st): Ax<Shared>) -> Json<serde_json::Value> {
    let st = st.lock().unwrap();
    let mail = st.mail_at.as_ref().map(|at| {
        json!({
            "id": "verify-1", "from": "Breeze Blue <noreply@breezeblue.ai>",
            "subject": "Verify your email", "received_at": at,
            "html": format!("<p>Hi</p><a href=\"{}/verify?t={VERIFY_TOKEN}\">Verify email</a>", st.site),
        })
    });
    Json(json!({ "emails": mail.into_iter().collect::<Vec<_>>() }))
}
