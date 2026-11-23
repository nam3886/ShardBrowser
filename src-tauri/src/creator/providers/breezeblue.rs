//! Breeze Blue (breezeblue.ai), email route (Python `providers/breezeblue.py`):
//! one form at /auth/login signs in or, for a new email, creates the account
//! behind a Cloudflare Turnstile -> verify link from noreply@breezeblue.ai ->
//! /welcome -> the survey skipped -> an API key made on /app/developer/api-keys,
//! tested on api.breeze.blue/v1/balance.
//!
//! The Turnstile passes by itself on most exits and never on some: it gets one
//! "Try again", then the run fails CAPTCHA_UNSOLVED. One still pending after
//! `tick_grace` is pressed in-page, at most `ticks` times per widget.
//!
//! Never in a log line, step note or error: the email address, password, verify
//! link or API key.

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use regex::Regex;

use super::cf;
use super::tts::{self, TtsRun};
use crate::creator::web::input::cookie_accept;
use crate::creator::web::{WebResult, World};

pub const KEY: &str = "breezeblue";
pub const EMAIL_EXISTS: &str = "A Breeze Blue account already exists for this email";
pub const NO_VERIFY_MAIL: &str = "no Breeze Blue verify mail";
pub const CAPTCHA_UNSOLVED: &str = "captcha not solved";
pub const NO_SIGNIN: &str = "Breeze Blue did not sign in";
pub const NO_KEY: &str = tts::NO_KEY;
const KEY_NAME: &str = "default";
const CLICK: Duration = Duration::from_secs(8);
const CLICK_SHORT: Duration = Duration::from_secs(5);

/// Keys can hold a hyphen.
pub static KEY_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^brz_[A-Za-z0-9_-]{32}$").unwrap());
static CONTINUE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)^\s*continue\s*$").unwrap());

/// The verify mail: sender and link words (Python `link_pick("breezeblue", ("verif",))`).
pub const VERIFY_SENDER: &str = "breezeblue";
pub const VERIFY_ACCEPT: &[&str] = &["verif"];

// Page texts, matched on the lowercased body (curly apostrophes made straight).
const WRONG_PW_TEXT: &str = "couldn't continue with that email and password";
const CHECK_MAIL_TEXT: &str = "check your email";
const TURNSTILE_TEXT: &str = "complete this check";
const CONFIG_LOADING: &str = "loading sign-in configuration";
const TURNSTILE_FAILED: [&str; 2] = ["temporarily unavailable", "verification failed"];
const CODES_JS: &str =
    "() => [...document.querySelectorAll('code')].map(e => (e.innerText || '').trim())";

/// Where the flow goes; `live()` is breezeblue.ai, tests point it at the fake site.
#[derive(Clone, Debug)]
pub struct Urls {
    pub login: String,
    pub home: String,
    pub keys: String,
    /// The new-account survey; a finished one sends on to /app/home.
    pub welcome: String,
    pub key_test: String,
    /// The signed-in check: the page's host ends with this.
    pub host_suffix: String,
    pub cf_host: String,
}

impl Urls {
    pub fn live() -> Urls {
        Urls {
            login: "https://breezeblue.ai/auth/login?next=/app/home".into(),
            home: "https://breezeblue.ai/app/home".into(),
            keys: "https://breezeblue.ai/app/developer/api-keys".into(),
            welcome: "https://breezeblue.ai/welcome".into(),
            key_test: "https://api.breeze.blue/v1/balance".into(),
            host_suffix: "breezeblue.ai".into(),
            cf_host: cf::CF_HOST.into(),
        }
    }
}

/// Python's module knobs.
#[derive(Clone, Copy, Debug)]
pub struct Timings {
    pub verify_mail: Duration,
    /// The form / key dialog show.
    pub field_wait: Duration,
    /// An outcome after "Continue"; again after "Try again".
    pub submit_wait: Duration,
    /// Signed in after opening the verify link.
    pub verify_wait: Duration,
    pub key_page_wait: Duration,
    /// The survey's questions, then "Start creating".
    pub welcome_wait: Duration,
    pub welcome_poll: Duration,
    /// A Turnstile still pending this long gets an in-page tick, again after
    /// another grace, at most `ticks` per widget.
    pub tick_grace: Duration,
    pub ticks: u32,
    /// After a tick the deadline is at least this far off.
    pub tick_settle: Duration,
    /// The tick's own draw and verify waits (Python `cf.QUICK_*`).
    pub cf_draw: Duration,
    pub cf_wait: Duration,
    pub tts: tts::Timings,
}

