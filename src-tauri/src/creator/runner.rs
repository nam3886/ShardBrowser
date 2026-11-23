//! One Breeze Blue account end to end, and what it leaves in our store
//! (Python `engine/account_service.py` for a voice platform): its mailbox,
//! browser and account rows, the password saved before the page opens, the
//! key, status, error and error code, a failure screenshot, the proxy's
//! sign-up count and an `events` row for every step and log line.
//!
//! Fully automatic: no step waits for the person. Never in an event, error or
//! step note: the email address, password, mailbox token, verify link, API
//! key, Ele key or a proxy password (`Scrub`).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::SeqCst};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use futures_util::future::BoxFuture;
use rand::seq::SliceRandom;
use rand::Rng;
use serde_json::{json, Value};

use super::errors::error_code;
use super::mail::{link_pick, wait_for_new_mail, EleClient, POLL_INTERVAL};
use super::providers::breezeblue::{self as bb, Timings, Urls, KEY};
use super::providers::tts::{Inbox, TtsRun};
use super::proxy::{self, NoHealthyProxy};
use super::shard::{self, Opened};
use super::store::{self, now, AccountPatch, Db, Mailbox, Proxy};

/// Python `account_service.INTERRUPTED` (`shard::failure_text`'s text).
pub const INTERRUPTED: &str = shard::INTERRUPTED;
pub const NO_BROWSER: &str = "No browser";
pub const NO_ADDRESS: &str = "Email has no address";
/// No usable proxy for a new account's browser.
pub const NO_PROXY: &str = "No healthy proxy for a new browser";
/// The settings our pages read and write; the Ele key is never read back.
const SETTINGS: [&str; 3] = ["ele_api_url", "ele_api_key", "proxy_rest_seconds"];
const SECRET_SETTING: &str = "ele_api_key";

/// How the run opens, checks and stops an account's browser: Shard's profiles
/// in the app (`Shard`), a bare test engine in tests.
pub trait Browsers: Send + Sync {
    fn open<'a>(
        &'a self,
        db: &'a Db,
        browser_id: i64,
        proxy: Option<&'a Proxy>,
    ) -> BoxFuture<'a, Result<Opened>>;
    /// The error of a run that did not link (`INTERRUPTED` once the browser is gone).
    fn failure_text(&self, profile_id: &str, error: String) -> String;
    fn stop<'a>(&'a self, profile_id: &'a str) -> BoxFuture<'a, ()>;
}

/// Our accounts' Shard profiles (`shard::open_session`).
pub struct Shard;

impl Browsers for Shard {
    fn open<'a>(
        &'a self,
        db: &'a Db,
        browser_id: i64,
        proxy: Option<&'a Proxy>,
    ) -> BoxFuture<'a, Result<Opened>> {
        Box::pin(shard::open_session(db, browser_id, proxy))
    }

    fn failure_text(&self, profile_id: &str, error: String) -> String {
        shard::failure_text(profile_id, error)
    }

    fn stop<'a>(&'a self, profile_id: &'a str) -> BoxFuture<'a, ()> {
        Box::pin(async move {
            let _ = shard::stop(profile_id).await;
        })
    }
}

/// What a run needs.
pub struct Ctx<B> {
    pub db: Arc<Db>,
    pub ele: EleClient,
    pub browsers: B,
    pub urls: Urls,
    pub tm: Timings,
    /// Failure screenshots go here as `<account_id>.png`.
    pub screens: PathBuf,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    /// "linked" | "failed".
    pub status: &'static str,
    pub error: Option<String>,
}

fn failed(error: String) -> Outcome {
    Outcome {
        status: "failed",
        error: Some(error),
    }
}

/// Values masked as `***` before any text is stored.
#[derive(Default)]
struct Scrub(Mutex<Vec<String>>);

impl Scrub {
    /// The store's secrets: the Ele key and every proxy password (Python `store.secrets()`).
    fn of(db: &Db) -> Scrub {
        let s = Scrub::default();
        if let Ok(Some(k)) = db.settings_get(SECRET_SETTING) {
            s.add(&k);
        }
        for p in db.proxies_list().unwrap_or_default() {
            s.add(p.password.as_deref().unwrap_or(""));
        }
        s
    }

