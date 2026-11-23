//! The mail clients: Ele temp mail (Python `providers/ele.py` `EleClient`)
//! and Outlook through Microsoft Graph (`providers/outlook.py`
//! `OutlookClient`), the mail waits of `providers/outlook.py` (`link_pick`,
//! `wait_for_new_mail`), the code reader (`ele.extract_otp`) and the
//! HTML-to-text of mail bodies (`_html_text`). Mailboxes (their tokens) and
//! received mail are kept in our store.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

use rand::seq::SliceRandom;
use regex::Regex;
use serde_json::Value;

use super::store::{parse_iso, Db, MailMessage, Mailbox, NewMail};

/// A mail API is unavailable (Ele or Outlook). Messages never hold URLs (an
/// Ele URL carries the mailbox token), tokens, the API key or a body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EleError(pub String);

impl fmt::Display for EleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for EleError {}

fn unavailable(what: impl fmt::Display) -> EleError {
    EleError(format!("Ele API unavailable: {what}"))
}

/// A store failure, without its text (it could hold a row's values).
fn db_err(_: anyhow::Error) -> EleError {
    unavailable("local store error")
}

/// What failed in a request, by kind only (Python `type(e).__name__`):
/// reqwest's own text names the URL.
fn class(e: &reqwest::Error) -> &'static str {
    if e.is_timeout() {
        "Timeout"
    } else if e.is_connect() {
        "ConnectError"
    } else if e.is_decode() || e.is_body() {
        "ReadError"
    } else {
        "RequestError"
    }
}

/// Seconds without a good inbox answer before a poll fails clearly.
pub const UNAVAILABLE_AFTER: Duration = Duration::from_secs(30);

const ELE_INBOX: &str = "Ele API unavailable: inbox ";
const OUTLOOK_INBOX: &str = "Outlook unavailable: inbox ";

/// A client's blip grace and reuse cutoffs (Python `_last_ok`, `_cutoff`).
pub struct Grace {
    /// Mailbox id -> last good inbox read.
    last_ok: Mutex<HashMap<i64, Instant>>,
    /// Reused mailbox id -> mail received at or before this (the provider's
    /// clock, micros) is from a past run.
    cutoff: Mutex<HashMap<i64, i64>>,
    /// Python's class constant; a test shortens it.
    pub unavailable_after: Duration,
}

impl Default for Grace {
    fn default() -> Self {
        Grace {
            last_ok: Mutex::default(),
            cutoff: Mutex::default(),
            unavailable_after: UNAVAILABLE_AFTER,
        }
    }
}

impl Grace {
    fn ok_now(&self, mailbox_id: i64) {
        self.last_ok
            .lock()
            .unwrap()
            .insert(mailbox_id, Instant::now());
    }

    /// A failed read: a blip (no mail) within the grace, an error after it;
    /// `strict` (a reuse, a check) has no grace.
    fn failed(
        &self,
        mailbox_id: i64,
        prefix: &str,
        reason: &str,
        strict: bool,
    ) -> Result<Vec<MailMessage>, EleError> {
        let last = *self
            .last_ok
            .lock()
            .unwrap()
            .entry(mailbox_id)
            .or_insert_with(Instant::now);
        if strict || last.elapsed() >= self.unavailable_after {
            return Err(EleError(format!("{prefix}{reason}")));
        }
        Ok(Vec::new())
    }

    fn cutoff_of(&self, mailbox_id: i64) -> Option<i64> {
        self.cutoff.lock().unwrap().get(&mailbox_id).copied()
    }

    fn set_cutoff(&self, mailbox_id: i64, at: Option<i64>) {
        let mut c = self.cutoff.lock().unwrap();
        match at {
            Some(t) => c.insert(mailbox_id, t),
            None => c.remove(&mailbox_id),
        };
    }
}

/// After a strict poll on a reused mailbox: every mail without a code taken
/// is "stale" (an old code or link is never used), and the newest dated mail
/// seen now is the cutoff.
fn mark_stale_and_cutoff(db: &Db, grace: &Grace, mailbox_id: i64) -> Result<(), EleError> {
    let msgs = db.mail_list(mailbox_id).map_err(db_err)?;
    for m in msgs.iter().filter(|m| m.otp_extracted.is_none()) {
        db.mail_set_otp(m.id, "stale").map_err(db_err)?;
    }
    let newest = msgs
        .iter()
        .filter_map(|m| received(m.received_at.as_deref()))
        .max();
    grace.set_cutoff(mailbox_id, newest);
    Ok(())
}

pub struct EleClient {
    http: reqwest::Client,
    base: String,
    key: String,
    db: Arc<Db>,
    pub grace: Grace,
}