impl Default for Timings {
    fn default() -> Self {
        let s = Duration::from_secs;
        Timings {
            verify_mail: s(180),
            field_wait: s(30),
            submit_wait: s(45),
            verify_wait: s(30),
            key_page_wait: s(20),
            welcome_wait: s(30),
            welcome_poll: s(1),
            tick_grace: s(10),
            ticks: 2,
            tick_settle: s(15),
            cf_draw: cf::QUICK_DRAW,
            cf_wait: cf::QUICK_WAIT,
            tts: tts::Timings::default(),
        }
    }
}

impl Timings {
    /// For the fake site: short waits, still long enough for a real browser.
    pub fn fast() -> Self {
        let ms = Duration::from_millis;
        Timings {
            verify_mail: ms(2000),
            field_wait: ms(5000),
            submit_wait: ms(3000),
            verify_wait: ms(2000),
            key_page_wait: ms(5000),
            welcome_wait: ms(10000),
            welcome_poll: ms(100),
            tick_grace: ms(10000),
            ticks: 2,
            tick_settle: ms(3000),
            cf_draw: ms(3000),
            cf_wait: ms(1500),
            tts: tts::Timings {
                settle: ms(0),
                submit_enable: ms(2000),
                click_poll: ms(100),
                session_wait: ms(500),
            },
        }
    }
}

pub fn key_headers(key: &str) -> Vec<(&'static str, String)> {
    vec![("xi-api-key", key.to_string())]
}

/// A saved key still works? (Python `check_key`.)
pub async fn check_key(urls: &Urls, key: &str, proxy: Option<&str>) -> tts::KeyState {
    tts::check_key(&urls.key_test, &key_headers(key), proxy).await
}

/// What "Continue" led to, or a failure text.
#[derive(Debug, PartialEq)]
enum Got {
    Closed,
    In,
    Wrong,
    Mail,
    Captcha,
    Stuck,
    Fail(String),
}

/// One email sign-up -> `Some("linked")`, or None with `t.error` naming why.
/// A hung browser ends every wait like a closed window, then fails the run with
/// `BROWSER_HUNG`. `proxy` (a URL) carries the key test.
pub async fn run(
    t: &mut TtsRun<'_>,
    urls: &Urls,
    tm: &Timings,
    proxy: Option<&str>,
) -> Option<&'static str> {
    let res = match run_inner(t, urls, tm, proxy).await {
        Ok(r) => r,
        Err(e) => {
            let msg = t.run_error(&e);
            t.fail(&msg);
            None
        }
    };
    if t.page.hung() {
        t.fail(crate::creator::web::BROWSER_HUNG);
        return None;
    }
    res
}

async fn run_inner(
    t: &mut TtsRun<'_>,
    urls: &Urls,
    tm: &Timings,
    proxy: Option<&str>,
) -> WebResult<Option<&'static str>> {
    if let Some(saved) = t.saved_key.clone() {
        // An earlier run's untested key: test it before making another.
        if t.test_key(&urls.key_test, &key_headers(&saved), proxy, &saved)
            .await
            == Some("linked")
            || t.untested_key.is_some()
        {
            return Ok(t.api_key.as_ref().map(|_| "linked"));
        }
        t.error = None; // refused: the account makes a new key below
    }
    if t.password.is_none() {
        t.fail("route not supported yet"); // Google route: M6
        return Ok(None);
    }
    for trigger in cookie_accept(t.page) {
        t.page.add_overlay(trigger);
    }
    let mut ok = false;
    if t.password_reused {
        // A rerun: the earlier run's session may still be signed in.
        t.goto(&urls.login).await?;
        ok = session_alive(t, urls, tm).await;
    }
    if !ok && !email_signup(t, urls, tm).await? {
        return Ok(None);
    }
    if t.wc() {
        t.closed_during("sign-in");
        return Ok(None);
    }
    t.step("signed in");
    let Some(key) = create_key(t, urls, tm).await else {
        return Ok(None);
    };
    Ok(t.test_key(&urls.key_test, &key_headers(&key), proxy, &key)
        .await)
}

