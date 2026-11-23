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
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering::SeqCst};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use futures_util::future::BoxFuture;
use rand::seq::SliceRandom;
use rand::Rng;
use serde_json::json;

use super::errors::{error_code, MAILBOX_GONE};
use super::mail::{
    link_pick, wait_for_new_mail, EleClient, MailClient, OutlookClient, OutlookUrls, POLL_INTERVAL,
};
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
/// Never read back by our pages; masked in every stored text.
pub const SECRET_SETTING: &str = "ele_api_key";
/// An Ele mailbox while the Ele address or key is not set.
pub const ELE_UNSET: &str = "Ele API unavailable: ele_api_url / ele_api_key not set";

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
    /// None when the Ele address or key is not set (an Outlook run).
    pub ele: Option<EleClient>,
    pub outlook: OutlookClient,
    pub browsers: B,
    pub urls: Urls,
    pub tm: Timings,
    /// Failure screenshots go here as `<account_id>.png`.
    pub screens: PathBuf,
}

impl<B> Ctx<B> {
    /// The inbox reader for this mailbox.
    pub fn mail_for(&self, mb: &Mailbox) -> Result<MailClient<'_>> {
        if mb.provider == "outlook" {
            return Ok(MailClient::Outlook(&self.outlook));
        }
        self.ele
            .as_ref()
            .map(MailClient::Ele)
            .ok_or_else(|| anyhow!(ELE_UNSET))
    }
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

/// The account's mailbox (Ele or Outlook) as the flow's `Inbox`. A link it
/// hands out is masked from then on.
struct MailInbox<'a> {
    client: MailClient<'a>,
    mailbox: Mailbox,
    scrub: &'a Scrub,
}