impl EleClient {
    pub fn new(base_url: &str, api_key: &str, db: Arc<Db>) -> Result<EleClient, EleError> {
        if base_url.is_empty() || api_key.is_empty() {
            return Err(unavailable("ele_api_url / ele_api_key not set"));
        }
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(8))
            .build()
            .map_err(|_| unavailable("client setup failed"))?;
        Ok(EleClient {
            http,
            base: format!("{}/", base_url.trim_end_matches('/')),
            key: api_key.to_string(),
            db,
            grace: Grace::default(),
        })
    }

    fn req(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}{path}", self.base))
            .header("X-API-Key", &self.key)
            .header("accept", "application/json")
    }

    /// Active domains, blocked ones left out.
    pub async fn active_domains(&self) -> Result<Vec<String>, EleError> {
        let r = self
            .req(reqwest::Method::GET, "mail-domains?active_only=true")
            .send()
            .await
            .map_err(|e| unavailable(format!("domains {}", class(&e))))?;
        if r.status() != 200 {
            return Err(unavailable(format!(
                "domains fetch failed (HTTP {})",
                r.status().as_u16()
            )));
        }
        let body: Value = r
            .json()
            .await
            .map_err(|e| unavailable(format!("domains {}", class(&e))))?;
        let blocked = self.db.blocked_domains().map_err(db_err)?;
        let items = body["domains"].as_array().cloned().unwrap_or_default();
        Ok(items
            .iter()
            .filter(|d| truthy(&d["is_active"]))
            .filter_map(|d| d["domain"].as_str().filter(|s| !s.is_empty()))
            .filter(|d| !blocked.contains(&d.to_lowercase()))
            .map(str::to_string)
            .collect())
    }

    /// Number of usable domains.
    pub async fn check(&self) -> Result<usize, EleError> {
        Ok(self.active_domains().await?.len())
    }

    /// A new inbox on a random usable domain; its row (with the token) is
    /// saved before anything else.
    pub async fn create_mailbox(&self) -> Result<Mailbox, EleError> {
        let domains = self.active_domains().await?;
        let domain = domains
            .choose(&mut rand::thread_rng())
            .ok_or_else(|| unavailable("no usable (active, unblocked) domains"))?;
        let r = self
            .req(reqwest::Method::POST, "inbox")
            .json(&serde_json::json!({ "domain": domain }))
            .send()
            .await
            .map_err(|e| unavailable(format!("inbox create {}", class(&e))))?;
        if !matches!(r.status().as_u16(), 200 | 201) {
            return Err(unavailable(format!(
                "inbox create failed (HTTP {})",
                r.status().as_u16()
            )));
        }
        let data: Value = r
            .json()
            .await
            .map_err(|e| unavailable(format!("inbox create {}", class(&e))))?;
        let (Some(email), Some(token)) = (data.get("email"), data.get("token")) else {
            return Err(unavailable("inbox create KeyError"));
        };
        let (email, token) = (email.as_str().unwrap_or(""), token.as_str().unwrap_or(""));
        if email.is_empty() || token.is_empty() {
            return Err(unavailable("inbox create returned no email/token"));
        }
        let mb = self
            .db
            .mailbox_create("ele", email, domain, token)
            .map_err(db_err)?;
        self.grace.ok_now(mb.id);
        Ok(mb)
    }

    /// Fetch the inbox, store mail not seen before, return the NEW rows.
    pub async fn poll_inbox(&self, mb: &Mailbox) -> Result<Vec<MailMessage>, EleError> {
        self.poll(mb, false).await
    }

    async fn poll(&self, mb: &Mailbox, strict: bool) -> Result<Vec<MailMessage>, EleError> {
        let failed = |reason: &str| self.grace.failed(mb.id, ELE_INBOX, reason, strict);
        let r = match self
            .req(reqwest::Method::GET, &format!("inbox/{}/emails", mb.token))
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return failed(class(&e)),
        };
        let status = r.status().as_u16();
        if status == 404 {
            // "Inbox not found": this address can never get mail again.
            self.db.mailbox_set_status(mb.id, "gone").map_err(db_err)?;
        }
        if status != 200 {
            return failed(&format!("HTTP {status}"));
        }
        self.db
            .mailbox_set_status(mb.id, "active")
            .map_err(db_err)?;
        let items = match r.json::<Value>().await {
            Ok(v) if v.is_object() => v["emails"].as_array().cloned().unwrap_or_default(),
            _ => return failed("unreadable response"),
        };
        self.grace.ok_now(mb.id);

        let existing = self.db.mail_list(mb.id).map_err(db_err)?;
        let by_id: HashMap<String, MailMessage> = existing
            .iter()
            .filter_map(|m| Some((m.ele_message_id.clone()?, m.clone())))
            .collect();
        let mut seen_ids: HashSet<String> = by_id.keys().cloned().collect();
        let mut seen_content: HashSet<String> = existing
            .iter()
            .filter(|m| m.ele_message_id.is_none())
            .map(|m| content_key(m.subject.as_deref(), m.body.as_deref()))
            .collect();
        let mut new = Vec::new();
        for it in items.iter().filter(|it| it.is_object()) {
            let mid = match &it["id"] {
                Value::Null => None,
                Value::String(s) if s.is_empty() => None,
                Value::String(s) => Some(s.clone()),
                v => Some(v.to_string()),
            };
            let subject = text(it, &["subject"]).unwrap_or_default();
            let html = it["html"].as_str().map(str::to_string);
            let body = text(it, &["body", "text", "body_text"])
                .unwrap_or_else(|| html_text(html.as_deref().unwrap_or("")));
            if let Some(mid) = &mid {
                if seen_ids.contains(mid) {
                    // Backfill rows saved before html was read.
                    if let Some(old) = by_id.get(mid) {
                        if old.body.as_deref().unwrap_or("").is_empty() && !body.is_empty() {
                            self.db.mail_update_body(old.id, &body).map_err(db_err)?;
                        }
                    }
                    continue;
                }
                seen_ids.insert(mid.clone());
            } else if !seen_content.insert(content_key(Some(&subject), Some(&body))) {
                continue;
            }
            let row = NewMail {
                ele_message_id: mid,
                sender: text(it, &["from", "sender"]).unwrap_or_default(),
                subject,
                body,
                html,
                received_at: text(it, &["received_at", "created_at", "date"]),
            };
            new.extend(self.db.mail_add(mb.id, &row).map_err(db_err)?);
        }
        Ok(new)
    }

    /// Before a retry on the same address: store what the inbox holds now,
    /// then mark every mail without a code taken as "stale" so an old code or
    /// link is never used. Any inbox failure is an error at once.
    pub async fn reuse_mailbox(&self, mb: &Mailbox) -> Result<(), EleError> {
        self.poll(mb, true).await?;
        mark_stale_and_cutoff(&self.db, &self.grace, mb.id)
    }
}

/// Microsoft's endpoints (Python `TOKEN_URL`, `GRAPH_URL`); a test points
/// them at a fake.
#[derive(Clone, Debug)]
pub struct OutlookUrls {
    pub token: String,
    pub graph: String,
}

const GRAPH_QUERY: &str = "$top=20&$select=id,subject,from,body,bodyPreview,receivedDateTime&$orderby=receivedDateTime desc";

impl OutlookUrls {
    pub fn live() -> OutlookUrls {
        OutlookUrls {
            token: "https://login.live.com/oauth20_token.srf".into(),
            graph: format!("https://graph.microsoft.com/v1.0/me/messages?{GRAPH_QUERY}"),
        }
    }

    /// The same paths under `/ms/` of a test server.
    #[cfg(test)]
    pub fn on(site: &str) -> OutlookUrls {
        OutlookUrls {
            token: format!("{site}/ms/oauth20_token.srf"),
            graph: format!("{site}/ms/v1.0/me/messages?{GRAPH_QUERY}"),
        }
    }
}

const SCOPE: &str = "https://graph.microsoft.com/Mail.Read offline_access";

fn outlook_store_err(_: anyhow::Error) -> EleError {
    EleError(format!("{OUTLOOK_INBOX}local store error"))
}

/// An Outlook mailbox read through Microsoft Graph with its refresh token,
/// which Microsoft rotates: the new one is saved before anything else, and
/// every poll reads the token from the row (never from the `Mailbox` given).
pub struct OutlookClient {
    http: reqwest::Client,
    db: Arc<Db>,
    urls: OutlookUrls,
    pub grace: Grace,
}