async fn signed_in(t: &TtsRun<'_>, urls: &Urls) -> bool {
    let Ok(u) = url::Url::parse(&t.page.url().await) else {
        return false;
    };
    u.host_str().unwrap_or("").ends_with(&urls.host_suffix)
        && (u.path().starts_with("/app/") || u.path().starts_with("/welcome"))
}

/// A signed-in session on the sign-in page moves on by itself within `session_wait`.
async fn session_alive(t: &TtsRun<'_>, urls: &Urls, tm: &Timings) -> bool {
    let end = Instant::now() + tm.tts.session_wait;
    while !signed_in(t, urls).await {
        if Instant::now() >= end || t.wc() {
            return false;
        }
        tokio::time::sleep(tm.tts.click_poll).await;
    }
    true
}

/// After "Continue". A failed Turnstile gets one "Try again".
async fn outcome(t: &TtsRun<'_>, urls: &Urls, tm: &Timings) -> Got {
    let now = Instant::now;
    let mut end = now() + tm.submit_wait;
    let (mut tick_at, mut ticks) = (now() + tm.tick_grace, 0);
    let (mut retried, mut stale) = (false, false);
    loop {
        if t.wc() {
            return Got::Closed;
        }
        if signed_in(t, urls).await {
            return Got::In;
        }
        let body = t.body().await.replace('\u{2019}', "'");
        if body.contains(WRONG_PW_TEXT) {
            return Got::Wrong;
        }
        if body.contains(CHECK_MAIL_TEXT) {
            return Got::Mail;
        }
        let failed = TURNSTILE_FAILED.iter().any(|f| body.contains(f));
        if stale && !failed {
            stale = false; // the old failure text went away: the next one is the retry's own
        }
        if failed && stale {
            if now() >= end {
                return Got::Captcha;
            }
        } else if failed {
            if retried {
                return Got::Captcha;
            }
            retried = true;
            stale = true;
            t.warn("Turnstile failed, trying again");
            let _ = t
                .page
                .role("button", "Try again")
                .first()
                .click(CLICK)
                .await;
            end = now() + tm.submit_wait;
            (tick_at, ticks) = (now() + tm.tick_grace, 0); // a fresh widget
        } else if now() >= end {
            return if body.contains(TURNSTILE_TEXT) {
                Got::Captcha
            } else {
                Got::Stuck
            };
        } else if body.contains(TURNSTILE_TEXT)
            && ticks < tm.ticks
            && now() >= tick_at
            && cf::turnstile_pending(t.page, &urls.cf_host).await
        {
            ticks += 1;
            t.info(&format!(
                "Turnstile still pending, ticking it ({ticks}/{})",
                tm.ticks
            ));
            let wc = || t.wc();
            cf::press_turnstile(t.page, &urls.cf_host, tm.cf_draw, tm.cf_wait, &wc).await;
            tick_at = now() + tm.tick_grace;
            end = end.max(now() + tm.tick_settle);
            continue; // the press's own wait has passed: look at the page again now
        }
        tokio::time::sleep(tm.tts.click_poll).await;
    }
}