    /// Under 4 characters is not masked (it would garble every message), as
    /// in Python. Its URL-encoded forms are too: a proxy URL carries the
    /// password quoted (`proxy_url`), a form body with `+` for spaces.
    fn add(&self, v: &str) {
        if v.chars().count() < 4 {
            return;
        }
        let quoted = url::form_urlencoded::byte_serialize(v.as_bytes());
        let mut forms = vec![v.to_string(), quoted.collect()];
        if let Ok(mut u) = url::Url::parse("http://h") {
            if u.set_password(Some(v)).is_ok() {
                forms.extend(u.password().map(str::to_string));
            }
        }
        let mut all = self.0.lock().unwrap();
        for f in forms {
            if !all.contains(&f) {
                all.push(f);
            }
        }
    }

    fn clean(&self, msg: &str) -> String {
        let mut all = self.0.lock().unwrap().clone();
        all.sort_by_key(|s| std::cmp::Reverse(s.len()));
        all.iter().fold(msg.to_string(), |m, s| m.replace(s, "***"))
    }
}

/// Python `elevenlabs.new_password`: 14 characters, letters and digits with at
/// least one digit and one of `!@#$%`, shuffled.
pub fn new_password() -> String {
    const AN: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::thread_rng();
    loop {
        let mut c = vec![
            b'0' + rng.gen_range(0..10),
            *b"!@#$%".choose(&mut rng).unwrap(),
        ];
        c.extend((0..12).map(|_| *AN.choose(&mut rng).unwrap()));
        c.shuffle(&mut rng);
        if c.iter().any(u8::is_ascii_alphabetic) {
            return String::from_utf8(c).unwrap();
        }
    }
}

/// `http://user:pass@host:port` (credentials quoted). Carries the password: never log it.
fn proxy_url(p: &Proxy) -> String {
    let plain = format!("http://{}:{}", p.host, p.port);
    let Ok(mut u) = url::Url::parse(&plain) else {
        return plain;
    };
    if let Some(user) = p.username.as_deref().filter(|u| !u.is_empty()) {
        let _ = u.set_username(user);
        let _ = u.set_password(p.password.as_deref());
    }
    u.to_string()
}

/// The account's Ele mailbox as the flow's `Inbox`. A link it hands out is
/// masked from then on.
struct EleInbox<'a> {
    ele: &'a EleClient,
    mailbox: Mailbox,
    scrub: &'a Scrub,
}

impl Inbox for EleInbox<'_> {
    fn reuse(&self) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(async move { self.ele.reuse_mailbox(&self.mailbox).await.map_err(|e| e.0) })
    }

    fn wait_link<'b>(
        &'b self,
        sender_has: &'b str,
        accept: &'b [&'b str],
        timeout: Duration,
        stop: &'b (dyn Fn() -> bool + Send + Sync),
    ) -> BoxFuture<'b, Result<Option<String>, String>> {
        Box::pin(async move {
            let pick = link_pick(sender_has, accept, &[]);
            #[cfg(test)]
            let pick = tests::loopback_http(pick);
            let link = wait_for_new_mail(
                self.ele,
                &self.mailbox,
                timeout,
                pick,
                POLL_INTERVAL,
                Some(stop),
            )
            .await
            .map_err(|e| e.0)?;
            if let Some(l) = &link {
                self.scrub.add(l);
            }
            Ok(link)
        })
    }
}

/// The final row write: status, error and its code (recomputed from the
/// error, NULL with it), finished_at; linked -> verified now; Interrupted ->
/// also the step note.
fn finish(db: &Db, id: i64, status: &str, error: Option<String>, shot: Option<String>) {
    let linked = status == "linked";
    let p = AccountPatch {
        status: Some(status.into()),
        error_code: Some(error.as_deref().and_then(error_code).map(str::to_string)),
        step_note: (error.as_deref() == Some(INTERRUPTED)).then(|| Some(INTERRUPTED.into())),
        error: Some(error),
        finished_at: Some(Some(now())),
        verify_status: linked.then(|| Some("active".into())),
        verified_at: linked.then(|| Some(now())),
        screenshot: shot.map(Some),
        ..Default::default()
    };
    let _ = db.account_update(id, &p);
}

/// An error that ended a run: its first line, masked, then cut to 120
/// characters (Python `err_line`: masked BEFORE the cut).
fn err_line(e: &anyhow::Error, scrub: &Scrub) -> String {
    if e.is::<NoHealthyProxy>() {
        return NO_PROXY.into();
    }
    let text = format!("{e:#}");
    let line = scrub.clean(text.lines().next().unwrap_or(""));
    line.chars().take(120).collect()
}