impl Inbox for MailInbox<'_> {
    fn reuse(&self) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(async move {
            self.client
                .reuse_mailbox(&self.mailbox)
                .await
                .map_err(|e| e.0)
        })
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
                self.client,
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
    scrub.add(mb.client_id.as_deref().unwrap_or(""));
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
    // A mailbox known dead fails before anything launches (D-M3-2). An
    // Outlook one is read once, strictly: a dead grant is gone, an error is
    // the mail service down; mail already there is never the verify mail.
    if mb.status == "gone" {
        bail!(MAILBOX_GONE);
    }
    let client = cx.mail_for(&mb)?;
    if let MailClient::Outlook(_) = client {
        client.check(&mb).await.map_err(|e| anyhow!(e.0))?;
        if db.mailbox_get(mb.id)?.is_some_and(|m| m.status == "gone") {
            bail!(MAILBOX_GONE);
        }
    }
    let opened = cx.browsers.open(db, br.id, proxy.as_ref()).await?;
    // Nothing below may skip the stop at the end: no `?` from here on.
    let (bid, mid) = (Some(br.id), Some(mb.id));
    let _ = db.event(bid, mid, "info", Some("browser"), "browser opened");

    let inbox = MailInbox {
        client,
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
    /// Its `jobs` row (0 until made).
    job: AtomicI64,
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
        self.job.store(0, SeqCst);
        self.stop.store(false, SeqCst);
        Ok(())
    }

    pub fn end(&self) {
        self.running.store(false, SeqCst);
    }

    /// The running job's id and requested count; None when no run is on.
    pub fn job(&self) -> Option<(i64, u32)> {
        let job = self.job.load(SeqCst);
        (self.running.load(SeqCst) && job > 0).then(|| (job, self.total.load(SeqCst)))
    }

    /// Read by the tests; our pages read `pages::run_state`.
    #[cfg(test)]
    pub fn json(&self) -> serde_json::Value {
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
/// Per account, in this order: its mailbox (`mailbox_type` "ele": a new Ele
/// inbox, its row saved inside; "outlook": the next free one, oldest first,
/// taken once at the start, so fewer free ones make fewer accounts), a proxy
/// and the browser row (an Outlook mailbox's earlier browser is reused), the
/// account row (running), then `run_account`.
pub async fn make_breeze<B: Browsers>(
    cx: &Ctx<B>,
    count: u32,
    proxy_mode: &str,
    mailbox_type: &str,
    run: &RunState,
) -> Vec<Outcome> {
    // `begin_job` made it already (the app's run); tests come without one.
    let job = match run.job.load(SeqCst) {
        0 => match cx.db.job_create(KEY, count.into(), proxy_mode) {
            Ok(j) => {
                run.job.store(j, SeqCst);
                j
            }
            Err(e) => return vec![failed(format!("{e:#}"))],
        },
        j => j,
    };
    let outlook = if mailbox_type == "outlook" {
        match cx.db.breeze_free_outlook(count.into()) {
            Ok(ids) => Some(ids),
            Err(e) => return vec![failed(format!("{e:#}"))],
        }
    } else {
        None
    };
    let n = outlook.as_ref().map_or(count as usize, Vec::len);
    let mut out = Vec::new();
    for i in 0..n {
        if run.stop.load(SeqCst) {
            break;
        }
        let o = new_account(cx, job, proxy_mode, outlook.as_ref().map(|ids| ids[i])).await;
        let no_proxy = o.error.as_deref() == Some(NO_PROXY);
        out.push(o);
        run.done.fetch_add(1, SeqCst);
        if no_proxy {
            break; // an empty pool: more accounts would only burn Ele mailboxes
        }
    }
    out
}

/// One account on `outlook` (a free mailbox's id), or on a new Ele inbox.
async fn new_account<B: Browsers>(
    cx: &Ctx<B>,
    job: i64,
    mode: &str,
    outlook: Option<i64>,
) -> Outcome {
    let db = &*cx.db;
    let scrub = Scrub::of(db);
    let mb_id = match (outlook, &cx.ele) {
        (Some(id), _) => id,
        (None, None) => return failed(ELE_UNSET.into()),
        (None, Some(ele)) => match ele.create_mailbox().await {
            Ok(m) => m.id,
            Err(e) => {
                let _ = db.event(None, None, "error", Some("mail"), &e.0);
                return failed(e.0);
            }
        },
    };
    let has_browser = match db.browser_of_mailbox(mb_id) {
        Ok(b) => b.is_some(),
        Err(e) => return failed(err_line(&e, &scrub)),
    };
    let picked = if has_browser {
        Ok(None)
    } else {
        proxy::pick_proxy(db, mode, "")
    };
    let rows = (|| -> Result<i64> {
        if !has_browser {
            let proxy_id = picked.as_ref().ok().and_then(|p| p.as_ref().map(|p| p.id));
            db.browser_create(mb_id, proxy_id, mode)?;
        }
        let id = db.account_queue(mb_id, KEY, "email", Some(job))?;
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

static OUTLOOK: std::sync::OnceLock<OutlookClient> = std::sync::OnceLock::new();

/// The pages' Outlook client on our store (one connection pool, so a Test
/// all reuses it).
pub fn outlook() -> Result<&'static OutlookClient> {
    if let Some(c) = OUTLOOK.get() {
        return Ok(c);
    }
    let c = OutlookClient::new(db()?, OutlookUrls::live())?;
    Ok(OUTLOOK.get_or_init(|| c))
}

/// Ends the app's run when the spawned task ends, a panic included.
struct EndRun;

impl Drop for EndRun {
    fn drop(&mut self) {
        RUN.end();
    }
}

/// Begin `run` and make its `jobs` row at once, so the page's first
/// `run_state` after Make already shows the job (and starts following it).
pub fn begin_job(db: &Db, run: &RunState, count: u32, proxy_mode: &str) -> Result<i64> {
    run.begin(count)?;
    match db.job_create(KEY, count.into(), proxy_mode) {
        Ok(j) => {
            run.job.store(j, SeqCst);
            Ok(j)
        }
        Err(e) => {
            run.end();
            Err(e)
        }
    }
}

/// Where failure screenshots go (`<account_id>.png`).
pub fn screens_dir() -> Result<PathBuf> {
    Ok(crate::store::local_root()?.join("creator").join("screens"))
}

/// The Ele client on the saved address and key.
pub fn ele_client(db: &Arc<Db>) -> Result<EleClient> {
    let setting = |k| db.settings_get(k).map(Option::unwrap_or_default);
    Ok(EleClient::new(
        &setting("ele_api_url")?,
        &setting(SECRET_SETTING)?,
        db.clone(),
    )?)
}

/// A Make run's checks: the count, the proxy mode, the route
/// (`email:ele` | `email:outlook`) and, on Outlook, enough free mailboxes.
/// Answers the mailbox type.
pub fn check_start(db: &Db, count: u32, proxy_mode: &str, route: &str) -> Result<&'static str> {
    if !(1..=20).contains(&count) {
        bail!("count must be 1-20");
    }
    if !["none", "rotate", "least"].contains(&proxy_mode) {
        bail!("unknown proxy mode: {proxy_mode}");
    }
    let kind = match route {
        "email:ele" => "ele",
        "email:outlook" => "outlook",
        _ => bail!("unknown route: {route}"),
    };
    if kind == "outlook" {
        let free = db.breeze_free_outlook_count()?;
        if i64::from(count) > free {
            bail!("Only {free} free emails on this route");
        }
    }
    Ok(kind)
}

/// Start a Make run in the background (`breeze_run`).
pub fn start(count: u32, proxy_mode: String, route: &str) -> Result<()> {
    let db = db()?;
    let kind = check_start(&db, count, &proxy_mode, route)?;
    let ele = match kind {
        "ele" => Some(ele_client(&db)?),
        _ => None,
    };
    let outlook = OutlookClient::new(db.clone(), OutlookUrls::live())?;
    let screens = screens_dir()?;
    let cx = Ctx {
        db,
        ele,
        outlook,
        browsers: Shard,
        urls: Urls::live(),
        tm: Timings::default(),
        screens,
    };
    begin_job(&cx.db, &RUN, count, &proxy_mode)?;
    tokio::spawn(async move {
        let _end = EndRun;
        make_breeze(&cx, count, &proxy_mode, kind, &RUN).await;
    });
    Ok(())
}

#[cfg(test)]
mod live;
#[cfg(test)]
pub(crate) mod tests;