/// The /auth/login form with email + password -> what it led to. `reuse`: the
/// mailbox cutoff is set once the fill sticks, before the "Continue" that sends
/// the verify mail.
async fn continue_(t: &TtsRun<'_>, urls: &Urls, tm: &Timings, reuse: bool) -> WebResult<Got> {
    t.goto(&urls.login).await?;
    if signed_in(t, urls).await {
        return Ok(Got::In);
    }
    let poll = tm.tts.click_poll;
    let email = t.page.css("input[type=email]").first();
    let password = t.page.css("input[type=password]").first();
    if t.visible(&email, tm.field_wait, poll).await.is_none() {
        return Ok(if t.wc() {
            Got::Closed
        } else {
            Got::Fail("Breeze Blue sign-in form did not show".into())
        });
    }
    let pw = t.password.clone().unwrap_or_default();
    let pairs = [(&email, t.email.as_str()), (&password, pw.as_str())];
    if !t.fill(&pairs, poll).await {
        return Ok(Got::Fail("Breeze Blue sign-in form did not fill".into()));
    }
    if reuse {
        if let Some(mail) = t.mail {
            if let Err(e) = mail.reuse().await {
                return Ok(Got::Fail(format!("mail: {e}")));
            }
        }
        // The form can re-render while the mailbox is reused: fill it again.
        if !t.fill(&pairs, poll).await {
            return Ok(Got::Fail("Breeze Blue sign-in form did not fill".into()));
        }
    }
    // "Continue" does nothing until the page's sign-in configuration has loaded.
    let end = Instant::now() + tm.field_wait;
    while t.body().await.contains(CONFIG_LOADING) && Instant::now() < end && !t.wc() {
        tokio::time::sleep(poll).await;
    }
    if !t.submit(&CONTINUE_RE, &tm.tts).await {
        return Ok(Got::Fail(
            "Breeze Blue Continue button did not enable".into(),
        ));
    }
    Ok(outcome(t, urls, tm).await)
}

async fn email_signup(t: &mut TtsRun<'_>, urls: &Urls, tm: &Timings) -> WebResult<bool> {
    t.step("sign up by email");
    let mut got = continue_(t, urls, tm, true).await?;
    if got == Got::Stuck {
        // Seen: the form came back empty after Continue: a fresh form once more.
        got = continue_(t, urls, tm, false).await?;
    }
    match got {
        Got::In => return Ok(true), // an existing account with our password
        Got::Mail => {}
        Got::Closed => {
            t.closed_during("sign-up");
            return Ok(false);
        }
        Got::Wrong => {
            let e = t.exists_error(false, EMAIL_EXISTS, NO_SIGNIN);
            t.fail(e);
            return Ok(false);
        }
        Got::Captcha => {
            t.fail(CAPTCHA_UNSOLVED);
            return Ok(false);
        }
        Got::Stuck => {
            t.fail("Breeze Blue sign-up did not go through");
            return Ok(false);
        }
        Got::Fail(e) => {
            t.fail(&e);
            return Ok(false);
        }
    }

    t.step("verify mail");
    let Some(link) = t
        .wait_mail(VERIFY_SENDER, VERIFY_ACCEPT, tm.verify_mail)
        .await
    else {
        if t.wc() {
            t.closed_during("verify mail");
        } else if t.error.is_none() {
            t.fail(NO_VERIFY_MAIL);
        }
        return Ok(false);
    };
    t.goto(&link).await?;
    let end = Instant::now() + tm.verify_wait;
    while !signed_in(t, urls).await {
        if t.wc() {
            t.closed_during("verify");
            return Ok(false);
        }
        if Instant::now() >= end {
            t.step("sign in");
            if sign_in(t, urls, tm).await? == Some(Got::In) {
                return Ok(true);
            }
            t.fail(NO_SIGNIN);
            return Ok(false);
        }
        tokio::time::sleep(tm.tts.click_poll).await;
    }
    Ok(true)
}

/// An existing account, the row's password -> In | Wrong | None.
async fn sign_in(t: &TtsRun<'_>, urls: &Urls, tm: &Timings) -> WebResult<Option<Got>> {
    Ok(match continue_(t, urls, tm, false).await? {
        g @ (Got::In | Got::Wrong) => Some(g),
        _ => None,
    })
}

/// The /welcome survey of a new account: "Skip question" (enabled only: the last
/// one stays disabled while the survey saves) until "Start creating", within
/// `welcome_wait` (best effort).
async fn skip_welcome(t: &TtsRun<'_>, tm: &Timings) {
    let w = t.page.watch();
    let end = Instant::now() + tm.welcome_wait;
    while t.page.url().await.contains("/welcome") && Instant::now() < end && !t.wc() {
        for name in ["Start creating", "Skip question"] {
            let btn = t.page.role("button", name).first();
            if w.call(btn.count(), Ok(0)).await.unwrap_or(0) > 0
                && !w.call(btn.is_disabled(), true).await
            {
                let _ = btn.click(CLICK_SHORT).await;
                break;
            }
        }
        tokio::time::sleep(tm.tts.click_poll.max(tm.welcome_poll)).await;
    }
}