/// Run account `id` (its row, mailbox and browser exist) and persist the
/// result on its row.
// ponytail: no watcher for a browser Shard closes mid-run: the flow ends on
// its own errors, then `failure_text` reads "Interrupted". Add one with the
// queue (M5). Same for the startup sweep of rows a crash left "running".
pub async fn run_account<B: Browsers>(cx: &Ctx<B>, id: i64) -> Outcome {
    let scrub = Scrub::of(&cx.db);
    let (status, error, shot) = match attempt(cx, id, &scrub).await {
        Ok(r) => r,
        Err(e) => ("failed", Some(err_line(&e, &scrub)), None),
    };
    let error = error.map(|e| scrub.clean(&e));
    finish(&cx.db, id, status, error.clone(), shot);
    Outcome { status, error }
}

type Ended = (&'static str, Option<String>, Option<String>);

async fn attempt<B: Browsers>(cx: &Ctx<B>, id: i64, scrub: &Scrub) -> Result<Ended> {
    let db = &*cx.db;
    let acc = db
        .account_get(id)?
        .ok_or_else(|| anyhow!("account {id} not found"))?;
    let mb = db
        .mailbox_get(acc.mailbox_id)?
        .filter(|m| !m.email.trim().is_empty())
        .ok_or_else(|| anyhow!(NO_ADDRESS))?;
    scrub.add(&mb.email);
    scrub.add(&mb.token);
    let br = db
        .browser_of_mailbox(mb.id)?
        .ok_or_else(|| anyhow!(NO_BROWSER))?;
    // One password per row, saved before the page opens (an earlier run's is reused).
    let (password, reused) = match db.account_secret(id, "password")? {
        Some(p) => (p, true),
        None => {
            let p = new_password();
            db.account_set_secret(id, "password", &p)?;
            (p, false)
        }
    };
    scrub.add(&password);
    // A key an earlier run made but could not test: tested first, no other made.
    let saved_key = db.account_secret(id, "api_key")?;
    scrub.add(saved_key.as_deref().unwrap_or(""));
    let proxy = proxy::browser_proxy(db, br.id, "")?;
    db.account_update(
        id,
        &AccountPatch {
            status: Some("running".into()),
            step: Some(Some("browser".into())),
            step_note: Some(None),
            error: Some(None),
            error_code: Some(None),
            started_at: Some(Some(now())),
            ..Default::default()
        },
    )?;
    let opened = cx.browsers.open(db, br.id, proxy.as_ref()).await?;
    // Nothing below may skip the stop at the end: no `?` from here on.
    let (bid, mid) = (Some(br.id), Some(mb.id));
    let _ = db.event(bid, mid, "info", Some("browser"), "browser opened");

    let inbox = EleInbox {
        ele: &cx.ele,
        mailbox: mb.clone(),
        scrub,
    };
    let mut t = TtsRun::new(&opened.page, KEY, mb.email.clone());
    t.password = Some(password);
    t.password_reused = reused;
    t.saved_key = saved_key;
    t.mail = Some(&inbox);
    t.on_step = Box::new(move |note| {
        let p = AccountPatch {
            step: Some(Some(KEY.into())),
            step_note: Some(Some(scrub.clean(note))),
            ..Default::default()
        };
        let _ = db.account_update(id, &p);
    });
    t.log = Box::new(move |level, msg| {
        let _ = db.event(bid, mid, level.as_str(), Some(KEY), &scrub.clean(msg));
    });
    let purl = proxy.as_ref().map(proxy_url);
    let linked = bb::run(&mut t, &cx.urls, &cx.tm, purl.as_deref()).await == Some("linked");
    let (api_key, untested, flow_error) = (t.api_key.take(), t.untested_key.take(), t.error.take());
    drop(t);

    // Keys first: nothing below can lose one.
    let error = if linked {
        if let Some(k) = &api_key {
            scrub.add(k);
            let _ = db.account_set_secret(id, "api_key", k);
            let _ = db.account_set_extra(
                id,
                &json!({ "api_key_at": now(), "api_key_untested": false }),
            );
        }
        None
    } else {
        // Made, but its test said nothing about it: never orphan it.
        if let Some(k) = &untested {
            scrub.add(k);
            let _ = db.account_set_secret(id, "api_key", k);
            let _ = db.account_set_extra(
                id,
                &json!({ "api_key_at": now(), "api_key_untested": true }),
            );
        }
        // Before our own stop, or every failure would read "Interrupted".
        let e = flow_error.unwrap_or_else(|| "signup failed".into());
        Some(cx.browsers.failure_text(&opened.profile_id, e))
    };
    let mut shot = None;
    if error.is_some() && error.as_deref() != Some(INTERRUPTED) {
        // Unmasked on purpose (Python, owner 2026-11-21): the page may show the address.
        let path = cx.screens.join(format!("{id}.png"));
        if std::fs::create_dir_all(&cx.screens).is_ok()
            && opened.page.screenshot(&path).await.is_ok()
        {
            shot = Some(path.display().to_string());
        }
    }
    if let Some(p) = &proxy {
        let _ = db.proxy_signup(p.id, linked);
    }
    cx.browsers.stop(&opened.profile_id).await;
    Ok((if linked { "linked" } else { "failed" }, error, shot))
}

/// A Make run's progress; one run at a time.
#[derive(Default)]
pub struct RunState {
    running: AtomicBool,
    done: AtomicU32,
    total: AtomicU32,
    /// "Stop after this account": checked between accounts only.
    pub stop: AtomicBool,
}

impl RunState {
    pub fn begin(&self, total: u32) -> Result<()> {
        if self
            .running
            .compare_exchange(false, true, SeqCst, SeqCst)
            .is_err()
        {
            bail!("a run is in progress");
        }
        self.done.store(0, SeqCst);
        self.total.store(total, SeqCst);
        self.stop.store(false, SeqCst);
        Ok(())
    }

    pub fn end(&self) {
        self.running.store(false, SeqCst);
    }

    pub fn json(&self) -> Value {
        json!({
            "running": self.running.load(SeqCst),
            "done": self.done.load(SeqCst),
            "total": self.total.load(SeqCst),
        })
    }
}

/// The app's one run.
pub static RUN: LazyLock<RunState> = LazyLock::new(RunState::default);

/// `count` new Breeze Blue accounts, one after another, under one `jobs` row.
/// Per account, in this order: an Ele mailbox (its row saved inside), a
/// proxy, the browser row, the account row (running), then `run_account`.
pub async fn make_breeze<B: Browsers>(
    cx: &Ctx<B>,
    count: u32,
    proxy_mode: &str,
    run: &RunState,
) -> Vec<Outcome> {
    let job = match cx.db.job_create(KEY, count.into(), proxy_mode) {
        Ok(j) => j,
        Err(e) => return vec![failed(format!("{e:#}"))],
    };
    let mut out = Vec::new();
    for _ in 0..count {
        if run.stop.load(SeqCst) {
            break;
        }
        let o = new_account(cx, job, proxy_mode).await;
        let no_proxy = o.error.as_deref() == Some(NO_PROXY);
        out.push(o);
        run.done.fetch_add(1, SeqCst);
        if no_proxy {
            break; // an empty pool: more accounts would only burn Ele mailboxes
        }
    }
    out
}

async fn new_account<B: Browsers>(cx: &Ctx<B>, job: i64, mode: &str) -> Outcome {
    let db = &*cx.db;
    let mb = match cx.ele.create_mailbox().await {
        Ok(m) => m,
        Err(e) => {
            let _ = db.event(None, None, "error", Some("mail"), &e.0);
            return failed(e.0);
        }
    };
    let picked = proxy::pick_proxy(db, mode, "");
    let rows = (|| -> Result<i64> {
        let proxy_id = picked.as_ref().ok().and_then(|p| p.as_ref().map(|p| p.id));
        db.browser_create(mb.id, proxy_id, mode)?;
        let id = db.account_create(mb.id, KEY, "email", Some(job))?;
        db.account_update(
            id,
            &AccountPatch {
                status: Some("running".into()),
                started_at: Some(Some(now())),
                ..Default::default()
            },
        )?;
        Ok(id)
    })();
    let scrub = Scrub::of(db);
    let id = match rows {
        Ok(id) => id,
        Err(e) => return failed(err_line(&e, &scrub)),
    };
    if let Err(e) = picked {
        // ponytail: a rerun of this account finds its browser without a proxy
        // and fails "proxy deleted" (PROXY_UNUSABLE), not NO_PROXY; reruns are M5.
        let error = err_line(&e, &scrub);
        finish(db, id, "failed", Some(error.clone()), None);
        return failed(error);
    }
    run_account(cx, id).await
}

static DB: Mutex<Option<Arc<Db>>> = Mutex::new(None);

/// Our store (`store::db_path`), opened once.
pub fn db() -> Result<Arc<Db>> {
    let mut g = DB.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(db) = &*g {
        return Ok(db.clone());
    }
    let path = store::db_path()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let db = Arc::new(Db::open(&path)?);
    *g = Some(db.clone());
    Ok(db)
}

/// Ends the app's run when the spawned task ends, a panic included.
struct EndRun;

impl Drop for EndRun {
    fn drop(&mut self) {
        RUN.end();
    }
}

/// Start a Make run in the background (`breeze_run`).
pub fn start(count: u32, proxy_mode: String) -> Result<()> {
    if !(1..=20).contains(&count) {
        bail!("count must be 1-20");
    }
    if !["none", "rotate", "least"].contains(&proxy_mode.as_str()) {
        bail!("unknown proxy mode: {proxy_mode}");
    }
    let db = db()?;
    let setting = |k| db.settings_get(k).map(Option::unwrap_or_default);
    let ele = EleClient::new(
        &setting("ele_api_url")?,
        &setting(SECRET_SETTING)?,
        db.clone(),
    )?;
    let screens = crate::store::local_root()?.join("creator").join("screens");
    let cx = Ctx {
        db,
        ele,
        browsers: Shard,
        urls: Urls::live(),
        tm: Timings::default(),
        screens,
    };
    RUN.begin(count)?;
    tokio::spawn(async move {
        let _end = EndRun;
        make_breeze(&cx, count, &proxy_mode, &RUN).await;
    });
    Ok(())
}

fn arg<'a>(args: &'a Value, k: &str) -> Result<&'a str> {
    args[k].as_str().ok_or_else(|| anyhow!("{k} required"))
}

