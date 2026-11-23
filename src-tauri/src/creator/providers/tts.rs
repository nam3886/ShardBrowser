//! Shared sign-up steps of the voice platforms (Python `providers/tts_base.py`
//! `TtsSignup`): goto, fill, submit, wait for mail, test the key.
//!
//! Never in a log line, step note or error: the email address, password, mail
//! code/link or API key.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::future::BoxFuture;
use regex::Regex;

use crate::creator::web::input::accept_cookies;
use crate::creator::web::locator::Locator;
use crate::creator::web::{Page, WebError, BROWSER_HUNG};

pub const NO_KEY: &str = "No API key shown";
pub const KEY_TEST_FAILED: &str = "API key test failed";
const KEY_TEST_TIMEOUT: Duration = Duration::from_secs(30);
const KEY_CHECK_TIMEOUT: Duration = Duration::from_secs(20);
const FILL_TIMEOUT: Duration = Duration::from_secs(10);
const CLICK_TIMEOUT: Duration = Duration::from_secs(8);

/// Stored as `info` / `warn` / `error` in `events.level`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl Level {
    pub fn as_str(self) -> &'static str {
        match self {
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
        }
    }
}

/// Python `tts_base` knobs; tests shrink them.
#[derive(Clone, Copy, Debug)]
pub struct Timings {
    /// After a cookie banner, before the submit click.
    pub settle: Duration,
    /// A submit button enabled within this.
    pub submit_enable: Duration,
    pub click_poll: Duration,
    /// A signed-in session leaves the sign-in page within this.
    pub session_wait: Duration,
}

impl Default for Timings {
    fn default() -> Self {
        Timings {
            settle: Duration::from_secs(3),
            submit_enable: Duration::from_secs(30),
            click_poll: Duration::from_millis(500),
            session_wait: Duration::from_secs(10),
        }
    }
}

/// The account's mailbox, as the flows use it. Errors are the mail client's own
/// texts (no URLs, tokens or keys). Task 12 implements it on the Ele client.
pub trait Inbox: Send + Sync {
    /// Python `reuse_mailbox`: mail stored so far goes stale, the cutoff is now.
    fn reuse(&self) -> BoxFuture<'_, Result<(), String>>;
    /// Python `wait_for_new_mail(…, link_pick(sender_has, accept), stop=stop)`:
    /// the first new link from that sender, None at the deadline or once `stop`.
    fn wait_link<'b>(
        &'b self,
        sender_has: &'b str,
        accept: &'b [&'b str],
        timeout: Duration,
        stop: &'b (dyn Fn() -> bool + Send + Sync),
    ) -> BoxFuture<'b, Result<Option<String>, String>>;
}

type StepFn<'a> = Box<dyn Fn(&str) + Send + Sync + 'a>;
type LogFn<'a> = Box<dyn Fn(Level, &str) + Send + Sync + 'a>;

/// One sign-up on one page. Walks the pages and never persists: the runner owns
/// persistence. `password: None` means the Google route.
pub struct TtsRun<'a> {
    pub page: &'a Page,
    pub key: &'static str,
    pub email: String,
    pub password: Option<String>,
    /// The password is the row's saved one (an earlier run made the account).
    pub password_reused: bool,
    pub mail: Option<&'a dyn Inbox>,
    pub error: Option<String>,
    pub api_key: Option<String>,
    /// Made here, but its test said nothing about it: the runner keeps it.
    pub untested_key: Option<String>,
    /// Set by the runner: a key an earlier run made but could not test.
    pub saved_key: Option<String>,
    pub last_step: Option<String>,
    pub on_step: StepFn<'a>,
    pub log: LogFn<'a>,
    /// The window was closed (the runner sets it).
    pub closed: Arc<AtomicBool>,
}

impl<'a> TtsRun<'a> {
    pub fn new(page: &'a Page, key: &'static str, email: String) -> Self {
        TtsRun {
            page,
            key,
            email,
            password: None,
            password_reused: false,
            mail: None,
            error: None,
            api_key: None,
            untested_key: None,
            saved_key: None,
            last_step: None,
            on_step: Box::new(|_| {}),
            log: Box::new(|_, _| {}),
            closed: Arc::default(),
        }
    }

    /// The window closed or the browser hung: every wait ends.
    pub fn wc(&self) -> bool {
        self.closed.load(Ordering::SeqCst) || self.page.hung()
    }