/// The "Create API key" button within `key_page_wait`; None at once when the app
/// sends us to /welcome (it does that after the key page has loaded).
async fn key_button(t: &TtsRun<'_>, tm: &Timings) -> Option<crate::creator::web::locator::Locator> {
    let btn = t.page.role("button", "Create API key");
    let end = Instant::now() + tm.key_page_wait;
    while !t.page.url().await.contains("/welcome") {
        let hit = t.page.watch().call(btn.first_visible(), Ok(None)).await;
        if let Ok(Some(hit)) = hit {
            return Some(hit);
        }
        if Instant::now() >= end || t.page.hung() {
            return None;
        }
        tokio::time::sleep(tm.tts.click_poll).await;
    }
    None
}

async fn create_key(t: &mut TtsRun<'_>, urls: &Urls, tm: &Timings) -> Option<String> {
    t.step("api key");
    let made = make_key(t, urls, tm).await;
    // Off the screen right away: the key never stays visible in the window.
    let w = t.page.watch();
    let done = t.page.role("button", "Done");
    if w.call(done.count(), Ok(0)).await.unwrap_or(0) > 0 {
        let _ = w.call(done.first().click(CLICK_SHORT), Ok(())).await;
    }
    let _ = w.call(t.page.goto("about:blank"), Ok(())).await;
    match made {
        Ok(key) => key,
        Err(e) => {
            t.fail(&format!("{NO_KEY}: {}", tts::kind(&e)));
            None
        }
    }
}

/// Ok(None): already failed with its reason.
async fn make_key(t: &mut TtsRun<'_>, urls: &Urls, tm: &Timings) -> WebResult<Option<String>> {
    // The survey also takes over the key page a few seconds after it has loaded:
    // get it out of the way first.
    t.goto(&urls.welcome).await?;
    skip_welcome(t, tm).await;
    t.goto(&urls.keys).await?;
    if t.page.url().await.contains("/auth/login") {
        // The URL-only signed-in check was wrong.
        if t.password.is_none() {
            t.fail(NO_SIGNIN);
            return Ok(None);
        }
        t.step("sign in");
        if sign_in(t, urls, tm).await? != Some(Got::In) {
            t.fail(NO_SIGNIN);
            return Ok(None);
        }
        t.goto(&urls.keys).await?;
    }
    let mut create = key_button(t, tm).await;
    if create.is_none() && t.page.url().await.contains("/welcome") {
        // The survey shows once the key page opens.
        skip_welcome(t, tm).await;
        t.goto(&urls.keys).await?;
        create = key_button(t, tm).await;
    }
    let Some(create) = create else {
        t.fail(NO_KEY);
        return Ok(None);
    };
    create.click(CLICK).await?;
    let dialog = t.page.role("dialog", None);
    let name = dialog.css("input[name=name]");
    let poll = tm.tts.click_poll;
    if t.visible(&name, tm.field_wait, poll).await.is_none()
        || !t.fill(&[(&name, KEY_NAME)], poll).await
    {
        t.fail(NO_KEY);
        return Ok(None);
    }
    dialog.role("button", "Create key").click(CLICK).await?;
    let end = Instant::now() + tm.field_wait;
    while !t.page.hung() {
        let texts: Vec<String> = t
            .page
            .watch()
            .call(t.page.eval(CODES_JS, World::Isolated), Ok(vec![]))
            .await
            .unwrap_or_default();
        if let Some(key) = texts.into_iter().find(|s| KEY_RE.is_match(s)) {
            return Ok(Some(key));
        }
        if Instant::now() >= end {
            break;
        }
        tokio::time::sleep(poll).await;
    }
    t.fail(NO_KEY);
    Ok(None)
}

/// The runner spawns a sign-up on tokio: it must stay `Send` (compile-time check).
#[allow(dead_code)]
fn run_is_send(t: &'static mut TtsRun<'static>, u: &'static Urls, tm: &'static Timings) {
    fn send<T: Send>(_: T) {}
    send(run(t, u, tm, None));
}

#[cfg(test)]
mod tests;