/// Our commands (`creator_call`); None for a command that is not ours here.
pub async fn call(cmd: &str, args: &Value) -> Option<Result<Value>> {
    Some(match cmd {
        "settings_get" => db().and_then(|db| settings_get(&db, args)),
        "settings_set" => db().and_then(|db| settings_set(&db, args)),
        "import_basics" => (|| {
            let n = store::import_basics(&*db()?, std::path::Path::new(arg(args, "path")?))?;
            Ok(
                json!({ "proxies": n.proxies, "blocked_domains": n.blocked_domains, "settings": n.settings }),
            )
        })(),
        "breeze_run" => {
            let count = args["count"].as_u64().unwrap_or(0).min(u32::MAX.into()) as u32;
            let mode = args["proxy_mode"].as_str().unwrap_or("rotate").to_string();
            start(count, mode).map(|()| Value::Null)
        }
        "breeze_stop" => {
            RUN.stop.store(true, SeqCst);
            Ok(Value::Null)
        }
        "run_state" => Ok(RUN.json()),
        "accounts" => db().and_then(|db| accounts(&db, args)),
        "events" => (|| {
            let id = args["account_id"]
                .as_i64()
                .ok_or_else(|| anyhow!("account_id required"))?;
            let rows = db()?.events_of(id)?;
            Ok(Value::from(
                rows.iter()
                    .map(|e| {
                        json!({ "id": e.id, "level": e.level, "step": e.step,
                                "message": e.message, "created_at": e.created_at })
                    })
                    .collect::<Vec<_>>(),
            ))
        })(),
        _ => return None,
    })
}