    /// Record a failure. The caller keeps secrets out of `msg`.
    pub fn fail(&mut self, msg: &str) {
        if self.page.hung() && msg != BROWSER_HUNG {
            return; // a hung browser is reported once, by run
        }
        self.error = Some(msg.to_string());
        (self.log)(Level::Error, msg);
    }

    pub fn closed_during(&mut self, at: &str) {
        self.fail(&format!("window closed during {at}"));
    }

    pub fn step(&mut self, note: &str) {
        self.last_step = Some(note.to_string());
        (self.on_step)(note);
        (self.log)(Level::Info, &format!("{}: {note}", self.key));
    }

    pub fn warn(&self, msg: &str) {
        (self.log)(Level::Warn, &format!("{}: {msg}", self.key));
    }

    pub fn info(&self, msg: &str) {
        (self.log)(Level::Info, &format!("{}: {msg}", self.key));
    }

    /// `Page::goto` (it retries navigation errors itself); the URL is never logged.
    pub async fn goto(&self, url: &str) -> Result<(), WebError> {
        self.page.goto(url).await
    }

    /// The first visible match within `wait`.
    pub async fn visible(&self, loc: &Locator, wait: Duration, poll: Duration) -> Option<Locator> {
        let end = Instant::now() + wait;
        loop {
            let hit = self
                .page
                .watch()
                .call(loc.first_visible(), Ok(None))
                .await
                .unwrap_or(None);
            if hit.is_some() || Instant::now() >= end || self.page.hung() {
                return hit;
            }
            tokio::time::sleep(poll).await;
        }
    }

    /// The page's text, lowercased ("" on failure).
    pub async fn body(&self) -> String {
        self.page.body_text().await.to_lowercase()
    }

    /// Fill every (locator, value), again until all values stick (React can drop
    /// an early fill), at most 5 rounds. A field that already holds its value is
    /// left alone (Playwright's fill was instant; typing it again costs seconds).
    pub async fn fill(&self, pairs: &[(&Locator, &str)], poll: Duration) -> bool {
        for _ in 0..5 {
            let mut typed = true;
            for (loc, value) in pairs {
                if self.value_of(loc).await == *value {
                    continue;
                }
                if loc.fill(value, FILL_TIMEOUT).await.is_err() {
                    typed = false;
                    break;
                }
            }
            if typed {
                tokio::time::sleep(poll).await;
                let mut all = true;
                for (loc, value) in pairs {
                    all &= self.value_of(loc).await == *value;
                }
                if all {
                    return true;
                }
            }
            if self.page.hung() {
                break;
            }
        }
        false
    }

    async fn value_of(&self, loc: &Locator) -> String {
        let got = self.page.watch().call(loc.input_value(), Ok(String::new()));
        got.await.unwrap_or_default()
    }

    /// The last button named `name`, clicked once enabled (within
    /// `submit_enable`); a covered one gets a JS click. A cookie banner is
    /// accepted first, then `settle`.
    pub async fn submit(&self, name: &Regex, tm: &Timings) -> bool {
        if accept_cookies(self.page).await {
            tokio::time::sleep(tm.settle).await;
        }
        let w = self.page.watch();
        let btn = self.page.role("button", name.clone()).last();
        let end = Instant::now() + tm.submit_enable;
        while !self.page.hung() {
            if w.call(btn.count(), Ok(0)).await.unwrap_or(0) > 0
                && !w.call(btn.is_disabled(), true).await
            {
                match btn.click(CLICK_TIMEOUT).await {
                    Ok(()) => return true,
                    // Only a covered button: a JS click is untrusted, so never for a
                    // hidden button, a navigation or a timeout of its own.
                    Err(WebError::Covered) => {
                        if w.call(btn.js_click(), Err(WebError::Hung)).await.is_ok() {
                            return true;
                        }
                    }
                    Err(_) => {}
                }
            }
            if Instant::now() >= end {
                break;
            }
            tokio::time::sleep(tm.click_poll).await;
        }
        false
    }