impl OutlookClient {
    pub fn new(db: Arc<Db>, urls: OutlookUrls) -> Result<OutlookClient, EleError> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|_| EleError(format!("{OUTLOOK_INBOX}client setup failed")))?;
        Ok(OutlookClient {
            http,
            db,
            urls,
            grace: Grace::default(),
        })
    }

    /// Refresh the token, read the inbox via Graph, store unseen mail, return
    /// the NEW rows. Only a dead grant (`invalid_grant`) marks it gone.
    pub async fn poll_inbox(&self, mb: &Mailbox) -> Result<Vec<MailMessage>, EleError> {
        self.poll(mb, false).await
    }

    async fn poll(&self, mb: &Mailbox, strict: bool) -> Result<Vec<MailMessage>, EleError> {
        let failed = |reason: &str| self.grace.failed(mb.id, OUTLOOK_INBOX, reason, strict);
        let row = self
            .db
            .mailbox_get(mb.id)
            .map_err(outlook_store_err)?
            .ok_or_else(|| EleError(format!("{OUTLOOK_INBOX}not found")))?;
        let form = [
            ("client_id", row.client_id.as_deref().unwrap_or("")),
            ("grant_type", "refresh_token"),
            ("refresh_token", row.token.as_str()),
            ("scope", SCOPE),
        ];
        let t = match self.http.post(&self.urls.token).form(&form).send().await {
            Ok(t) => t,
            Err(e) => return failed(class(&e)),
        };
        let status = t.status().as_u16();
        if status == 400 || status == 401 {
            let body: Value = t.json().await.unwrap_or(Value::Null);
            // Throttling and other AAD errors are temporary: only a dead grant is gone.
            if body.get("error").and_then(Value::as_str) != Some("invalid_grant") {
                return failed(&format!("token HTTP {status}"));
            }
            self.db
                .mailbox_set_status(mb.id, "gone")
                .map_err(outlook_store_err)?;
            return Ok(Vec::new());
        }
        if status != 200 {
            return failed(&format!("token HTTP {status}"));
        }
        let tok: Value = t.json().await.unwrap_or(Value::Null);
        let Some(access) = tok.get("access_token").and_then(Value::as_str) else {
            return failed("unreadable token response");
        };
        if let Some(new_rt) = tok.get("refresh_token").and_then(Value::as_str) {
            if !new_rt.is_empty() && new_rt != row.token {
                // Losing it kills the mailbox: saved before anything else.
                self.db
                    .mailbox_set_token(mb.id, new_rt)
                    .map_err(outlook_store_err)?;
            }
        }
        let r = match self
            .http
            .get(&self.urls.graph)
            .bearer_auth(access)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => return failed(class(&e)),
        };
        if r.status() != 200 {
            return failed(&format!("HTTP {}", r.status().as_u16()));
        }
        let items = match r.json::<Value>().await {
            Ok(Value::Object(o)) => o
                .get("value")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            _ => return failed("unreadable response"),
        };
        self.db
            .mailbox_set_status(mb.id, "active")
            .map_err(outlook_store_err)?;
        self.grace.ok_now(mb.id);

        let mut seen: HashSet<String> = self
            .db
            .mail_list(mb.id)
            .map_err(outlook_store_err)?
            .into_iter()
            .filter_map(|m| m.ele_message_id)
            .collect();
        let mut new = Vec::new();
        for it in items.iter().filter(|it| it.is_object()) {
            let mid = match &it["id"] {
                Value::Null => continue,
                Value::String(s) if s.is_empty() => continue,
                Value::String(s) => s.clone(),
                v => v.to_string(),
            };
            if !seen.insert(mid.clone()) {
                continue;
            }
            let content = it["body"]["content"].as_str().unwrap_or("");
            let row = NewMail {
                ele_message_id: Some(mid),
                sender: it["from"]["emailAddress"]["address"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
                subject: it["subject"].as_str().unwrap_or("").to_string(),
                body: html_text(content),
                html: (it["body"]["contentType"].as_str() == Some("html"))
                    .then(|| content.to_string()),
                received_at: it["receivedDateTime"].as_str().map(str::to_string),
            };
            // Another poll of this mailbox may have stored it a moment ago.
            new.extend(self.db.mail_add(mb.id, &row).map_err(outlook_store_err)?);
        }
        Ok(new)
    }

    /// Ele's `reuse_mailbox`, on Outlook (Python shares the body).
    pub async fn reuse_mailbox(&self, mb: &Mailbox) -> Result<(), EleError> {
        self.poll(mb, true).await?;
        mark_stale_and_cutoff(&self.db, &self.grace, mb.id)
    }
}

/// The inbox reader of one mailbox (Python `mail_client`).
#[derive(Clone, Copy)]
pub enum MailClient<'a> {
    Ele(&'a EleClient),
    Outlook(&'a OutlookClient),
}

impl MailClient<'_> {
    pub async fn poll_inbox(&self, mb: &Mailbox) -> Result<Vec<MailMessage>, EleError> {
        match self {
            MailClient::Ele(c) => c.poll_inbox(mb).await,
            MailClient::Outlook(c) => c.poll_inbox(mb).await,
        }
    }

    pub async fn reuse_mailbox(&self, mb: &Mailbox) -> Result<(), EleError> {
        match self {
            MailClient::Ele(c) => c.reuse_mailbox(mb).await,
            MailClient::Outlook(c) => c.reuse_mailbox(mb).await,
        }
    }

    /// One poll with no blip grace (Python's `_last_ok = -inf`): a failure is
    /// an error at once. A dead Outlook grant is not an error: the row is gone.
    pub async fn check(&self, mb: &Mailbox) -> Result<(), EleError> {
        match self {
            MailClient::Ele(c) => c.poll(mb, true).await.map(drop),
            MailClient::Outlook(c) => c.poll(mb, true).await.map(drop),
        }
    }

    fn cutoff_of(&self, mailbox_id: i64) -> Option<i64> {
        match self {
            MailClient::Ele(c) => c.grace.cutoff_of(mailbox_id),
            MailClient::Outlook(c) => c.grace.cutoff_of(mailbox_id),
        }
    }
}

/// Codes that read as words, never a code (Python `IGNORE_WORDS`).
const IGNORE_WORDS: [&str; 7] = [
    "SIGNIN", "PLEASE", "GOOGLE", "ACCESS", "SIGNUP", "VERIFY", "CLOUDS",
];

fn re(p: &str) -> Regex {
    Regex::new(p).unwrap()
}

/// Python's `(?=[A-Za-z]*\d)[A-Za-z0-9]{6}`: group 1 holding a digit. A
/// candidate without one is skipped and the search goes on from its next
/// character, as Python's search would.
fn with_digit(r: &Regex, t: &str) -> Option<String> {
    let mut at = 0;
    while let Some(c) = r.captures_at(t, at) {
        let g = c.get(1).unwrap().as_str();
        if g.bytes().any(|b| b.is_ascii_digit()) {
            return Some(g.to_string());
        }
        at = c.get(0).unwrap().start() + 1;
        while !t.is_char_boundary(at) {
            at += 1;
        }
    }
    None
}

static OTP: LazyLock<[Regex; 10]> = LazyLock::new(|| {
    [
        re(r"(?i)(?:one-time\s*)?verification\s*code\s*is:?\s*([A-Za-z0-9]{6})\b"),
        re(
            r"(?i)(?:verification\s*code|verify|your\s*code|enter\s*code|security\s*code)[^\w]{1,40}([A-Za-z0-9]{6})\b",
        ),
        re(r"(?i)(?:confirmation|verification|security)\s*code\s*is:?\s*(\d{5,8})\b"),
        re(r"(?i)G-(\d{6})\b"),
        re(r"(?i)(?:code|verify|verification)[^\d]{1,25}(\d{3})\s+(\d{3})\b"),
        re(r"[a-zA-Z0-9_.+-]+@[a-zA-Z0-9-]+\.[a-zA-Z0-9.\-]+"),
        re(r"(?i)[a-f0-9]{8}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{4}-[a-f0-9]{12}"),
        re(r"\b([A-Z0-9]{6})\b"),
        re(r"<[^>]+>"),
        re(r"\s+"),
    ]
});

/// A sign-in or verify code in a mail's text (Python `ele.extract_otp`,
/// every pattern in its order, then the fallback). Letter codes are
/// upper-cased.
pub fn extract_otp(text: &str) -> Option<String> {
    let [p1, p2, p3, p4, p5, mail, uuid, word, tag, ws] = &*OTP;
    if text.is_empty() {
        return None;
    }
    let t = tag.replace_all(text, " ").replace("&nbsp;", " ");
    let t = ws.replace_all(&t, " ");
    if let Some(c) = with_digit(p1, &t).or_else(|| with_digit(p2, &t)) {
        return Some(c.to_uppercase());
    }
    if let Some(c) = p3.captures(&t).or_else(|| p4.captures(&t)) {
        return Some(c[1].to_string());
    }
    if let Some(c) = p5.captures(&t) {
        return Some(format!("{}{}", &c[1], &c[2]));
    }
    let f = mail.replace_all(&t, " ");
    let f = uuid.replace_all(&f, " ");
    word.captures_iter(&f)
        .map(|c| c[1].to_string())
        .filter(|c| c.bytes().any(|b| b.is_ascii_digit()))
        .find(|c| {
            !IGNORE_WORDS.contains(&c.as_str())
                && !["201", "202", "203"].iter().any(|y| c.starts_with(y))
        })
}

fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// The first non-empty string of `keys` (Python's `a or b or c`).
fn text(it: &Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|k| it[*k].as_str().filter(|s| !s.is_empty()))
        .map(str::to_string)
}

fn content_key(subject: Option<&str>, body: Option<&str>) -> String {
    format!("{}\n{}", subject.unwrap_or(""), body.unwrap_or(""))
}

/// Ele's `received_at` in its own clock; None when missing, a bare date or
/// unreadable (too coarse to tell an old mail from a fresh one).
fn received(at: Option<&str>) -> Option<i64> {
    let s = at.unwrap_or("").trim();
    if s.len() <= 10 {
        return None;
    }
    parse_iso(s)
}

/// A `wait_for_new_mail` pick: the first https link (HTML, then the text
/// body) holding any of `accept` and none of `skip`, from a sender holding
/// `sender_has`. A link in the text body loses trailing `.`, `,`, `)`.
pub fn link_pick(
    sender_has: &str,
    accept: &[&str],
    skip: &[&str],
) -> impl Fn(&MailMessage) -> Option<String> + Send + Sync {
    let sender_has = sender_has.to_lowercase();
    let accept: Vec<String> = accept.iter().map(|s| s.to_string()).collect();
    let skip: Vec<String> = skip.iter().map(|s| s.to_string()).collect();
    let re = regex::Regex::new(r#"https://[^\s"'<>]+"#).unwrap();
    move |m: &MailMessage| {
        if !m
            .sender
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .contains(&sender_has)
        {
            return None;
        }
        for (src, tail) in [(&m.html, ""), (&m.body, ".,)")] {
            for u in re.find_iter(src.as_deref().unwrap_or("")) {
                let u = u.as_str().replace("&amp;", "&");
                let u = u.trim_end_matches(|c| tail.contains(c));
                if accept.iter().any(|a| u.contains(a.as_str()))
                    && !skip.iter().any(|s| u.contains(s.as_str()))
                {
                    return Some(u.to_string());
                }
            }
        }
        None
    }
}

/// How often `wait_for_new_mail` polls (Python's default).
pub const POLL_INTERVAL: Duration = Duration::from_secs(4);

/// Poll the inbox until `pick` returns a value. Only rows a poll returns as
/// NEW count, so mail stored earlier (by `reuse_mailbox`) is never used; mail
/// dated at or before the reuse cutoff is dropped. Newest first within a
/// poll; None at the deadline or once `stop()` is true (checked after every poll).
pub async fn wait_for_new_mail(
    client: MailClient<'_>,
    mb: &Mailbox,
    timeout: Duration,
    pick: impl Fn(&MailMessage) -> Option<String>,
    poll_interval: Duration,
    stop: Option<&(dyn Fn() -> bool + Send + Sync)>,
) -> Result<Option<String>, EleError> {
    let deadline = Instant::now() + timeout;
    loop {
        let mut new = client.poll_inbox(mb).await?;
        new.sort_by(|a, b| {
            let key = |m: &MailMessage| (m.received_at.clone().unwrap_or_default(), m.id);
            key(b).cmp(&key(a))
        });
        let cutoff = client.cutoff_of(mb.id);
        for m in &new {
            if let (Some(c), Some(d)) = (cutoff, received(m.received_at.as_deref())) {
                if d <= c {
                    continue;
                }
            }
            if let Some(got) = pick(m) {
                return Ok(Some(got));
            }
        }
        if Instant::now() >= deadline || stop.is_some_and(|s| s()) {
            return Ok(None);
        }
        tokio::time::sleep(poll_interval).await;
    }
}

const BLOCK: [&str; 12] = [
    "br", "p", "div", "tr", "li", "table", "h1", "h2", "h3", "h4", "h5", "h6",
];

/// HTML mail -> readable plain text (link text kept, URLs dropped): style,
/// script and head dropped, block tags become newlines (Python `_Text`).
pub fn html_text(html: &str) -> String {
    let mut out = String::new();
    let mut skip = 0usize;
    let lower = html.to_ascii_lowercase();
    let mut i = 0;
    while i < html.len() {
        let Some(lt) = html[i..].find('<').map(|n| i + n) else {
            if skip == 0 {
                out.push_str(&unescape(&html[i..]));
            }
            break;
        };
        if skip == 0 {
            out.push_str(&unescape(&html[i..lt]));
        }
        if lower[lt..].starts_with("<!--") {
            i = lower[lt..].find("-->").map_or(html.len(), |n| lt + n + 3);
            continue;
        }
        let Some(gt) = html[lt..].find('>').map(|n| lt + n) else {
            if skip == 0 {
                out.push_str(&unescape(&html[lt..]));
            }
            break;
        };
        let tag = &lower[lt + 1..gt];
        let (end, tag) = match tag.strip_prefix('/') {
            Some(t) => (true, t),
            None => (false, tag),
        };
        let name: String = tag
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric())
            .collect();
        i = gt + 1;
        if matches!(name.as_str(), "style" | "script" | "head") {
            if end {
                skip = skip.saturating_sub(1);
            } else {
                skip += 1;
                // Script and style bodies are raw text, not markup.
                if name != "head" {
                    let close = format!("</{name}");
                    if let Some(n) = lower[i..].find(&close) {
                        i += n;
                    }
                }
            }
        } else if BLOCK.contains(&name.as_str()) {
            out.push('\n');
        }
    }
    let t = regex::Regex::new("[ \t\u{a0}]+")
        .unwrap()
        .replace_all(&out, " ");
    let t = regex::Regex::new(" ?\n ?").unwrap().replace_all(&t, "\n");
    let t = regex::Regex::new("\n{3,}").unwrap().replace_all(&t, "\n\n");
    t.trim().to_string()
}