/// The Accounts list: `platform` (Breeze Blue by default), `limit` 1..1000 (100).
fn accounts(db: &Db, args: &Value) -> Result<Value> {
    let platform = args["platform"].as_str().unwrap_or(KEY);
    let limit = args["limit"].as_i64().unwrap_or(100).clamp(1, 1000);
    Ok(Value::from(db.account_rows(platform, limit)?))
}

/// `{key: value}` for `keys`; the Ele key only says whether it is set.
fn settings_get(db: &Db, args: &Value) -> Result<Value> {
    let mut out = serde_json::Map::new();
    for k in args["keys"].as_array().into_iter().flatten() {
        let k = k.as_str().unwrap_or("");
        if !SETTINGS.contains(&k) {
            bail!("unknown setting: {k}");
        }
        let v = db.settings_get(k)?;
        out.insert(
            k.into(),
            if k == SECRET_SETTING {
                Value::from(v.is_some_and(|v| !v.is_empty()))
            } else {
                Value::from(v)
            },
        );
    }
    Ok(Value::Object(out))
}

/// One of `SETTINGS`, trimmed; an empty (or blank) Ele key means "unchanged".
fn settings_set(db: &Db, args: &Value) -> Result<Value> {
    let (k, v) = (arg(args, "key")?, arg(args, "value")?.trim());
    if !SETTINGS.contains(&k) {
        bail!("unknown setting: {k}");
    }
    if !(k == SECRET_SETTING && v.is_empty()) {
        db.settings_set(k, v)?;
    }
    Ok(Value::Null)
}

#[cfg(test)]
pub(crate) mod tests;
