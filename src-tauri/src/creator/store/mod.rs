//! Our store: the Python app's SQLite schema at `user_version = 24`, verbatim
//! (`schema_v24.sql`), so an import later is a copy of `ghidorah.sqlite` (D7).
//! Row functions are few and plain: only what M2 calls.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Result};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, Row};

const SCHEMA: &str = include_str!("schema_v24.sql");
pub const VERSION: i64 = 24;

/// One connection, shared; every call is short.
pub struct Db(Mutex<Connection>);

/// `store::local_root()/creator.sqlite`: this machine's, never roamed.
pub fn db_path() -> Result<PathBuf> {
    Ok(crate::store::local_root()?.join("creator.sqlite"))
}

/// Python `models.now()`: `datetime.now(UTC).isoformat(timespec="microseconds")`.
pub fn now() -> String {
    let d = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    iso(d.as_secs() as i64, d.subsec_micros())
}

fn iso(secs: i64, micros: u32) -> String {
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{micros:06}+00:00",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// An ISO 8601 date-time (Python `datetime.fromisoformat` forms:
/// `YYYY-MM-DD[T ]HH:MM[:SS[.f]][Z|±HH[:MM]]`) as microseconds since the
/// epoch; a value without an offset counts as UTC. None when unreadable or a
/// bare date.
pub fn parse_iso(s: &str) -> Option<i64> {
    let s = s.trim();
    let num = |a: usize, b: usize| -> Option<i64> {
        let t = s.get(a..b)?;
        t.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| t.parse().ok())?
    };
    if s.len() < 16 || s.as_bytes()[4] != b'-' || s.as_bytes()[7] != b'-' {
        return None;
    }
    if !matches!(s.as_bytes()[10], b'T' | b' ') || s.as_bytes()[13] != b':' {
        return None;
    }
    let (y, mo, d, h, mi) = (
        num(0, 4)?,
        num(5, 7)?,
        num(8, 10)?,
        num(11, 13)?,
        num(14, 16)?,
    );
    let mut rest = &s[16..];
    let mut sec = 0;
    let mut micros = 0;
    if let Some(r) = rest.strip_prefix(':') {
        sec = r.get(..2)?.parse::<i64>().ok()?;
        rest = &r[2..];
        if let Some(r) = rest.strip_prefix('.') {
            let n = r.bytes().take_while(u8::is_ascii_digit).count();
            if n == 0 {
                return None;
            }
            let digits: String = r[..n].chars().chain("000000".chars()).take(6).collect();
            micros = digits.parse::<i64>().ok()?;
            rest = &r[n..];
        }
    }
    let offset = match rest {
        "" | "Z" | "z" => 0,
        o if o.starts_with('+') || o.starts_with('-') => {
            let sign = if o.starts_with('-') { -1 } else { 1 };
            let hm = o[1..].replace(':', "");
            if hm.len() < 2 || !hm.bytes().all(|c| c.is_ascii_digit()) {
                return None;
            }
            let oh: i64 = hm[..2].parse().ok()?;
            let om: i64 = hm.get(2..4).map_or(Some(0), |t| t.parse().ok())?;
            sign * (oh * 3600 + om * 60)
        }
        _ => return None,
    };
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || sec > 59 {
        return None;
    }
    let secs = days_from_civil(y, mo, d) * 86400 + h * 3600 + mi * 60 + sec - offset;
    Some(secs * 1_000_000 + micros)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mailbox {
    pub id: i64,
    pub provider: String,
    pub email: String,
    pub domain: String,
    pub token: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MailMessage {
    pub id: i64,
    pub mailbox_id: i64,
    pub ele_message_id: Option<String>,
    pub sender: Option<String>,
    pub subject: Option<String>,
    pub body: Option<String>,
    pub otp_extracted: Option<String>,
    pub received_at: Option<String>,
    pub html: Option<String>,
}

/// A mail to store (`mail_create`).
#[derive(Debug, Clone, Default)]
pub struct NewMail {
    pub ele_message_id: Option<String>,
    pub sender: String,
    pub subject: String,
    pub body: String,
    pub html: Option<String>,
    pub received_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Proxy {
    pub id: i64,
    pub host: String,
    pub port: i64,
    pub username: Option<String>,
    pub password: Option<String>,
    pub active: bool,
    /// 'unknown' | 'healthy' | 'slow' | 'failing'.
    pub status: String,
    pub last_used_at: Option<String>,
    pub refused_at: Option<String>,
    /// 'static' | 'rotating'.
    pub kind: String,
    pub signups_ok: i64,
    pub signups_total: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Browser {
    pub id: i64,
    pub mailbox_id: i64,
    pub name: Option<String>,
    /// The ShardBrowser profile id (None until its first use).
    pub path: Option<String>,
    pub proxy_id: Option<i64>,
    pub proxy_mode: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Account {
    pub id: i64,
    pub mailbox_id: i64,
    pub platform: String,
    pub route: String,
    pub job_id: Option<i64>,
    pub status: String,
    pub step: Option<String>,
    pub step_note: Option<String>,
    pub error: Option<String>,
    pub error_code: Option<String>,
    pub screenshot: Option<String>,
    /// JSON text, non-secret.
    pub extra: String,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    pub created_at: String,
}

/// The account columns `account_update` writes. `Some(None)` writes NULL.
#[derive(Debug, Clone, Default)]
pub struct AccountPatch {
    pub status: Option<String>,
    pub step: Option<Option<String>>,
    pub step_note: Option<Option<String>>,
    pub error: Option<Option<String>>,
    pub error_code: Option<Option<String>>,
    pub screenshot: Option<Option<String>>,
    pub started_at: Option<Option<String>>,
    pub finished_at: Option<Option<String>>,
    pub verify_status: Option<Option<String>>,
    pub verified_at: Option<Option<String>>,
}

/// The Accounts list's filters (`account_list`); `group` is a status group
/// (`linked`, `signed_out`, `making`, `failed`).
#[derive(Debug, Clone, Default)]
pub struct AccountQuery {
    pub platform: Option<String>,
    pub group: Option<String>,
    pub flag: Option<String>,
    pub q: String,
    /// `newest` (default), `oldest`, `email`, `status`.
    pub order: String,
    pub offset: i64,
    pub limit: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub id: i64,
    pub browser_id: Option<i64>,
    pub mailbox_id: Option<i64>,
    pub level: String,
    pub step: Option<String>,
    pub message: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImportCounts {
    pub proxies: usize,
    pub blocked_domains: usize,
    pub settings: usize,
}

/// The settings `import_basics` takes from a Python database.
const IMPORTED_SETTINGS: [&str; 3] = ["ele_api_url", "ele_api_key", "proxy_rest_seconds"];

const MAILBOX: &str = "id, provider, email, domain, token, status, created_at";
const MAIL: &str =
    "id, mailbox_id, ele_message_id, sender, subject, body, otp_extracted, received_at, html";
const PROXY: &str = "id, host, port, username, password, active, status, last_used_at, refused_at, kind, signups_ok, signups_total";
const BROWSER: &str = "id, mailbox_id, name, path, proxy_id, proxy_mode";
const ACCOUNT: &str = "id, mailbox_id, platform, route, job_id, status, step, step_note, error, error_code, screenshot, extra, started_at, finished_at, created_at";
const EVENT: &str = "id, browser_id, mailbox_id, level, step, message, created_at";

fn mailbox(r: &Row) -> rusqlite::Result<Mailbox> {
    Ok(Mailbox {
        id: r.get(0)?,
        provider: r.get(1)?,
        email: r.get(2)?,
        domain: r.get(3)?,
        token: r.get(4)?,
        status: r.get(5)?,
        created_at: r.get(6)?,
    })
}

fn mail(r: &Row) -> rusqlite::Result<MailMessage> {
    Ok(MailMessage {
        id: r.get(0)?,
        mailbox_id: r.get(1)?,
        ele_message_id: r.get(2)?,
        sender: r.get(3)?,
        subject: r.get(4)?,
        body: r.get(5)?,
        otp_extracted: r.get(6)?,
        received_at: r.get(7)?,
        html: r.get(8)?,
    })
}

fn proxy(r: &Row) -> rusqlite::Result<Proxy> {
    Ok(Proxy {
        id: r.get(0)?,
        host: r.get(1)?,
        port: r.get(2)?,
        username: r.get(3)?,
        password: r.get(4)?,
        active: r.get::<_, i64>(5)? != 0,
        status: r.get(6)?,
        last_used_at: r.get(7)?,
        refused_at: r.get(8)?,
        kind: r.get(9)?,
        signups_ok: r.get(10)?,
        signups_total: r.get(11)?,
    })
}

fn browser(r: &Row) -> rusqlite::Result<Browser> {
    Ok(Browser {
        id: r.get(0)?,
        mailbox_id: r.get(1)?,
        name: r.get(2)?,
        path: r.get(3)?,
        proxy_id: r.get(4)?,
        proxy_mode: r.get(5)?,
    })
}

fn account(r: &Row) -> rusqlite::Result<Account> {
    Ok(Account {
        id: r.get(0)?,
        mailbox_id: r.get(1)?,
        platform: r.get(2)?,
        route: r.get(3)?,
        job_id: r.get(4)?,
        status: r.get(5)?,
        step: r.get(6)?,
        step_note: r.get(7)?,
        error: r.get(8)?,
        error_code: r.get(9)?,
        screenshot: r.get(10)?,
        extra: r.get(11)?,
        started_at: r.get(12)?,
        finished_at: r.get(13)?,
        created_at: r.get(14)?,
    })
}

fn event(r: &Row) -> rusqlite::Result<Event> {
    Ok(Event {
        id: r.get(0)?,
        browser_id: r.get(1)?,
        mailbox_id: r.get(2)?,
        level: r.get(3)?,
        step: r.get(4)?,
        message: r.get(5)?,
        created_at: r.get(6)?,
    })
}

/// Python `_json_path`: `$."key"`.
fn json_path(key: &str) -> String {
    format!("$.\"{}\"", key.replace('"', "\"\""))
}

impl Db {
    /// A new file gets the v24 schema; a file at 24 opens as is; any other
    /// version is refused (an older Python database is M4's import).
    pub fn open(path: &Path) -> Result<Db> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(Duration::from_secs(30))?;
        conn.query_row("PRAGMA journal_mode=WAL", [], |_| Ok(()))?;
        conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=NORMAL;")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        let tables: i64 = conn.query_row("SELECT COUNT(*) FROM sqlite_master", [], |r| r.get(0))?;
        match version {
            0 if tables == 0 => conn.execute_batch(&format!(
                "BEGIN;\n{SCHEMA}\nPRAGMA user_version={VERSION};\nCOMMIT;"
            ))?,
            VERSION => {}
            n => bail!("database version {n} not supported"),
        }
        Ok(Db(Mutex::new(conn)))
    }

    fn c(&self) -> MutexGuard<'_, Connection> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn settings_get(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .c()
            .query_row("SELECT value FROM settings WHERE key=?", [key], |r| {
                r.get(0)
            })
            .optional()?
            .flatten())
    }

    pub fn settings_set(&self, key: &str, value: &str) -> Result<()> {
        self.c().execute(
            "INSERT INTO settings(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn proxies_list(&self) -> Result<Vec<Proxy>> {
        let c = self.c();
        let mut st = c.prepare(&format!("SELECT {PROXY} FROM proxies ORDER BY id"))?;
        let rows = st.query_map([], proxy)?.collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn proxy_get(&self, id: i64) -> Result<Option<Proxy>> {
        Ok(self
            .c()
            .query_row(
                &format!("SELECT {PROXY} FROM proxies WHERE id=?"),
                [id],
                proxy,
            )
            .optional()?)
    }

    /// A test or an import adds proxies; the Proxies page (M7) will too.
    pub fn proxy_add(
        &self,
        host: &str,
        port: i64,
        username: Option<&str>,
        password: Option<&str>,
        kind: &str,
    ) -> Result<i64> {
        let c = self.c();
        c.execute(
            "INSERT INTO proxies (host, port, username, password, kind, created_at) VALUES (?,?,?,?,?,?)",
            params![host, port, username, password, kind, now()],
        )?;
        Ok(c.last_insert_rowid())
    }

    /// `last_used_at = now()`: the rotate order.
    pub fn proxy_touch(&self, id: i64) -> Result<()> {
        self.c().execute(
            "UPDATE proxies SET last_used_at=? WHERE id=?",
            params![now(), id],
        )?;
        Ok(())
    }

    /// One sign-up on the proxy ended: `signups_total + 1`, `signups_ok + ok`.
    pub fn proxy_signup(&self, id: i64, ok: bool) -> Result<()> {
        self.c().execute(
            "UPDATE proxies SET signups_total=signups_total+1, signups_ok=signups_ok+? WHERE id=?",
            params![i64::from(ok), id],
        )?;
        Ok(())
    }

    /// Browsers per proxy (the `least` pick).
    pub fn browsers_per_proxy(&self) -> Result<std::collections::HashMap<i64, i64>> {
        let c = self.c();
        let mut st = c.prepare(
            "SELECT proxy_id, COUNT(*) FROM browsers WHERE proxy_id IS NOT NULL GROUP BY proxy_id",
        )?;
        let rows = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// Lower-cased, as Python stores them.
    pub fn blocked_domains(&self) -> Result<HashSet<String>> {
        let c = self.c();
        let mut st = c.prepare("SELECT domain FROM blocked_domains")?;
        let rows = st
            .query_map([], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn blocked_domain_add(&self, domain: &str, reason: Option<&str>) -> Result<()> {
        self.c().execute(
            "INSERT OR IGNORE INTO blocked_domains(domain, reason, added_at) VALUES (?,?,?)",
            params![domain.trim().to_lowercase(), reason, now()],
        )?;
        Ok(())
    }

    pub fn mailbox_create(
        &self,
        provider: &str,
        email: &str,
        domain: &str,
        token: &str,
    ) -> Result<Mailbox> {
        let id = {
            let c = self.c();
            c.execute(
                "INSERT INTO mailboxes (provider, email, domain, token, created_at) VALUES (?,?,?,?,?)",
                params![provider, email, domain, token, now()],
            )?;
            c.last_insert_rowid()
        };
        self.mailbox_get(id)?
            .ok_or_else(|| anyhow::anyhow!("mailbox {id} vanished"))
    }

    pub fn mailbox_get(&self, id: i64) -> Result<Option<Mailbox>> {
        Ok(self
            .c()
            .query_row(
                &format!("SELECT {MAILBOX} FROM mailboxes WHERE id=?"),
                [id],
                mailbox,
            )
            .optional()?)
    }

    /// Writes only a change (the revs triggers count real writes).
    pub fn mailbox_set_status(&self, id: i64, status: &str) -> Result<()> {
        self.c().execute(
            "UPDATE mailboxes SET status=?1 WHERE id=?2 AND status<>?1",
            params![status, id],
        )?;
        Ok(())
    }

    pub fn mail_list(&self, mailbox_id: i64) -> Result<Vec<MailMessage>> {
        let c = self.c();
        let mut st = c.prepare(&format!(
            "SELECT {MAIL} FROM mail_messages WHERE mailbox_id=? ORDER BY id"
        ))?;
        let rows = st
            .query_map([mailbox_id], mail)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    pub fn mail_create(&self, mailbox_id: i64, m: &NewMail) -> Result<MailMessage> {
        let c = self.c();
        c.execute(
            "INSERT INTO mail_messages (mailbox_id, ele_message_id, sender, subject, body, html, received_at, stored_at) VALUES (?,?,?,?,?,?,?,?)",
            params![mailbox_id, m.ele_message_id, m.sender, m.subject, m.body, m.html, m.received_at, now()],
        )?;
        Ok(c.query_row(
            &format!("SELECT {MAIL} FROM mail_messages WHERE id=?"),
            [c.last_insert_rowid()],
            mail,
        )?)
    }

    pub fn mail_update_body(&self, id: i64, body: &str) -> Result<()> {
        self.c().execute(
            "UPDATE mail_messages SET body=? WHERE id=?",
            params![body, id],
        )?;
        Ok(())
    }

    pub fn mail_set_otp(&self, id: i64, otp: &str) -> Result<()> {
        self.c().execute(
            "UPDATE mail_messages SET otp_extracted=? WHERE id=?",
            params![otp, id],
        )?;
        Ok(())
    }

    pub fn job_create(&self, platform: &str, count: i64, proxy_mode: &str) -> Result<i64> {
        let c = self.c();
        c.execute(
            "INSERT INTO jobs (platform, count, proxy_mode, created_at) VALUES (?,?,?,?)",
            params![platform, count, proxy_mode, now()],
        )?;
        Ok(c.last_insert_rowid())
    }

    pub fn browser_create(
        &self,
        mailbox_id: i64,
        proxy_id: Option<i64>,
        proxy_mode: &str,
    ) -> Result<i64> {
        let c = self.c();
        let ts = now();
        c.execute(
            "INSERT INTO browsers (mailbox_id, proxy_id, proxy_mode, created_at, updated_at) VALUES (?,?,?,?,?)",
            params![mailbox_id, proxy_id, proxy_mode, ts, ts],
        )?;
        Ok(c.last_insert_rowid())
    }

    pub fn browser_get(&self, id: i64) -> Result<Option<Browser>> {
        Ok(self
            .c()
            .query_row(
                &format!("SELECT {BROWSER} FROM browsers WHERE id=?"),
                [id],
                browser,
            )
            .optional()?)
    }

    /// A mailbox has at most one browser.
    pub fn browser_of_mailbox(&self, mailbox_id: i64) -> Result<Option<Browser>> {
        Ok(self
            .c()
            .query_row(
                &format!("SELECT {BROWSER} FROM browsers WHERE mailbox_id=?"),
                [mailbox_id],
                browser,
            )
            .optional()?)
    }

    /// The ShardBrowser profile id (`browsers.path`); None clears it.
    pub fn browser_set_path(&self, id: i64, profile_id: Option<&str>) -> Result<()> {
        self.c().execute(
            "UPDATE browsers SET path=?, updated_at=? WHERE id=?",
            params![profile_id, now(), id],
        )?;
        Ok(())
    }

    pub fn browser_set_proxy(&self, id: i64, proxy_id: Option<i64>) -> Result<()> {
        self.c().execute(
            "UPDATE browsers SET proxy_id=?, updated_at=? WHERE id=?",
            params![proxy_id, now(), id],
        )?;
        Ok(())
    }

    pub fn account_create(
        &self,
        mailbox_id: i64,
        platform: &str,
        route: &str,
        job_id: Option<i64>,
    ) -> Result<i64> {
        let c = self.c();
        let ts = now();
        c.execute(
            "INSERT INTO accounts (mailbox_id, platform, route, job_id, created_at, updated_at) VALUES (?,?,?,?,?,?)",
            params![mailbox_id, platform, route, job_id, ts, ts],
        )?;
        Ok(c.last_insert_rowid())
    }

    pub fn account_get(&self, id: i64) -> Result<Option<Account>> {
        Ok(self
            .c()
            .query_row(
                &format!("SELECT {ACCOUNT} FROM accounts WHERE id=?"),
                [id],
                account,
            )
            .optional()?)
    }

    pub fn account_update(&self, id: i64, p: &AccountPatch) -> Result<()> {
        let mut sets: Vec<&str> = Vec::new();
        let mut vals: Vec<Option<String>> = Vec::new();
        let mut put = |col: &'static str, v: &Option<Option<String>>| {
            if let Some(v) = v {
                sets.push(col);
                vals.push(v.clone());
            }
        };
        put("status", &p.status.clone().map(Some));
        put("step", &p.step);
        put("step_note", &p.step_note);
        put("error", &p.error);
        put("error_code", &p.error_code);
        put("screenshot", &p.screenshot);
        put("started_at", &p.started_at);
        put("finished_at", &p.finished_at);
        put("verify_status", &p.verify_status);
        put("verified_at", &p.verified_at);
        let sql = format!(
            "UPDATE accounts SET {}updated_at=? WHERE id=?",
            sets.iter().map(|c| format!("{c}=?, ")).collect::<String>()
        );
        let mut args: Vec<&dyn rusqlite::ToSql> = vals.iter().map(|v| v as _).collect();
        let ts = now();
        args.push(&ts);
        args.push(&id);
        self.c().execute(&sql, args.as_slice())?;
        Ok(())
    }

    /// A value of `accounts.secrets` (JSON); never logged.
    pub fn account_secret(&self, id: i64, key: &str) -> Result<Option<String>> {
        Ok(self
            .c()
            .query_row(
                "SELECT json_extract(secrets, ?) FROM accounts WHERE id=?",
                params![json_path(key), id],
                |r| r.get(0),
            )
            .optional()?
            .flatten())
    }

    pub fn account_set_secret(&self, id: i64, key: &str, value: &str) -> Result<()> {
        self.c().execute(
            "UPDATE accounts SET secrets=json_set(secrets, ?, ?), updated_at=? WHERE id=?",
            params![json_path(key), value, now(), id],
        )?;
        Ok(())
    }

    /// Sets each key of the object `values` in `accounts.extra` (Python `set_extra(**values)`).
    pub fn account_set_extra(&self, id: i64, values: &serde_json::Value) -> Result<()> {
        let Some(obj) = values.as_object() else {
            bail!("extra must be an object");
        };
        let c = self.c();
        for (k, v) in obj {
            c.execute(
                "UPDATE accounts SET extra=json_set(extra, ?, json(?)), updated_at=? WHERE id=?",
                params![json_path(k), v.to_string(), now(), id],
            )?;
        }
        Ok(())
    }

    /// Newest first.
    pub fn accounts_recent(&self, platform: &str, limit: i64) -> Result<Vec<Account>> {
        let c = self.c();
        let mut st = c.prepare(&format!(
            "SELECT {ACCOUNT} FROM accounts WHERE platform=? ORDER BY id DESC LIMIT ?"
        ))?;
        let rows = st
            .query_map(params![platform, limit], account)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }

    /// One window of the Accounts list (`AccountList` in `src/creator/types.ts`):
    /// the rows (never a secret), the total under the filters, the count per
    /// platform and per status group (search and flag applied, group not).
    pub fn account_list(&self, f: &AccountQuery) -> Result<serde_json::Value> {
        use rusqlite::types::Value as V;
        use serde_json::json;
        const GROUP: &str = "CASE WHEN a.status='linked' AND a.verify_status IN ('signed_out','expired') \
             THEN 'signed_out' WHEN a.status IN ('queued','running') THEN 'making' ELSE a.status END";
        const FROM: &str = "FROM accounts a JOIN mailboxes m ON m.id=a.mailbox_id";
        let mut wh = vec!["1=1".to_string()];
        let mut args: Vec<V> = Vec::new();
        if let Some(p) = &f.platform {
            wh.push("a.platform=?".into());
            args.push(V::Text(p.clone()));
        }
        let q = f.q.trim();
        if !q.is_empty() {
            let esc = q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_");
            wh.push("m.email LIKE ? ESCAPE '\\'".into());
            args.push(V::Text(format!("%{esc}%")));
        }
        match f.flag.as_deref() {
            Some("frozen") => wh.push("a.frozen=1".into()),
            Some("pushed") => wh.push("a.relay_id IS NOT NULL".into()),
            Some("not_pushed") => wh.push("a.relay_id IS NULL".into()),
            _ => {}
        }
        let wh = wh.join(" AND ");
        let c = self.c();
        let mut counts = json!({ "all": 0, "linked": 0, "signed_out": 0, "making": 0, "failed": 0 });
        {
            let mut st = c.prepare(&format!("SELECT {GROUP}, COUNT(*) {FROM} WHERE {wh} GROUP BY 1"))?;
            let mut rows = st.query(rusqlite::params_from_iter(&args))?;
            let mut all = 0;
            while let Some(r) = rows.next()? {
                let (g, n): (String, i64) = (r.get(0)?, r.get(1)?);
                all += n;
                if counts.get(&g).is_some() {
                    counts[g] = n.into();
                }
            }
            counts["all"] = all.into();
        }
        let tabs: Vec<serde_json::Value> = c
            .prepare("SELECT platform, COUNT(*) FROM accounts GROUP BY platform ORDER BY platform")?
            .query_map([], |r| Ok(json!({ "platform": r.get::<_, String>(0)?, "count": r.get::<_, i64>(1)? })))?
            .collect::<rusqlite::Result<_>>()?;
        let mut wh = wh;
        let total = match &f.group {
            Some(g) => {
                wh.push_str(&format!(" AND {GROUP}=?"));
                args.push(V::Text(g.clone()));
                counts.get(g).and_then(|v| v.as_i64()).unwrap_or(0)
            }
            None => counts["all"].as_i64().unwrap_or(0),
        };
        let order = match f.order.as_str() {
            "oldest" => "a.id",
            "email" => "m.email, a.id",
            "status" => "2, a.id DESC",
            _ => "a.id DESC",
        };
        args.push(V::Integer(f.limit));
        args.push(V::Integer(f.offset));
        let mut st = c.prepare(&format!(
            "SELECT a.id, {GROUP}, a.mailbox_id, m.email, m.provider, a.platform, a.status, a.route, \
             a.frozen, a.relay_id, a.error, a.error_code, a.verify_status, b.id, b.path, p.host, p.port, \
             p.country, a.step_note, a.created_at {FROM} \
             LEFT JOIN browsers b ON b.mailbox_id=a.mailbox_id LEFT JOIN proxies p ON p.id=b.proxy_id \
             WHERE {wh} ORDER BY {order} LIMIT ? OFFSET ?"
        ))?;
        let items: Vec<serde_json::Value> = st
            .query_map(rusqlite::params_from_iter(&args), |r| {
                let status: String = r.get(6)?;
                let code: Option<String> = r.get(11)?;
                let host: Option<String> = r.get(15)?;
                let port: Option<i64> = r.get(16)?;
                Ok(json!({
                    "id": r.get::<_, i64>(0)?,
                    "mailbox_id": r.get::<_, i64>(2)?,
                    "email": r.get::<_, String>(3)?,
                    "mailbox_type": r.get::<_, String>(4)?,
                    "platform": r.get::<_, String>(5)?,
                    "route": r.get::<_, String>(7)?,
                    "frozen": r.get::<_, i64>(8)? != 0,
                    "busy": status == "queued" || status == "running",
                    "relay_id": r.get::<_, Option<String>>(9)?,
                    "error": r.get::<_, Option<String>>(10)?,
                    "fail": code.as_deref().map(|c| {
                        let (group, retryable) = super::errors::fail_info(c);
                        json!({ "code": c, "group": group, "retryable": retryable })
                    }),
                    "verify_status": r.get::<_, Option<String>>(12)?,
                    "browser_id": r.get::<_, Option<i64>>(13)?,
                    "profile_id": r.get::<_, Option<String>>(14)?,
                    "proxy": host.map(|h| format!("{h}:{}", port.unwrap_or(0))),
                    "proxy_cc": r.get::<_, Option<String>>(17)?.filter(|c| !c.is_empty()),
                    // What a running account is doing now; a finished one's last note is history.
                    "step": if status == "running" { r.get::<_, Option<String>>(18)? } else { None },
                    "created_at": r.get::<_, String>(19)?,
                    "status": status,
                }))
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(json!({ "items": items, "total": total, "offset": f.offset, "tabs": tabs, "counts": counts }))
    }

    /// A Make run's progress: linked and failed accounts of `job`, and the
    /// step note of the one running now.
    pub fn job_progress(&self, job: i64) -> Result<(i64, i64, Option<String>)> {
        Ok(self.c().query_row(
            "SELECT COUNT(*) FILTER (WHERE status='linked'), COUNT(*) FILTER (WHERE status='failed'), \
             (SELECT step_note FROM accounts WHERE job_id=?1 AND status='running' ORDER BY id DESC LIMIT 1) \
             FROM accounts WHERE job_id=?1",
            [job],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?)
    }

    /// The sidebar's counts: accounts, mailboxes, proxies.
    pub fn nav_counts(&self) -> Result<(i64, i64, i64)> {
        Ok(self.c().query_row(
            "SELECT (SELECT COUNT(*) FROM accounts), (SELECT COUNT(*) FROM mailboxes), (SELECT COUNT(*) FROM proxies)",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?)
    }

    /// Never pass a secret in `message` (Global Constraints).
    pub fn event(
        &self,
        browser_id: Option<i64>,
        mailbox_id: Option<i64>,
        level: &str,
        step: Option<&str>,
        message: &str,
    ) -> Result<()> {
        self.c().execute(
            "INSERT INTO events (browser_id, mailbox_id, level, step, message, created_at) VALUES (?,?,?,?,?,?)",
            params![browser_id, mailbox_id, level, step, message, now()],
        )?;
        Ok(())
    }

    /// The account's events: its mailbox's, from the account's creation on.
    // ponytail: by mailbox and time, as events carry no account id; two
    // platforms run at once on one mailbox would show each other's events.
    pub fn events_of(&self, account_id: i64) -> Result<Vec<Event>> {
        let c = self.c();
        let mut st = c.prepare(&format!(
            "SELECT {EVENT} FROM events WHERE mailbox_id=(SELECT mailbox_id FROM accounts WHERE id=?1) \
             AND created_at >= (SELECT created_at FROM accounts WHERE id=?1) ORDER BY id"
        ))?;
        let rows = st
            .query_map([account_id], event)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(rows)
    }
}

/// Seed a fresh database from a Python one (D7, M2): proxies (one already
/// here by host, port and user is skipped; `kind` kept, `last_used_at`
/// reset), blocked domains, and the Ele and proxy-rest settings. The source
/// is opened read-only.
pub fn import_basics(db: &Db, src: &Path) -> Result<ImportCounts> {
    let s = Connection::open_with_flags(src, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let mut n = ImportCounts::default();
    let mut c = db.c();
    let tx = c.transaction()?;
    {
        let cols = "host, port, username, password, active, status, reason, latency_ms, exit_ip, country, \
                    fail_streak, checked_at, signups_ok, signups_total, created_at, org, refused_at, kind";
        let mut read = s.prepare(&format!("SELECT {cols} FROM proxies ORDER BY id"))?;
        let mut rows = read.query([])?;
        while let Some(r) = rows.next()? {
            let (host, port, user): (String, i64, Option<String>) =
                (r.get(0)?, r.get(1)?, r.get(2)?);
            let here: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM proxies WHERE host=? AND port=? AND username IS ?)",
                params![host, port, user],
                |q| q.get(0),
            )?;
            if here {
                continue;
            }
            let vals: Vec<rusqlite::types::Value> =
                (0..18).map(|i| r.get(i)).collect::<rusqlite::Result<_>>()?;
            tx.execute(
                &format!(
                    "INSERT INTO proxies ({cols}) VALUES ({})",
                    ["?"; 18].join(",")
                ),
                rusqlite::params_from_iter(vals),
            )?;
            n.proxies += 1;
        }
        let mut read = s.prepare("SELECT domain, reason, added_at FROM blocked_domains")?;
        let mut rows = read.query([])?;
        while let Some(r) = rows.next()? {
            let (d, reason, at): (String, Option<String>, String) =
                (r.get(0)?, r.get(1)?, r.get(2)?);
            n.blocked_domains += tx.execute(
                "INSERT OR IGNORE INTO blocked_domains(domain, reason, added_at) VALUES (?,?,?)",
                params![d, reason, at],
            )?;
        }
        for key in IMPORTED_SETTINGS {
            let v: Option<Option<String>> = s
                .query_row("SELECT value FROM settings WHERE key=?", [key], |r| {
                    r.get(0)
                })
                .optional()?;
            if let Some(Some(v)) = v {
                tx.execute(
                    "INSERT INTO settings(key,value) VALUES(?,?) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                    params![key, v],
                )?;
                n.settings += 1;
            }
        }
    }
    tx.commit()?;
    Ok(n)
}

#[cfg(test)]
impl Db {
    /// A test sets a column no row function writes.
    pub(crate) fn exec_for_test(&self, sql: &str) {
        self.c().execute_batch(sql).unwrap();
    }
}

#[cfg(test)]
pub(crate) fn temp_db() -> (Db, TempFile) {
    let path =
        std::env::temp_dir().join(format!("creator-{}.sqlite", uuid::Uuid::new_v4().simple()));
    (Db::open(&path).expect("open a temp db"), TempFile(path))
}

/// Removes the database and its WAL files on drop.
#[cfg(test)]
pub(crate) struct TempFile(pub PathBuf);

#[cfg(test)]
impl Drop for TempFile {
    fn drop(&mut self) {
        for ext in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{ext}", self.0.display()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Python's v24 tables (`sqlite_master`, minus ANALYZE's `sqlite_stat*`).
    const PYTHON_TABLES: [&str; 13] = [
        "account_checks",
        "accounts",
        "blocked_domains",
        "browsers",
        "events",
        "jobs",
        "mail_messages",
        "mailboxes",
        "platform_settings",
        "proxies",
        "proxy_moves",
        "revs",
        "settings",
    ];

    fn names(c: &Connection, kind: &str) -> Vec<String> {
        let mut st = c
            .prepare("SELECT name FROM sqlite_master WHERE type=? ORDER BY name")
            .unwrap();
        st.query_map([kind], |r| r.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    #[test]
    fn new_db_is_v24_and_matches_python_tables() {
        let (db, _f) = temp_db();
        let c = db.c();
        assert_eq!(
            c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            24
        );
        assert_eq!(names(&c, "table"), PYTHON_TABLES);
        assert_eq!(names(&c, "trigger").len(), 41);
        assert_eq!(
            names(&c, "index")
                .iter()
                .filter(|n| !n.starts_with("sqlite_autoindex"))
                .count(),
            21
        );
        let revs: i64 = c
            .query_row("SELECT COUNT(*) FROM revs", [], |r| r.get(0))
            .unwrap();
        assert_eq!(revs, 13);
        assert_eq!(
            c.query_row("PRAGMA foreign_keys", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            1
        );
        assert_eq!(
            c.query_row("PRAGMA journal_mode", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "wal"
        );
    }

    #[test]
    fn reopening_a_v24_file_keeps_its_rows() {
        let (db, f) = temp_db();
        db.settings_set("ele_api_url", "u").unwrap();
        drop(db);
        let db = Db::open(&f.0).unwrap();
        assert_eq!(
            db.settings_get("ele_api_url").unwrap().as_deref(),
            Some("u")
        );
    }

    #[test]
    fn refuses_other_versions() {
        for v in [23, 25] {
            let (db, f) = temp_db();
            db.c()
                .execute_batch(&format!("PRAGMA user_version={v}"))
                .unwrap();
            drop(db);
            let err = Db::open(&f.0).err().expect("refused").to_string();
            assert_eq!(err, format!("database version {v} not supported"));
        }
    }

    #[test]
    fn import_basics_copies_and_skips_duplicates() {
        let (src, sf) = temp_db();
        src.proxy_add("1.1.1.1", 80, Some("u"), Some("p"), "rotating")
            .unwrap();
        src.proxy_add("2.2.2.2", 81, None, None, "static").unwrap();
        src.proxy_touch(1).unwrap();
        src.blocked_domain_add("Bad.com", Some("x")).unwrap();
        src.settings_set("ele_api_url", "https://ele").unwrap();
        src.settings_set("ele_api_key", "KEY").unwrap();
        src.settings_set("max_chrome", "9").unwrap();
        drop(src);

        let (db, _f) = temp_db();
        db.proxy_add("2.2.2.2", 81, None, None, "static").unwrap();
        let n = import_basics(&db, &sf.0).unwrap();
        assert_eq!(
            n,
            ImportCounts {
                proxies: 1,
                blocked_domains: 1,
                settings: 2
            }
        );
        let px = db.proxies_list().unwrap();
        assert_eq!(px.len(), 2);
        let got = px.iter().find(|p| p.host == "1.1.1.1").unwrap();
        assert_eq!(
            (got.kind.as_str(), got.last_used_at.as_ref()),
            ("rotating", None)
        );
        assert_eq!(got.password.as_deref(), Some("p"));
        assert_eq!(
            db.blocked_domains().unwrap(),
            HashSet::from(["bad.com".to_string()])
        );
        assert_eq!(
            db.settings_get("ele_api_key").unwrap().as_deref(),
            Some("KEY")
        );
        assert_eq!(db.settings_get("max_chrome").unwrap(), None);
        // Again: nothing new.
        assert_eq!(
            import_basics(&db, &sf.0).unwrap(),
            ImportCounts {
                proxies: 0,
                blocked_domains: 0,
                settings: 2
            }
        );
    }

    #[test]
    fn now_is_pythons_format_and_parses_back() {
        assert_eq!(iso(0, 5), "1970-01-01T00:00:00.000005+00:00");
        assert_eq!(
            iso(1_793_000_000, 123_456),
            "2026-10-26T07:33:20.123456+00:00"
        );
        let t = now();
        assert_eq!(t.len(), 32, "{t}");
        assert!(parse_iso(&t).is_some());
        assert_eq!(
            parse_iso("2026-10-26T07:33:20.123456+00:00"),
            Some(1_793_000_000_123_456)
        );
        assert_eq!(
            parse_iso("2026-10-26 09:33:20+02:00"),
            Some(1_793_000_000_000_000)
        );
        assert_eq!(
            parse_iso("2026-10-26T05:33:20Z"),
            parse_iso("2026-10-26T05:33:20")
        );
        assert_eq!(parse_iso("2026-10-26T07:33"), Some(1_792_999_980_000_000));
        for bad in ["", "2026-10-25", "yesterday", "2026-10-26T05:33:20 UTC"] {
            assert_eq!(parse_iso(bad), None, "{bad}");
        }
    }

    #[test]
    fn account_rows_secrets_and_extra() {
        let (db, _f) = temp_db();
        let mb = db.mailbox_create("ele", "u@a.com", "a.com", "tok").unwrap();
        let job = db.job_create("breezeblue", 1, "rotate").unwrap();
        let id = db
            .account_create(mb.id, "breezeblue", "email", Some(job))
            .unwrap();
        db.account_update(
            id,
            &AccountPatch {
                status: Some("running".into()),
                error: Some(Some("x".into())),
                started_at: Some(Some(now())),
                ..Default::default()
            },
        )
        .unwrap();
        db.account_update(
            id,
            &AccountPatch {
                error: Some(None),
                ..Default::default()
            },
        )
        .unwrap();
        let a = db.account_get(id).unwrap().unwrap();
        assert_eq!(
            (a.status.as_str(), a.error, a.job_id),
            ("running", None, Some(job))
        );
        assert!(a.started_at.is_some());
        assert_eq!(db.account_secret(id, "password").unwrap(), None);
        db.account_set_secret(id, "password", "pw").unwrap();
        assert_eq!(
            db.account_secret(id, "password").unwrap().as_deref(),
            Some("pw")
        );
        db.account_set_extra(id, &serde_json::json!({ "api_key_untested": true, "n": 2 }))
            .unwrap();
        let extra: serde_json::Value =
            serde_json::from_str(&db.account_get(id).unwrap().unwrap().extra).unwrap();
        assert_eq!(
            extra,
            serde_json::json!({ "api_key_untested": true, "n": 2 })
        );
        db.event(None, Some(mb.id), "info", Some("mail"), "sign up by email")
            .unwrap();
        assert_eq!(db.events_of(id).unwrap().len(), 1);
        assert_eq!(db.accounts_recent("breezeblue", 10).unwrap().len(), 1);
    }
}