    /// The verify link (Python `_wait_mail` with a `link_pick`); a mail error fails the run.
    pub async fn wait_mail(
        &mut self,
        sender_has: &str,
        accept: &[&str],
        timeout: Duration,
    ) -> Option<String> {
        let mail = self.mail?;
        let (closed, watch) = (self.closed.clone(), self.page.watch().clone());
        let stop = move || closed.load(Ordering::SeqCst) || watch.hung();
        match mail.wait_link(sender_has, accept, timeout, &stop).await {
            Ok(link) => link,
            Err(e) => {
                self.fail(&format!("mail: {e}"));
                None
            }
        }
    }

    /// Fatal only when the password is surely not ours: the page said so, or no
    /// earlier run saved it.
    pub fn exists_error(
        &self,
        page_said_wrong: bool,
        exists: &'static str,
        no_signin: &'static str,
    ) -> &'static str {
        if page_said_wrong || !self.password_reused {
            exists
        } else {
            no_signin
        }
    }

    /// GET the key test URL through the account's proxy: 200 -> linked; 401/403
    /// refuse the key; anything else (or a network error) keeps it untested.
    pub async fn test_key(
        &mut self,
        url: &str,
        headers: &[(&str, String)],
        proxy: Option<&str>,
        key: &str,
    ) -> Option<&'static str> {
        self.step("key test");
        let status = match get_status(url, headers, proxy, KEY_TEST_TIMEOUT).await {
            Ok(s) => s,
            Err(class) => {
                self.untested_key = Some(key.to_string());
                self.fail(&format!("{KEY_TEST_FAILED}: {class}"));
                return None;
            }
        };
        if status != 200 {
            if status != 401 && status != 403 {
                self.untested_key = Some(key.to_string());
            }
            self.fail(&format!("{KEY_TEST_FAILED}: HTTP {status}"));
            return None;
        }
        self.api_key = Some(key.to_string());
        self.step("linked");
        Some("linked")
    }

    /// A page error that ended the run -> its text (Python `_run_error`): a
    /// navigation that never loaded -> `page load: …`, a step's own wait ->
    /// `<step>: TimeoutError`, else `run: <kind>`.
    pub fn run_error(&self, e: &WebError) -> String {
        match e {
            WebError::Nav(code) => format!("page load: {code}"),
            WebError::Timeout("navigation") => "page load: TimeoutError".into(),
            WebError::Timeout(_) => match &self.last_step {
                Some(s) => format!("{s}: TimeoutError"),
                None => "run: TimeoutError".into(),
            },
            other => format!("run: {}", kind(other)),
        }
    }
}

/// A class-like name for an error (Python's `type(e).__name__`).
pub fn kind(e: &WebError) -> &'static str {
    match e {
        WebError::Timeout(_) => "TimeoutError",
        WebError::Hung => "Hung",
        WebError::NotFound => "NotFound",
        WebError::Covered => "Covered",
        WebError::Disabled => "Disabled",
        WebError::Nav(_) => "NavError",
        WebError::Cdp(_) => "CdpError",
    }
}

/// What a key check said (Python `check_key_with`: 'active' | 'signed_out' | 'unknown').
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KeyState {
    Active,
    SignedOut,
    Unknown,
}

/// GET `url` with `headers` through `proxy`: 200 -> Active, 401/403 -> SignedOut,
/// anything else -> Unknown.
pub async fn check_key(url: &str, headers: &[(&str, String)], proxy: Option<&str>) -> KeyState {
    match get_status(url, headers, proxy, KEY_CHECK_TIMEOUT).await {
        Ok(200) => KeyState::Active,
        Ok(401 | 403) => KeyState::SignedOut,
        _ => KeyState::Unknown,
    }
}

/// The status of one GET, or the error's kind (never its text: it can hold the URL).
async fn get_status(
    url: &str,
    headers: &[(&str, String)],
    proxy: Option<&str>,
    timeout: Duration,
) -> Result<u16, &'static str> {
    let mut b = reqwest::Client::builder().timeout(timeout);
    if let Some(p) = proxy {
        b = b.proxy(reqwest::Proxy::all(p).map_err(|_| "ProxyError")?);
    }
    let client = b.build().map_err(|_| "ClientError")?;
    let mut req = client.get(url);
    for (k, v) in headers {
        req = req.header(*k, v);
    }
    match req.send().await {
        Ok(r) => Ok(r.status().as_u16()),
        Err(e) if e.is_timeout() => Err("TimeoutError"),
        Err(e) if e.is_connect() => Err("ConnectError"),
        Err(_) => Err("RequestError"),
    }
}