/// Character references.
// ponytail: the common named ones only; add the HTML5 table when a mail needs it.
fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let re = regex::Regex::new(r"&(#[0-9]+|#[xX][0-9a-fA-F]+|[a-zA-Z]+);").unwrap();
    re.replace_all(s, |c: &regex::Captures| {
        let r = &c[1];
        let ch = if let Some(h) = r.strip_prefix("#x").or_else(|| r.strip_prefix("#X")) {
            u32::from_str_radix(h, 16).ok().and_then(char::from_u32)
        } else if let Some(d) = r.strip_prefix('#') {
            d.parse().ok().and_then(char::from_u32)
        } else {
            match r {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some('\u{a0}'),
                _ => None,
            }
        };
        ch.map_or_else(|| c[0].to_string(), |c| c.to_string())
    })
    .into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creator::store::temp_db;
    use crate::creator::web::testkit::FixtureServer;
    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode};
    use axum::routing::{get, post};
    use axum::{Json, Router};

    #[derive(Default)]
    struct Fake {
        domains: Vec<Value>,
        emails: Vec<Value>,
        inbox_status: u16,
    }

    type Shared = Arc<Mutex<Fake>>;

    fn keyed(h: &HeaderMap) {
        assert_eq!(h["X-API-Key"], "KEY");
    }

    async fn server(fake: Shared) -> FixtureServer {
        let app = Router::new()
            .route(
                "/api/mail-domains",
                get(|State(f): State<Shared>, h: HeaderMap| async move {
                    keyed(&h);
                    Json(serde_json::json!({ "domains": f.lock().unwrap().domains }))
                }),
            )
            .route(
                "/api/inbox",
                post(|h: HeaderMap, Json(b): Json<Value>| async move {
                    keyed(&h);
                    let d = b["domain"].as_str().unwrap();
                    (
                        StatusCode::CREATED,
                        Json(serde_json::json!({ "email": format!("u@{d}"), "token": "tok123" })),
                    )
                }),
            )
            .route(
                "/api/inbox/:token/emails",
                get(|State(f): State<Shared>, h: HeaderMap| async move {
                    keyed(&h);
                    let f = f.lock().unwrap();
                    (
                        StatusCode::from_u16(f.inbox_status).unwrap(),
                        Json(serde_json::json!({ "emails": f.emails })),
                    )
                }),
            )
            .with_state(fake);
        FixtureServer::start_with(app).await
    }

    struct Env {
        db: Arc<Db>,
        fake: Shared,
        ele: EleClient,
        _srv: FixtureServer,
        _f: crate::creator::store::TempFile,
    }

    async fn env() -> Env {
        let (db, f) = temp_db();
        let db = Arc::new(db);
        let fake: Shared = Arc::new(Mutex::new(Fake {
            domains: vec![
                serde_json::json!({ "domain": "a.com", "is_active": true }),
                serde_json::json!({ "domain": "Bad.com", "is_active": true }),
            ],
            inbox_status: 200,
            ..Fake::default()
        }));
        let srv = server(fake.clone()).await;
        let ele = EleClient::new(&format!("{}/api/", srv.site), "KEY", db.clone()).unwrap();
        Env {
            db,
            fake,
            ele,
            _srv: srv,
            _f: f,
        }
    }

    fn set_emails(e: &Env, emails: Value) {
        e.fake.lock().unwrap().emails = emails.as_array().unwrap().clone();
    }

    const FAST: Duration = Duration::from_millis(10);

    #[tokio::test]
    async fn create_mailbox_skips_blocked_domains_and_saves_token_first() {
        let e = env().await;
        assert_eq!(e.ele.active_domains().await.unwrap(), ["a.com", "Bad.com"]);
        e.db.blocked_domain_add("bad.com", Some("x")).unwrap();
        assert_eq!(e.ele.active_domains().await.unwrap(), ["a.com"]);
        let mb = e.ele.create_mailbox().await.unwrap();
        let row = e.db.mailbox_get(mb.id).unwrap().unwrap();
        assert_eq!(
            (row.token.as_str(), row.domain.as_str(), row.email.as_str()),
            ("tok123", "a.com", "u@a.com")
        );
        assert_eq!(e.ele.check().await.unwrap(), 1);
        e.db.blocked_domain_add("a.com", Some("x")).unwrap();
        let err = e.ele.create_mailbox().await.unwrap_err();
        assert_eq!(
            err.0,
            "Ele API unavailable: no usable (active, unblocked) domains"
        );
    }

    #[tokio::test]
    async fn poll_stores_only_new_mail_by_id_then_by_content() {
        let e = env().await;
        let mb = e.ele.create_mailbox().await.unwrap();
        set_emails(
            &e,
            serde_json::json!([{ "id": 7, "from": "g@x", "subject": "s", "text": "hello", "date": "2026-01-01" }]),
        );
        assert_eq!(e.ele.poll_inbox(&mb).await.unwrap().len(), 1);
        assert!(e.ele.poll_inbox(&mb).await.unwrap().is_empty());
        let rows = e.db.mail_list(mb.id).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].ele_message_id.as_deref(), Some("7"));
        assert_eq!(rows[0].body.as_deref(), Some("hello"));
        assert_eq!(
            (rows[0].sender.as_deref(), rows[0].received_at.as_deref()),
            (Some("g@x"), Some("2026-01-01"))
        );

        // No id: the content identifies it.
        set_emails(&e, serde_json::json!([{ "subject": "s", "body": "one" }]));
        e.ele.poll_inbox(&mb).await.unwrap();
        e.ele.poll_inbox(&mb).await.unwrap();
        assert_eq!(e.db.mail_list(mb.id).unwrap().len(), 2);
        set_emails(
            &e,
            serde_json::json!([{ "subject": "s", "body": "one" }, { "subject": "s", "body": "two" }]),
        );
        assert_eq!(e.ele.poll_inbox(&mb).await.unwrap().len(), 1);
        assert_eq!(e.db.mail_list(mb.id).unwrap().len(), 3);
    }

    #[tokio::test]
    async fn poll_html_only_backfills_and_stores_html() {
        let e = env().await;
        let mb = e.ele.create_mailbox().await.unwrap();
        set_emails(
            &e,
            serde_json::json!([{ "id": 9, "sender": "m@x", "subject": "s", "body": "", "html": HTML }]),
        );
        assert_eq!(e.ele.poll_inbox(&mb).await.unwrap().len(), 1);
        let row = &e.db.mail_list(mb.id).unwrap()[0];
        assert!(row.body.as_deref().unwrap().contains("48219"));
        assert_eq!(row.html.as_deref(), Some(HTML));
        e.db.mail_update_body(row.id, "").unwrap();
        assert!(e.ele.poll_inbox(&mb).await.unwrap().is_empty());
        assert!(e.db.mail_list(mb.id).unwrap()[0]
            .body
            .as_deref()
            .unwrap()
            .contains("48219"));
    }

    #[tokio::test]
    async fn poll_404_marks_gone() {
        let e = env().await;
        let mb = e.ele.create_mailbox().await.unwrap();
        e.fake.lock().unwrap().inbox_status = 404;
        // Within the blip grace: no error, but a 404 is final for the address.
        assert!(e.ele.poll_inbox(&mb).await.unwrap().is_empty());
        assert_eq!(e.db.mailbox_get(mb.id).unwrap().unwrap().status, "gone");
        e.fake.lock().unwrap().inbox_status = 200;
        e.ele.poll_inbox(&mb).await.unwrap();
        assert_eq!(e.db.mailbox_get(mb.id).unwrap().unwrap().status, "active");
    }

    #[tokio::test]
    async fn blip_then_error_after_unavailable_after() {
        let mut e = env().await;
        let mb = e.ele.create_mailbox().await.unwrap();
        e.fake.lock().unwrap().inbox_status = 503;
        assert!(e.ele.poll_inbox(&mb).await.unwrap().is_empty());
        e.ele.grace.unavailable_after = Duration::ZERO;
        let err = e.ele.poll_inbox(&mb).await.unwrap_err();
        assert_eq!(err.0, "Ele API unavailable: inbox HTTP 503");
    }

    #[tokio::test]
    async fn reuse_marks_old_mail_stale_and_sets_cutoff() {
        let e = env().await;
        // A mailbox from an earlier run: no good read known.
        let mb =
            e.db.mailbox_create("ele", "u@a.com", "a.com", "tok123")
                .unwrap();
        let used =
            e.db.mail_create(
                mb.id,
                &NewMail {
                    ele_message_id: Some("1".into()),
                    body: "x".into(),
                    ..Default::default()
                },
            )
            .unwrap();
        e.db.mail_set_otp(used.id, "AAAAAA").unwrap();
        set_emails(
            &e,
            serde_json::json!([
                { "id": 1, "body": "x" },
                { "id": 2, "from": "breezeblue", "body": "https://x/verify?t=old", "received_at": "2026-10-25T10:00:00" },
            ]),
        );
        e.ele.reuse_mailbox(&mb).await.unwrap();
        let otp: HashMap<_, _> =
            e.db.mail_list(mb.id)
                .unwrap()
                .into_iter()
                .map(|m| (m.ele_message_id.unwrap(), m.otp_extracted))
                .collect();
        assert_eq!(
            otp,
            HashMap::from([
                ("1".to_string(), Some("AAAAAA".to_string())),
                ("2".to_string(), Some("stale".to_string()))
            ])
        );
        assert_eq!(
            e.ele.grace.cutoff_of(mb.id),
            parse_iso("2026-10-25T10:00:00")
        );

        // An inbox that cannot be read fails a reuse at once.
        e.fake.lock().unwrap().inbox_status = 404;
        let err = e.ele.reuse_mailbox(&mb).await.unwrap_err();
        assert!(!err.0.contains("tok123") && !err.0.contains("KEY"), "{err}");

        // A reuse of an empty inbox sets no cutoff.
        let mb2 =
            e.db.mailbox_create("ele", "v@a.com", "a.com", "tok123")
                .unwrap();
        e.fake.lock().unwrap().inbox_status = 200;
        set_emails(&e, serde_json::json!([]));
        e.ele.reuse_mailbox(&mb2).await.unwrap();
        assert_eq!(e.ele.grace.cutoff_of(mb2.id), None);
    }

    fn link(n: u32, at: Option<&str>) -> Value {
        let mut m = serde_json::json!({ "id": n, "from": "Breeze <hi@breezeblue.ai>", "body": format!("https://b.ai/verify?t={n}") });
        if let Some(at) = at {
            m["received_at"] = at.into();
        }
        m
    }

    #[tokio::test]
    async fn wait_for_new_mail_ignores_mail_before_cutoff() {
        let e = env().await;
        let mb =
            e.db.mailbox_create("ele", "u@a.com", "a.com", "tok123")
                .unwrap();
        let pick = link_pick("breezeblue", &["verif"], &[]);
        set_emails(
            &e,
            serde_json::json!([link(1, Some("2026-10-25T10:00:00"))]),
        );
        e.ele.reuse_mailbox(&mb).await.unwrap();
        // Stored by the reuse: not new, never used.
        assert_eq!(
            wait_for_new_mail(
                MailClient::Ele(&e.ele),
                &mb,
                Duration::ZERO,
                &pick,
                FAST,
                None
            )
            .await
            .unwrap(),
            None
        );
        // An old mail that came in late.
        set_emails(
            &e,
            serde_json::json!([
                link(1, Some("2026-10-25T10:00:00")),
                link(2, Some("2026-10-25T09:59:00"))
            ]),
        );
        assert_eq!(
            wait_for_new_mail(
                MailClient::Ele(&e.ele),
                &mb,
                Duration::ZERO,
                &pick,
                FAST,
                None
            )
            .await
            .unwrap(),
            None
        );
        // Fresh, whatever this machine's clock says; and an undated one is never dropped.
        set_emails(
            &e,
            serde_json::json!([link(3, Some("2026-10-25T10:01:00"))]),
        );
        assert_eq!(
            wait_for_new_mail(
                MailClient::Ele(&e.ele),
                &mb,
                Duration::ZERO,
                &pick,
                FAST,
                None
            )
            .await
            .unwrap()
            .as_deref(),
            Some("https://b.ai/verify?t=3")
        );
        set_emails(
            &e,
            serde_json::json!([link(4, None), link(5, Some("2026-10-25"))]),
        );
        let got = wait_for_new_mail(
            MailClient::Ele(&e.ele),
            &mb,
            Duration::ZERO,
            &pick,
            FAST,
            None,
        )
        .await
        .unwrap();
        assert!(
            matches!(
                got.as_deref(),
                Some("https://b.ai/verify?t=4" | "https://b.ai/verify?t=5")
            ),
            "{got:?}"
        );
    }

    #[tokio::test]
    async fn wait_for_new_mail_polls_until_the_deadline_or_stop() {
        let e = env().await;
        let mb = e.ele.create_mailbox().await.unwrap();
        let pick = link_pick("breezeblue", &["verif"], &[]);
        let t = Instant::now();
        let stop = || true;
        assert_eq!(
            wait_for_new_mail(
                MailClient::Ele(&e.ele),
                &mb,
                Duration::from_secs(5),
                &pick,
                FAST,
                Some(&stop)
            )
            .await
            .unwrap(),
            None
        );
        assert!(t.elapsed() < Duration::from_secs(1));
        let fake = e.fake.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            fake.lock().unwrap().emails = vec![link(1, None)];
        });
        let got = wait_for_new_mail(
            MailClient::Ele(&e.ele),
            &mb,
            Duration::from_secs(5),
            &pick,
            FAST,
            None,
        )
        .await
        .unwrap();
        assert_eq!(got.as_deref(), Some("https://b.ai/verify?t=1"));
    }

    fn msg(sender: &str, html: Option<&str>, body: &str) -> MailMessage {
        MailMessage {
            id: 1,
            mailbox_id: 1,
            ele_message_id: None,
            sender: Some(sender.into()),
            subject: None,
            body: Some(body.into()),
            otp_extracted: None,
            received_at: None,
            html: html.map(Into::into),
        }
    }

    #[test]
    fn link_pick_html_first_and_trims_text_punctuation() {
        let pick = link_pick("BreezeBlue", &["verif"], &["unsubscribe"]);
        let html = r#"<a href="https://b.ai/unsubscribe?verif=1">x</a> <a href="https://b.ai/verify?a=1&amp;t=2">go</a>"#;
        assert_eq!(
            pick(&msg(
                "hi@breezeblue.ai",
                Some(html),
                "https://b.ai/verify?t=text"
            ))
            .as_deref(),
            Some("https://b.ai/verify?a=1&t=2")
        );
        assert_eq!(
            pick(&msg(
                "hi@breezeblue.ai",
                None,
                "Open https://b.ai/verify?t=3)."
            ))
            .as_deref(),
            Some("https://b.ai/verify?t=3")
        );
        assert_eq!(
            pick(&msg("hi@breezeblue.ai", None, "http://b.ai/verify?t=3")),
            None
        );
        assert_eq!(
            pick(&msg("someone@else", None, "https://b.ai/verify?t=3")),
            None
        );
    }

    #[tokio::test]
    async fn errors_never_hold_token_or_key() {
        let mut e = env().await;
        let mb = e.ele.create_mailbox().await.unwrap();
        e.ele.grace.unavailable_after = Duration::ZERO;
        let mut errs = Vec::new();
        for status in [404, 500] {
            e.fake.lock().unwrap().inbox_status = status;
            errs.push(e.ele.poll_inbox(&mb).await.unwrap_err());
        }
        // Nobody listening: a network error.
        let (db2, _f2) = temp_db();
        let dead = EleClient::new("http://127.0.0.1:9/api", "KEY", Arc::new(db2)).unwrap();
        errs.push(dead.active_domains().await.unwrap_err());
        errs.push(dead.reuse_mailbox(&mb).await.unwrap_err());
        for err in &errs {
            assert!(err.0.starts_with("Ele API unavailable: "), "{err}");
            for secret in ["tok123", "KEY", "127.0.0.1", "http"] {
                assert!(!err.0.contains(secret), "{err}");
            }
        }
        assert_eq!(
            EleClient::new("", "KEY", e.db.clone()).err().unwrap().0,
            "Ele API unavailable: ele_api_url / ele_api_key not set"
        );
    }

    const HTML: &str =
        "<html><head><title>T</title><style>p{color:red}</style></head><body><script>x=1</script>\
        <p>Confirm your <a href='http://t.co/x'>email</a></p>Your confirmation code is 48219<br>\
        <table><tr><td>Tom &amp; Jerry</td></tr></table></body></html>";

    #[test]
    fn html_text_drops_head_and_script_and_breaks_blocks() {
        assert_eq!(
            html_text(HTML),
            "Confirm your email\nYour confirmation code is 48219\n\nTom & Jerry"
        );
        assert_eq!(html_text("a&nbsp; b &#33;&#x41;<!-- <p> -->c"), "a b !Ac");
    }

    // ---- Outlook (Python tests/test_outlook.py)

    struct OFake {
        token: (u16, Value),
        graph: (u16, Value),
        /// `refresh_token` of each token call, in order.
        refresh_seen: Vec<String>,
    }

    type OShared = Arc<Mutex<OFake>>;

    fn msg_json() -> Value {
        serde_json::json!({
            "id": "AAA1", "subject": "Your code is: ABC123 verification code",
            "receivedDateTime": "2026-11-17T10:00:00Z",
            "from": { "emailAddress": { "address": "no-reply@sender.example" } },
            "body": { "content": "<p>Hi <b>there</b></p>" },
        })
    }

    async fn outlook_server(fake: OShared) -> FixtureServer {
        use axum::extract::RawQuery;
        use axum::Form;
        let app = Router::new()
            .route(
                "/ms/oauth20_token.srf",
                post(
                    |State(f): State<OShared>, Form(form): Form<HashMap<String, String>>| async move {
                        assert_eq!(form["client_id"], "CID");
                        assert_eq!(form["grant_type"], "refresh_token");
                        assert_eq!(form["scope"], SCOPE);
                        let mut f = f.lock().unwrap();
                        f.refresh_seen.push(form["refresh_token"].clone());
                        let (code, body) = f.token.clone();
                        (StatusCode::from_u16(code).unwrap(), Json(body))
                    },
                ),
            )
            .route(
                "/ms/v1.0/me/messages",
                get(
                    |State(f): State<OShared>, h: HeaderMap, RawQuery(q): RawQuery| async move {
                        assert_eq!(h["authorization"], "Bearer AT");
                        assert_eq!(q.as_deref(), Some(GRAPH_QUERY.replace(' ', "%20").as_str()));
                        let (code, body) = f.lock().unwrap().graph.clone();
                        (StatusCode::from_u16(code).unwrap(), Json(body))
                    },
                ),
            )
            .with_state(fake);
        FixtureServer::start_with(app).await
    }

    struct OEnv {
        db: Arc<Db>,
        fake: OShared,
        cl: OutlookClient,
        mb: Mailbox,
        _srv: FixtureServer,
        _f: crate::creator::store::TempFile,
    }

    async fn oenv() -> OEnv {
        let (db, f) = temp_db();
        let db = Arc::new(db);
        let fake: OShared = Arc::new(Mutex::new(OFake {
            token: (
                200,
                serde_json::json!({ "access_token": "AT", "refresh_token": "NEW" }),
            ),
            graph: (200, serde_json::json!({ "value": [msg_json()] })),
            refresh_seen: Vec::new(),
        }));
        let srv = outlook_server(fake.clone()).await;
        let cl = OutlookClient::new(db.clone(), OutlookUrls::on(&srv.site)).unwrap();
        let mb = db
            .mailbox_create("outlook", "a@outlook.example", "outlook.example", "OLD")
            .unwrap();
        db.exec_for_test(&format!(
            "UPDATE mailboxes SET client_id='CID' WHERE id={}",
            mb.id
        ));
        let mb = db.mailbox_get(mb.id).unwrap().unwrap();
        OEnv {
            db,
            fake,
            cl,
            mb,
            _srv: srv,
            _f: f,
        }
    }

    impl OEnv {
        fn token(&self, code: u16, body: Value) {
            self.fake.lock().unwrap().token = (code, body);
        }
        fn graph(&self, code: u16, body: Value) {
            self.fake.lock().unwrap().graph = (code, body);
        }
        fn row(&self) -> Mailbox {
            self.db.mailbox_get(self.mb.id).unwrap().unwrap()
        }
    }

    #[tokio::test]
    async fn poll_maps_mail() {
        let e = oenv().await;
        assert_eq!(e.cl.poll_inbox(&e.mb).await.unwrap().len(), 1);
        let m = &e.db.mail_list(e.mb.id).unwrap()[0];
        assert_eq!(
            (
                m.ele_message_id.as_deref(),
                m.sender.as_deref(),
                m.subject.as_deref(),
                m.body.as_deref(),
                m.received_at.as_deref()
            ),
            (
                Some("AAA1"),
                Some("no-reply@sender.example"),
                Some("Your code is: ABC123 verification code"),
                Some("Hi there"),
                Some("2026-11-17T10:00:00Z")
            )
        );
        assert_eq!(
            extract_otp(m.subject.as_deref().unwrap()).as_deref(),
            Some("ABC123")
        );
    }

    #[tokio::test]
    async fn poll_persists_rotated_token() {
        let e = oenv().await;
        e.cl.poll_inbox(&e.mb).await.unwrap();
        assert_eq!(e.row().token, "NEW");
    }

    #[tokio::test]
    async fn second_poll_uses_the_rotated_token() {
        let e = oenv().await;
        // The same `Mailbox` value both times: the token comes from the row.
        e.cl.poll_inbox(&e.mb).await.unwrap();
        e.cl.poll_inbox(&e.mb).await.unwrap();
        assert_eq!(e.fake.lock().unwrap().refresh_seen, ["OLD", "NEW"]);
    }

    #[tokio::test]
    async fn rotated_token_is_saved_even_when_graph_fails() {
        let e = oenv().await;
        e.graph(503, serde_json::json!({}));
        assert!(e.cl.poll_inbox(&e.mb).await.unwrap().is_empty());
        let row = e.row();
        assert_eq!((row.token.as_str(), row.status.as_str()), ("NEW", "active"));
    }

    #[tokio::test]
    async fn poll_dedup() {
        let e = oenv().await;
        e.cl.poll_inbox(&e.mb).await.unwrap();
        assert!(e.cl.poll_inbox(&e.mb).await.unwrap().is_empty());
        assert_eq!(e.db.mail_list(e.mb.id).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn poll_invalid_grant_marks_gone() {
        let e = oenv().await;
        e.token(400, serde_json::json!({ "error": "invalid_grant" }));
        assert!(e.cl.poll_inbox(&e.mb).await.unwrap().is_empty());
        assert_eq!(e.row().status, "gone");
        // Strict too: a dead grant is an answer, not an error.
        MailClient::Outlook(&e.cl).check(&e.mb).await.unwrap();
    }

    #[tokio::test]
    async fn poll_other_400_not_gone() {
        for body in [
            serde_json::json!({ "error": "temporarily_unavailable" }),
            serde_json::json!({ "error": "invalid_request", "error_codes": [90033] }),
            serde_json::json!({}),
        ] {
            let e = oenv().await;
            e.token(400, body.clone());
            assert!(e.cl.poll_inbox(&e.mb).await.unwrap().is_empty(), "{body}");
            assert_eq!(e.row().status, "active", "{body}");
            e.token(401, body.clone());
            assert!(e.cl.poll_inbox(&e.mb).await.unwrap().is_empty(), "{body}");
            assert_eq!(e.row().status, "active", "{body}");
        }
    }

    #[tokio::test]
    async fn poll_429_and_5xx_are_transient() {
        let mut e = oenv().await;
        e.token(429, serde_json::json!({}));
        assert!(e.cl.poll_inbox(&e.mb).await.unwrap().is_empty());
        e.token(
            200,
            serde_json::json!({ "access_token": "AT", "refresh_token": "NEW" }),
        );
        e.graph(503, serde_json::json!({}));
        assert!(e.cl.poll_inbox(&e.mb).await.unwrap().is_empty());
        assert_eq!(e.row().status, "active");
        e.cl.grace.unavailable_after = Duration::ZERO;
        let err = e.cl.poll_inbox(&e.mb).await.unwrap_err();
        assert_eq!(err.0, "Outlook unavailable: inbox HTTP 503");
        e.token(429, serde_json::json!({}));
        let err = e.cl.poll_inbox(&e.mb).await.unwrap_err();
        assert_eq!(err.0, "Outlook unavailable: inbox token HTTP 429");
        assert_eq!(e.row().status, "active");
    }

    #[tokio::test]
    async fn poll_stores_html_only_for_html_content() {
        let e = oenv().await;
        let mut html = msg_json();
        html["body"] = serde_json::json!({ "contentType": "html", "content": "<p>hi</p>" });
        let mut text = msg_json();
        text["id"] = "AAA2".into();
        text["body"] = serde_json::json!({ "contentType": "text", "content": "plain" });
        e.graph(200, serde_json::json!({ "value": [html, text] }));
        e.cl.poll_inbox(&e.mb).await.unwrap();
        let rows: HashMap<_, _> =
            e.db.mail_list(e.mb.id)
                .unwrap()
                .into_iter()
                .map(|m| (m.ele_message_id.clone().unwrap(), m))
                .collect();
        assert_eq!(
            (rows["AAA1"].html.as_deref(), rows["AAA1"].body.as_deref()),
            (Some("<p>hi</p>"), Some("hi"))
        );
        let a = &rows["AAA1"];
        let code = extract_otp(&format!(
            "{}{}",
            a.subject.as_deref().unwrap(),
            a.body.as_deref().unwrap()
        ));
        assert_eq!(code.as_deref(), Some("ABC123"));
        assert_eq!(rows["AAA2"].html, None);
    }

    #[tokio::test]
    async fn reuse_mailbox_then_poll_returns_only_new() {
        let e = oenv().await;
        let mut old = msg_json();
        old["id"] = "OLD1".into();
        e.graph(200, serde_json::json!({ "value": [old.clone()] }));
        e.cl.reuse_mailbox(&e.mb).await.unwrap();
        assert_eq!(e.row().token, "NEW");
        let ids: Vec<_> =
            e.db.mail_list(e.mb.id)
                .unwrap()
                .into_iter()
                .map(|m| (m.ele_message_id.unwrap(), m.otp_extracted))
                .collect();
        assert_eq!(ids, [("OLD1".to_string(), Some("stale".to_string()))]);
        e.token(
            200,
            serde_json::json!({ "access_token": "AT", "refresh_token": "NEWER" }),
        );
        let mut n2 = msg_json();
        n2["id"] = "N2".into();
        e.graph(200, serde_json::json!({ "value": [n2, old] }));
        let new = e.cl.poll_inbox(&e.mb).await.unwrap();
        assert_eq!(
            new.iter()
                .map(|m| m.ele_message_id.as_deref().unwrap())
                .collect::<Vec<_>>(),
            ["N2"]
        );
        assert_eq!(e.row().token, "NEWER");
        e.token(500, serde_json::json!({}));
        assert!(e.cl.reuse_mailbox(&e.mb).await.is_err());
    }

    #[tokio::test]
    async fn wait_for_new_mail_on_outlook_drops_mail_at_or_before_the_cutoff() {
        let e = oenv().await;
        let at = |id: &str, t: &str| {
            let mut m = msg_json();
            m["id"] = id.into();
            m["receivedDateTime"] = t.into();
            m
        };
        let old = at("OLD1", "2026-11-19T10:00:00Z");
        e.graph(200, serde_json::json!({ "value": [old.clone()] }));
        let client = MailClient::Outlook(&e.cl);
        client.reuse_mailbox(&e.mb).await.unwrap();
        let pick = |m: &MailMessage| m.ele_message_id.clone();
        // A late old mail (dated at the cutoff and before it): dropped.
        e.graph(
            200,
            serde_json::json!({ "value": [old.clone(), at("L1", "2026-11-19T10:00:00Z"), at("L2", "2026-11-19T09:59:00Z")] }),
        );
        let got = wait_for_new_mail(client, &e.mb, Duration::ZERO, pick, FAST, None)
            .await
            .unwrap();
        assert_eq!(got, None);
        e.graph(
            200,
            serde_json::json!({ "value": [at("N1", "2026-11-19T10:01:00Z"), old] }),
        );
        let got = wait_for_new_mail(client, &e.mb, Duration::ZERO, pick, FAST, None)
            .await
            .unwrap();
        assert_eq!(got.as_deref(), Some("N1"));
    }

    #[tokio::test]
    async fn outlook_errors_hold_no_token() {
        let mut e = oenv().await;
        e.cl.grace.unavailable_after = Duration::ZERO;
        let mut errs = Vec::new();
        for (code, body) in [
            (
                500,
                serde_json::json!({ "error": "x", "refresh_token": "NEW" }),
            ),
            (429, serde_json::json!({})),
            (
                400,
                serde_json::json!({ "error": "temporarily_unavailable" }),
            ),
            (200, serde_json::json!({ "refresh_token": "NEW" })),
        ] {
            e.token(code, body);
            errs.push(e.cl.poll_inbox(&e.mb).await.unwrap_err());
        }
        e.token(
            200,
            serde_json::json!({ "access_token": "AT", "refresh_token": "NEW" }),
        );
        e.graph(503, serde_json::json!({ "error": "AT" }));
        errs.push(e.cl.poll_inbox(&e.mb).await.unwrap_err());
        e.graph(200, serde_json::json!(["not an object"]));
        errs.push(e.cl.poll_inbox(&e.mb).await.unwrap_err());
        // Nobody listening: a network error.
        let dead = OutlookClient::new(e.db.clone(), OutlookUrls::on("http://127.0.0.1:9")).unwrap();
        errs.push(MailClient::Outlook(&dead).check(&e.mb).await.unwrap_err());
        let at = Regex::new(r"\bAT\b").unwrap();
        for err in &errs {
            assert!(err.0.starts_with("Outlook unavailable: inbox "), "{err}");
            for secret in [
                "OLD",
                "NEW",
                "CID",
                "login.live.com",
                "graph.microsoft.com",
                "127.0.0.1",
                "http",
            ] {
                assert!(!err.0.contains(secret), "{err}");
            }
            assert!(!at.is_match(&err.0), "{err}");
        }
    }

    #[tokio::test]
    async fn mailbox_debug_masks_token_and_client_id() {
        let e = oenv().await;
        let s = format!("{:?}", e.mb);
        assert!(!s.contains("OLD") && !s.contains("CID"), "{s}");
        assert!(s.contains("***"), "{s}");
    }

    #[test]
    fn extract_otp_cases() {
        for (text, code) in [
            ("Your verification code is: 123456", Some("123456")),
            ("<p>G-482910 is your code</p>", Some("482910")),
            ("enter code 123 456", Some("123456")),
            ("nothing here", None),
            ("", None),
            (&html_text(HTML), Some("48219")),
            ("Hi\n123456\nthanks", Some("123456")),
            (
                "Fish Audio: Your verification code Your verification code is INSIDE this email. Code: 378554",
                Some("378554"),
            ),
            (
                "Fish Audio: Your verification code\nINSIDE\nverify your email 378554",
                Some("378554"),
            ),
            ("Welcome 2026, total $19.99, zip 90210", None),
        ] {
            assert_eq!(extract_otp(text).as_deref(), code, "{text:?}");
        }
    }
}
