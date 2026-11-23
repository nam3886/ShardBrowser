//! Our pages' commands (`creator_call`), answering in the shapes of
//! `src/creator/types.ts`, as `npm run dev:mock` does from `mock.ts`. Only
//! what Breeze Blue needs (M2); the other pages stay on mock data until M7.
//! Never a secret in a list or a Peek: a password or key is read only when
//! Copy is pressed (`account_secret`), the Ele key is only "saved" or not.

use std::path::Path;
use std::sync::atomic::Ordering::SeqCst;
use std::time::Duration;

use anyhow::{anyhow, bail, Result};
use base64::Engine as _;
use serde_json::{json, Value};

use super::mail::{extract_otp, EleClient, MailClient, OutlookClient};
use super::providers::breezeblue::KEY;
use super::proxy;
use super::runner::{self, RunState, RUN, SECRET_SETTING};
use super::store::{self, AccountQuery, Db, EmailQuery, MailMessage};

const DAY: i64 = 86400;
/// Python `TEST_TIMEOUT_S`: one Peek refresh or one mailbox test.
const TEST_TIMEOUT: Duration = Duration::from_secs(60);
/// Python `ADD_EMAILS_MAX`: new Ele mailboxes per Add.
const ADD_EMAILS_MAX: i64 = 100;

fn arg<'a>(args: &'a Value, k: &str) -> Result<&'a str> {
    args[k].as_str().ok_or_else(|| anyhow!("{k} required"))
}

fn id(args: &Value) -> Result<i64> {
    args["id"].as_i64().ok_or_else(|| anyhow!("id required"))
}

/// Our commands; None for a command that is not ours here.
pub async fn call(cmd: &str, args: &Value) -> Option<Result<Value>> {
    Some(match cmd {
        "settings" => runner::db().and_then(|db| settings(&db)),
        "settings_set" => runner::db().and_then(|db| settings_set(&db, args)),
        "test_ele" => test_ele().await,
        "import_basics" => (|| {
            let n = store::import_basics(&*runner::db()?, Path::new(arg(args, "path")?))?;
            Ok(json!({ "proxies": n.proxies, "blocked_domains": n.blocked_domains, "settings": n.settings }))
        })(),
        "breeze_run" => {
            let count = args["count"].as_u64().unwrap_or(0).min(u32::MAX.into()) as u32;
            let mode = args["proxy_mode"].as_str().unwrap_or("rotate").to_string();
            let route = args["route"].as_str().unwrap_or("email:ele");
            runner::start(count, mode, route).map(|()| Value::Null)
        }
        "breeze_stop" => {
            RUN.stop.store(true, SeqCst);
            Ok(Value::Null)
        }
        "run_state" => runner::db().and_then(|db| run_state(&db, &RUN)),
        "make_check" => runner::db().and_then(|db| make_check(&db, args)),
        "make_options" => runner::db().and_then(|db| make_options(&db)),
        "platforms" => runner::db().and_then(|db| Ok(json!([breeze_card(&db)?]))),
        "nav_counts" => runner::db().and_then(|db| {
            let (accounts, emails, proxies) = db.nav_counts()?;
            Ok(json!({ "accounts": accounts, "emails": emails, "platforms": 1, "proxies": proxies }))
        }),
        "accounts" => runner::db().and_then(|db| accounts(&db, args)),
        "account_events" => runner::db().and_then(|db| events(&db, args)),
        "account_secret" => runner::db().and_then(|db| {
            let key = arg(args, "key")?;
            if !["password", "api_key"].contains(&key) {
                bail!("unknown secret: {key}");
            }
            Ok(Value::from(db.account_secret(id(args)?, key)?))
        }),
        "account_screenshot" => runner::db()
            .and_then(|db| screenshot(&db, id(args)?, &runner::screens_dir()?)),
        "emails" => runner::db().and_then(|db| db.email_list(&email_query(args))),
        "email_ids" => runner::db().and_then(|db| Ok(json!(db.email_ids(&email_query(args))?))),
        "email" => email_detail_cmd(args).await,
        "email_mail" => runner::db().and_then(|db| {
            let (items, more) = db.email_mail_page(id(args)?, args["before_id"].as_i64())?;
            Ok(json!({ "items": mails_json(&items), "more": more }))
        }),
        "test_mailbox" => test_mailbox_cmd(args).await,
        "parse_outlook" => {
            let (valid, invalid) = store::parse_outlook(args["text"].as_str().unwrap_or(""));
            Ok(json!({ "valid": valid.len(), "invalid": invalid.len() }))
        }
        "add_outlook" => runner::db().and_then(|db| add_outlook(&db, args)),
        "add_ele" => add_ele_cmd(args).await,
        _ => return None,
    })
}

/// `EmailList` filters: `kind`, `status`, `free_for`, `q`, `order`,
/// `offset`, `limit` 1..100 (100).
fn email_query(args: &Value) -> EmailQuery {
    let s = |k: &str| {
        args[k]
            .as_str()
            .filter(|v| !v.is_empty() && *v != "all")
            .map(str::to_string)
    };
    EmailQuery {
        kind: s("kind"),
        status: s("status"),
        free_for: s("free_for"),
        q: args["q"].as_str().unwrap_or("").to_string(),
        order: s("order").unwrap_or_default(),
        offset: args["offset"].as_i64().unwrap_or(0).max(0),
        limit: args["limit"].as_i64().unwrap_or(100).clamp(1, 100),
    }
}

/// `MailMessage[]`: the code read from subject and body; the verify link
/// comes with ElevenLabs (M6).
fn mails_json(items: &[MailMessage]) -> Value {
    Value::from(
        items
            .iter()
            .map(|m| {
                let (subject, body) = (
                    m.subject.as_deref().unwrap_or(""),
                    m.body.as_deref().unwrap_or(""),
                );
                json!({
                    "id": m.id, "sender": m.sender.as_deref().unwrap_or(""), "subject": subject,
                    "body": body, "received_at": m.received_at.as_deref().unwrap_or(""),
                    "code": extract_otp(&format!("{subject}{body}")), "link": Value::Null,
                })
            })
            .collect::<Vec<_>>(),
    )
}

/// The saved Ele client; None while its address or key is not set.
fn ele_of(db: &std::sync::Arc<Db>) -> Option<EleClient> {
    ele_set(db)
        .unwrap_or(false)
        .then(|| runner::ele_client(db).ok())
        .flatten()
}

async fn email_detail_cmd(args: &Value) -> Result<Value> {
    let db = runner::db()?;
    let ele = ele_of(&db);
    email_detail(&db, ele.as_ref(), runner::outlook()?, id(args)?).await
}

/// `EmailDetail` (Python `get_email`): the inbox is read once first unless
/// the mailbox is gone or a run is on it (the run reads it itself), or it
/// is Ele while Ele is not set. A failed read shows the stored mail; its
/// text goes nowhere.
pub(crate) async fn email_detail(
    db: &Db,
    ele: Option<&EleClient>,
    outlook: &OutlookClient,
    id: i64,
) -> Result<Value> {
    let mb = db
        .mailbox_get(id)?
        .ok_or_else(|| anyhow!("Email not found"))?;
    let client = match (mb.provider.as_str(), ele) {
        ("outlook", _) => Some(MailClient::Outlook(outlook)),
        (_, Some(e)) => Some(MailClient::Ele(e)),
        _ => None,
    };
    if let Some(c) = client.filter(|_| mb.status != "gone") {
        if !db.mailbox_busy(id)? {
            let _ = tokio::time::timeout(TEST_TIMEOUT, c.poll_inbox(&mb)).await;
        }
    }
    let mut row = db
        .email_row(id)?
        .ok_or_else(|| anyhow!("Email not found"))?;
    let (items, more) = db.email_mail_page(id, None)?;
    let technical = match db.browser_of_mailbox(id)? {
        Some(b) => {
            let px = match b.proxy_id {
                Some(p) => db.proxy_get(p)?,
                None => None,
            };
            json!({ "browser_id": b.id, "profile": b.path,
                    "proxy": px.map(|p| format!("{}:{}", p.host, p.port)) })
        }
        None => json!({ "browser_id": null, "profile": null, "proxy": null }),
    };
    row["items"] = mails_json(&items);
    row["more"] = more.into();
    row["technical"] = technical;
    Ok(row)
}

async fn test_mailbox_cmd(args: &Value) -> Result<Value> {
    let db = runner::db()?;
    let ele = ele_of(&db);
    Ok(test_one(&db, ele.as_ref(), runner::outlook()?, id(args)?)
        .await
        .into())
}

/// One mailbox's Test (Python `check_emails` for one): `active`, `gone`
/// or `failed`. A mailbox a run is on answers its stored state unpolled;
/// otherwise one strict read: an Ele 404 is gone (the row is set already),
/// any other failure or a timeout is `failed`, never `gone`.
pub(crate) async fn test_one(
    db: &Db,
    ele: Option<&EleClient>,
    outlook: &OutlookClient,
    id: i64,
) -> &'static str {
    let state = |db: &Db| match db.mailbox_get(id) {
        Ok(Some(m)) if m.status == "gone" => "gone",
        Ok(Some(_)) => "active",
        _ => "failed",
    };
    let Ok(Some(mb)) = db.mailbox_get(id) else {
        return "failed";
    };
    if db.mailbox_busy(id).unwrap_or(true) {
        return state(db);
    }
    let client = match (mb.provider.as_str(), ele) {
        ("outlook", _) => MailClient::Outlook(outlook),
        (_, Some(e)) => MailClient::Ele(e),
        _ => return "failed",
    };
    match tokio::time::timeout(TEST_TIMEOUT, client.check(&mb)).await {
        Ok(Ok(())) => state(db),
        // Only Ele's 404 is final; a Graph 404 is an error, as in Python.
        Ok(Err(e)) if e.0 == "Ele API unavailable: inbox HTTP 404" => "gone",
        _ => "failed",
    }
}

/// `{added, duplicates, invalid: [line, reason][]}` of an Outlook paste.
fn add_outlook(db: &Db, args: &Value) -> Result<Value> {
    let r = db.import_outlook(arg(args, "text")?)?;
    Ok(json!({ "added": r.added, "duplicates": r.duplicates, "invalid": r.invalid }))
}

async fn add_ele_cmd(args: &Value) -> Result<Value> {
    let db = runner::db()?;
    let ele = ele_of(&db).ok_or_else(|| anyhow!("Set the Ele API key in Settings"))?;
    add_ele(&ele, args["count"].as_i64().unwrap_or(0)).await
}

/// `count` new Ele mailboxes with no account, one after another; a failure
/// part-way keeps what was made: `{created, error?}`.
pub(crate) async fn add_ele(ele: &EleClient, count: i64) -> Result<Value> {
    if !(1..=ADD_EMAILS_MAX).contains(&count) {
        bail!("count must be 1..{ADD_EMAILS_MAX}");
    }
    let mut created = 0;
    for _ in 0..count {
        if let Err(e) = ele.create_mailbox().await {
            return Ok(json!({ "created": created, "error": e.0 }));
        }
        created += 1;
    }
    Ok(json!({ "created": created }))
}

/// `PlatformCard` for Breeze Blue, the one platform made so far; `free` is
/// its free Outlook mailboxes (Ele makes a new one per account).
fn breeze_card(db: &Db) -> Result<Value> {
    Ok(json!({
        "key": KEY, "label": "Breeze Blue", "category": "voice", "ready": true, "routes": ["email"],
        "account_rule": "separate", "rule_fixed": false, "starred": true, "secrets": ["api_key"],
        "free": db.breeze_free_outlook_count()?, "options": [],
    }))
}

/// Free emails per route: Ele makes a new mailbox for each account (null).
fn make_options(db: &Db) -> Result<Value> {
    Ok(json!({ "email:ele": null, "email:outlook": db.breeze_free_outlook_count()? }))
}

/// `AccountList`: `tab` (platform), `status` (group), `flag`, `q`, `order`,
/// `offset`, `limit` 1..1000 (50).
fn accounts(db: &Db, args: &Value) -> Result<Value> {
    let s = |k: &str| {
        args[k]
            .as_str()
            .filter(|v| !v.is_empty() && *v != "all")
            .map(str::to_string)
    };
    db.account_list(&AccountQuery {
        platform: s("tab"),
        group: s("status"),
        flag: s("flag"),
        q: args["q"].as_str().unwrap_or("").to_string(),
        order: s("order").unwrap_or_default(),
        offset: args["offset"].as_i64().unwrap_or(0).max(0),
        limit: args["limit"].as_i64().unwrap_or(50).clamp(1, 1000),
    })
}

/// `EventRow[]`, oldest first.
fn events(db: &Db, args: &Value) -> Result<Value> {
    let rows = db.events_of(id(args)?)?;
    Ok(Value::from(
        rows.iter()
            .map(|e| {
                json!({ "id": e.id, "at": e.created_at, "level": e.level,
                        "step": e.step.as_deref().unwrap_or(""), "message": e.message })
            })
            .collect::<Vec<_>>(),
    ))
}

fn ele_set(db: &Db) -> Result<bool> {
    let set = |k| {
        db.settings_get(k)
            .map(|v| v.is_some_and(|v| !v.trim().is_empty()))
    };
    Ok(set("ele_api_url")? && set(SECRET_SETTING)?)
}

/// Shard is installing or swapping its engine: launches are refused now.
fn engine_busy() -> bool {
    crate::runtime::engine_use().is_none() || crate::runtime::swapping()
}

/// `RunState`: the one Make run while it is on; halted queues come with M5.
fn run_state(db: &Db, run: &RunState) -> Result<Value> {
    let jobs = match run.job() {
        Some((job, total)) => {
            let (linked, failed, step) = db.job_progress(job)?;
            let making = (i64::from(total) - linked - failed).max(0);
            vec![
                json!({ "job_id": job, "platform": KEY, "total": total, "linked": linked,
                         "making": making, "failed": failed, "step": step }),
            ]
        }
        None => vec![],
    };
    let (_, _, proxies) = db.nav_counts()?;
    Ok(json!({
        "jobs": jobs,
        "halted": [],
        "setup": { "ele": ele_set(db)?, "proxies": proxies, "captcha": false, "engine_busy": engine_busy() },
    }))
}

/// `MakeBlock[]`: why Make cannot start now, in the order they are said.
fn make_check(db: &Db, args: &Value) -> Result<Value> {
    let mut out = vec![];
    let ele_route = args["route"].as_str().is_none_or(|r| r == "email:ele");
    if ele_route && !ele_set(db)? {
        out.push("NO_ELE_KEY");
    }
    if engine_busy() {
        out.push("ENGINE_BUSY");
    }
    if args["proxy"].as_str() != Some("none") {
        let rest = proxy::rest_seconds(db)?;
        if !db.proxies_list()?.iter().any(|p| proxy::usable(p, rest)) {
            out.push("NO_PROXY");
        }
    }
    Ok(json!(out))
}

/// `CreatorSettings`, the fields M2 has: the Ele address, whether its key is
/// saved, and the proxy rest time in whole days.
fn settings(db: &Db) -> Result<Value> {
    let rest = proxy::rest_seconds(db)?;
    Ok(json!({
        "ele_api_url": db.settings_get("ele_api_url")?.unwrap_or_default(),
        "ele_key_saved": ele_key_saved(db)?,
        "proxy_rest_days": (rest + DAY / 2) / DAY,
    }))
}

fn ele_key_saved(db: &Db) -> Result<bool> {
    Ok(db
        .settings_get(SECRET_SETTING)?
        .is_some_and(|v| !v.is_empty()))
}

/// `{patch, keys}` from our tab's draft; answers `settings`. A blank key is
/// "unchanged", never "clear it". The rest time is written only when its
/// whole days changed, so a Python value in hours survives other saves.
fn settings_set(db: &Db, args: &Value) -> Result<Value> {
    let patch = &args["patch"];
    if let Some(url) = patch["ele_api_url"].as_str() {
        let url = url.trim();
        let web = url::Url::parse(url).is_ok_and(|u| {
            ["http", "https"].contains(&u.scheme()) && u.host_str().is_some_and(|h| !h.is_empty())
        });
        if !url.is_empty() && !web {
            bail!("Ele API address must start with http:// or https://");
        }
        db.settings_set("ele_api_url", url)?;
    }
    if let Some(days) = patch["proxy_rest_days"].as_i64() {
        let secs = days.saturating_mul(DAY).clamp(0, proxy::MAX_REST_SECONDS);
        if settings(db)?["proxy_rest_days"].as_i64() != Some(secs / DAY) {
            db.settings_set("proxy_rest_seconds", &secs.to_string())?;
        }
    }
    if let Some(key) = args["keys"]["ele_api_key"].as_str().map(str::trim) {
        if !key.is_empty() {
            db.settings_set(SECRET_SETTING, key)?;
        }
    }
    settings(db)
}

/// Test Ele: its active domains with the saved address and key.
async fn test_ele() -> Result<Value> {
    let db = runner::db()?;
    let domains = runner::ele_client(&db)?.active_domains().await?;
    Ok(json!({ "domains": domains.len() }))
}

/// The last page a failed sign-up saw, as a data URL; null with none, and
/// for a stored path that is not a file in `screens` (only ours are read).
fn screenshot(db: &Db, id: i64, screens: &Path) -> Result<Value> {
    let path = db.account_get(id)?.and_then(|a| a.screenshot);
    let inside = |p: &Path| {
        let (Ok(p), Ok(dir)) = (p.canonicalize(), screens.canonicalize()) else {
            return None;
        };
        p.starts_with(&dir).then_some(p)
    };
    let Some(png) = path
        .and_then(|p| inside(Path::new(&p)))
        .and_then(|p| std::fs::read(p).ok())
    else {
        return Ok(Value::Null);
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(png);
    Ok(Value::from(format!("data:image/png;base64,{b64}")))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::creator::store::{now, temp_db, AccountPatch};

    const TYPES: &str = include_str!("../../../src/creator/types.ts");

    /// The field names of `export type <name> = { ... };` in types.ts (on
    /// its own lines, two spaces in, or on one line), or of an inline
    /// `<name>: { ... }` member.
    fn fields(name: &str) -> BTreeSet<String> {
        let body = match TYPES.split_once(&format!("export type {name} = {{")) {
            Some((_, rest)) if rest.starts_with('\n') => {
                let body = rest.split("\n};").next().unwrap();
                let re = regex::Regex::new(r"(?m)^  (\w+)\??:").unwrap();
                return re.captures_iter(body).map(|c| c[1].to_string()).collect();
            }
            Some((_, rest)) => rest,
            None => TYPES.split_once(&format!("  {name}: {{")).expect(name).1,
        };
        let re = regex::Regex::new(r"(\w+)\??:").unwrap();
        re.captures_iter(&body[..body.find('}').unwrap()])
            .map(|c| c[1].to_string())
            .collect()
    }

    fn keys(v: &Value) -> BTreeSet<String> {
        v.as_object().unwrap().keys().cloned().collect()
    }

    #[test]
    fn accounts_rows_match_types() {
        let (db, _f) = temp_db();
        let mb = db.mailbox_create("ele", "u@a.test", "a.test", "t").unwrap();
        let job = db.job_create(KEY, 2, "none").unwrap();
        let id = db.account_create(mb.id, KEY, "email", Some(job)).unwrap();
        let p = AccountPatch {
            status: Some("running".into()),
            step_note: Some(Some("Reading the verification mail".into())),
            started_at: Some(Some(now())),
            ..Default::default()
        };
        db.account_update(id, &p).unwrap();
        db.event(None, Some(mb.id), "info", Some(KEY), "browser opened")
            .unwrap();

        let list = accounts(&db, &json!({})).unwrap();
        assert_eq!(keys(&list), fields("AccountList"));
        assert_eq!(keys(&list["items"][0]), fields("AccountRow"));
        assert_eq!(
            keys(&list["counts"]),
            BTreeSet::from(["all", "failed", "linked", "making", "signed_out"].map(String::from))
        );
        assert_eq!(list["items"][0]["step"], "Reading the verification mail");
        let ev = events(&db, &json!({ "id": id })).unwrap();
        assert_eq!(keys(&ev[0]), fields("EventRow"));

        let run = RunState::default();
        assert_eq!(run_state(&db, &run).unwrap()["jobs"], json!([]), "no run");
        let job = runner::begin_job(&db, &run, 2, "none").unwrap();
        let st = run_state(&db, &run).unwrap();
        assert_eq!(st["jobs"][0]["job_id"], job, "seen as soon as Make returns");
        assert_eq!(keys(&st), fields("RunState"));
        assert_eq!(keys(&st["jobs"][0]), fields("jobs"));
        assert_eq!(keys(&st["setup"]), fields("setup"));
        assert_eq!(
            (
                st["jobs"][0]["making"].as_i64(),
                st["setup"]["ele"].as_bool()
            ),
            (Some(2), Some(false))
        );
        assert_eq!(keys(&settings(&db).unwrap()).len(), 3);
    }

    #[test]
    fn accounts_filter_page_and_fail_info() {
        let (db, _f) = temp_db();
        for (i, (status, error)) in [
            ("linked", None),
            (
                "failed",
                Some("A Breeze Blue account already exists for this email"),
            ),
            ("failed", Some("captcha not solved")),
        ]
        .into_iter()
        .enumerate()
        {
            let mb = db
                .mailbox_create("ele", &format!("u{i}_x@a.test"), "a.test", "t")
                .unwrap();
            let id = db.account_create(mb.id, KEY, "email", None).unwrap();
            let p = AccountPatch {
                status: Some(status.into()),
                error_code: Some(
                    error
                        .and_then(crate::creator::errors::error_code)
                        .map(str::to_string),
                ),
                error: Some(error.map(str::to_string)),
                ..Default::default()
            };
            db.account_update(id, &p).unwrap();
        }
        let n = |a: Value| {
            accounts(&db, &a).unwrap()["items"]
                .as_array()
                .unwrap()
                .len()
        };
        assert_eq!(
            (
                n(json!({ "limit": 0 })),
                n(json!({ "limit": -5 })),
                n(json!({ "limit": 2 }))
            ),
            (1, 1, 2)
        );
        assert_eq!(n(json!({ "limit": 1_000_000, "offset": 1 })), 2);
        assert_eq!(
            n(json!({ "q": "U1_" })),
            1,
            "case-insensitive, `_` taken literally"
        );
        assert_eq!(n(json!({ "q": "%" })), 0);
        let failed = accounts(&db, &json!({ "status": "failed", "order": "oldest" })).unwrap();
        assert_eq!(
            (failed["total"].as_i64(), failed["counts"]["all"].as_i64()),
            (Some(2), Some(3))
        );
        assert_eq!(
            failed["items"][0]["fail"],
            json!({ "code": "ACCOUNT_EXISTS", "group": "account", "retryable": false })
        );
        assert_eq!(
            failed["items"][1]["fail"],
            json!({ "code": "CAPTCHA_UNSOLVED", "group": "captcha", "retryable": true })
        );
        assert_eq!(failed["tabs"], json!([{ "platform": KEY, "count": 3 }]));
    }

    #[test]
    fn make_options_counts_free_outlook() {
        let (db, _f) = temp_db();
        assert_eq!(
            make_options(&db).unwrap(),
            json!({ "email:ele": null, "email:outlook": 0 })
        );
        db.import_outlook("a@outlook.example|pw|T|C|r\nb@outlook.example|pw|T|C|r")
            .unwrap();
        assert_eq!(make_options(&db).unwrap()["email:outlook"], 2);
        assert_eq!(breeze_card(&db).unwrap()["free"], 2);
        assert_eq!(keys(&breeze_card(&db).unwrap()), fields("PlatformCard"));
        // No Ele key: blocks the Ele route only.
        let blocks =
            |route: &str| make_check(&db, &json!({ "route": route, "proxy": "none" })).unwrap();
        assert!(blocks("email:ele")
            .as_array()
            .unwrap()
            .contains(&json!("NO_ELE_KEY")));
        assert!(!blocks("email:outlook")
            .as_array()
            .unwrap()
            .contains(&json!("NO_ELE_KEY")));
    }

    #[test]
    fn settings_keep_the_key_secret_and_a_blank_key_unchanged() {
        let (db, _f) = temp_db();
        let set = |patch: Value, key: &str| {
            settings_set(
                &db,
                &json!({ "patch": patch, "keys": { "ele_api_key": key } }),
            )
        };
        set(json!({}), " KEY-1 ").unwrap();
        set(json!({}), "   ").unwrap();
        let got = set(json!({ "ele_api_url": " https://ele.test/api " }), "").unwrap();
        assert_eq!(
            db.settings_get(SECRET_SETTING).unwrap().as_deref(),
            Some("KEY-1")
        );
        assert_eq!(got["ele_api_url"], "https://ele.test/api");
        assert_eq!(got["ele_key_saved"], true);
        assert!(
            !got.to_string().contains("KEY-1"),
            "the key is never read back"
        );
        for bad in ["ele.test", "https://", "ftp://ele.test"] {
            assert!(set(json!({ "ele_api_url": bad }), "").is_err(), "{bad}");
        }
        assert!(set(json!({ "ele_api_url": "HTTPS://Ele.test/api" }), "").is_ok());
        assert!(
            set(json!({ "proxy_rest_days": i64::MAX }), "").is_ok(),
            "no overflow"
        );
        // Hours from the Python app survive a save that leaves the days alone.
        db.settings_set("proxy_rest_seconds", "3600").unwrap();
        set(json!({ "proxy_rest_days": 0 }), "").unwrap();
        assert_eq!(
            db.settings_get("proxy_rest_seconds").unwrap().as_deref(),
            Some("3600")
        );
        assert_eq!(
            set(json!({ "proxy_rest_days": 7 }), "").unwrap()["proxy_rest_days"],
            7
        );
        assert_eq!(
            db.settings_get("proxy_rest_seconds").unwrap().as_deref(),
            Some("604800")
        );
    }

    #[test]
    fn a_screenshot_is_read_only_from_the_screens_folder() {
        let (db, _f) = temp_db();
        let base = std::env::temp_dir().join(format!("creator-shots-{}", uuid::Uuid::new_v4()));
        let screens = base.join("screens");
        std::fs::create_dir_all(&screens).unwrap();
        let (inside, outside) = (screens.join("1.png"), base.join("secret.png"));
        std::fs::write(&inside, b"png").unwrap();
        std::fs::write(&outside, b"png").unwrap();
        let mb = db.mailbox_create("ele", "u@a.test", "a.test", "t").unwrap();
        let id = db.account_create(mb.id, KEY, "email", None).unwrap();
        let shot = |p: &Path| {
            let patch = AccountPatch {
                screenshot: Some(Some(p.display().to_string())),
                ..Default::default()
            };
            db.account_update(id, &patch).unwrap();
            screenshot(&db, id, &screens).unwrap()
        };
        assert!(shot(&inside)
            .as_str()
            .unwrap()
            .starts_with("data:image/png;base64,"));
        assert_eq!(shot(&outside), Value::Null);
        assert_eq!(shot(&screens.join("..").join("secret.png")), Value::Null);
        assert_eq!(shot(&screens.join("gone.png")), Value::Null);
        let _ = std::fs::remove_dir_all(&base);
    }

    // ---- Emails (M3)

    use crate::creator::mail::OutlookUrls;
    use crate::creator::web::testkit::FixtureServer;
    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::routing::{get, post};
    use axum::{Form, Json, Router};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    const RT: &str = "RT-OK-secret";
    const CID: &str = "CID-secret-9";

    /// A fake Outlook (by refresh token: `RT-OK-secret` answers, `RT-DEAD`
    /// is a dead grant, `RT-DOWN` a 503) and a fake Ele (`gone-tok`: 404),
    /// counting token calls.
    async fn fakes() -> (FixtureServer, Arc<AtomicU32>) {
        let calls = Arc::new(AtomicU32::new(0));
        let app = Router::new()
            .route(
                "/ms/oauth20_token.srf",
                post(
                    |State(n): State<Arc<AtomicU32>>, Form(f): Form<HashMap<String, String>>| async move {
                        n.fetch_add(1, Ordering::SeqCst);
                        match f["refresh_token"].as_str() {
                            "RT-DEAD" => (StatusCode::BAD_REQUEST, Json(json!({ "error": "invalid_grant" }))),
                            "RT-DOWN" => (StatusCode::SERVICE_UNAVAILABLE, Json(json!({}))),
                            rt => (StatusCode::OK, Json(json!({ "access_token": "AT", "refresh_token": rt }))),
                        }
                    },
                ),
            )
            .route(
                "/ms/v1.0/me/messages",
                get(|| async {
                    Json(json!({ "value": [{ "id": "M1", "subject": "Your verification code is: 482913",
                        "receivedDateTime": "2026-11-17T10:00:00Z",
                        "from": { "emailAddress": { "address": "a@sender.example" } },
                        "body": { "contentType": "text", "content": "hello" } }] }))
                }),
            )
            .route(
                "/ele/inbox/:token/emails",
                get(|axum::extract::Path(t): axum::extract::Path<String>| async move {
                    let code = if t == "gone-tok" { StatusCode::NOT_FOUND } else { StatusCode::OK };
                    (code, Json(json!({ "emails": [] })))
                }),
            )
            .with_state(calls.clone());
        (FixtureServer::start_with(app).await, calls)
    }

    fn outlook_row(db: &Db, email: &str, rt: &str) -> i64 {
        db.import_outlook(&format!("{email}|pw|{rt}|{CID}|r"))
            .unwrap();
        db.email_ids(&EmailQuery::default())
            .unwrap()
            .into_iter()
            .max()
            .unwrap()
    }

    fn set_account(db: &Db, mb: i64, platform: &str, status: &str) -> i64 {
        let id = db.account_create(mb, platform, "email", None).unwrap();
        let p = AccountPatch {
            status: Some(status.into()),
            ..Default::default()
        };
        db.account_update(id, &p).unwrap();
        id
    }

    #[tokio::test]
    async fn email_rows_match_types() {
        let (db, _f) = temp_db();
        let db = Arc::new(db);
        let (srv, _) = fakes().await;
        let ol = OutlookClient::new(db.clone(), OutlookUrls::on(&srv.site)).unwrap();
        let mb = outlook_row(&db, "a@outlook.example", RT);
        set_account(&db, mb, KEY, "linked");
        db.mail_create(
            mb,
            &store::NewMail {
                subject: "s".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let list = db.email_list(&email_query(&json!({}))).unwrap();
        assert_eq!(keys(&list), fields("EmailList"));
        assert_eq!(keys(&list["items"][0]), fields("EmailRow"));
        assert_eq!(
            keys(&list["items"][0]["accounts"][0]),
            BTreeSet::from(["platform", "status", "verify_status"].map(String::from))
        );
        assert_eq!(
            keys(&list["counts"]),
            BTreeSet::from(["all", "outlook", "ele"].map(String::from))
        );
        let d = email_detail(&db, None, &ol, mb).await.unwrap();
        let mut want = fields("EmailRow");
        want.extend(["items", "more", "technical"].map(String::from));
        assert_eq!(keys(&d), want);
        assert_eq!(keys(&d["technical"]), fields("technical"));
        assert_eq!(keys(&d["items"][0]), fields("MailMessage"));
        let last = d["items"].as_array().unwrap().last().unwrap()["id"].as_i64();
        let (items, more) = db.email_mail_page(mb, last).unwrap();
        assert!(items.is_empty() && !more);
    }

    #[test]
    fn emails_filter_count_and_window() {
        let (db, _f) = temp_db();
        for i in 0..5 {
            outlook_row(&db, &format!("o{i}_x@outlook.example"), RT);
        }
        for i in 0..3 {
            db.mailbox_create("ele", &format!("e{i}@ele.example"), "ele.example", "t")
                .unwrap();
        }
        db.mailbox_set_status(1, "gone").unwrap();
        let q = |a: Value| db.email_list(&email_query(&a)).unwrap();
        let all = q(json!({}));
        assert_eq!(all["counts"], json!({ "all": 8, "outlook": 5, "ele": 3 }));
        assert_eq!(
            (all["total"].as_i64(), all["items"][0]["id"].as_i64()),
            (Some(8), Some(8))
        );
        let ol = q(json!({ "kind": "outlook", "order": "oldest", "limit": 2, "offset": 1 }));
        assert_eq!(ol["total"], 5);
        assert_eq!(ol["counts"]["all"], 8, "type counts ignore the type filter");
        let ids: Vec<i64> = ol["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["id"].as_i64().unwrap())
            .collect();
        assert_eq!(ids, [2, 3]);
        let gone = q(json!({ "status": "gone" }));
        assert_eq!(
            (gone["total"].as_i64(), gone["counts"]["ele"].as_i64()),
            (Some(1), Some(0))
        );
        assert_eq!(
            q(json!({ "q": "O1_" }))["total"],
            1,
            "case-insensitive, `_` literal"
        );
        assert_eq!(q(json!({ "q": "%" }))["total"], 0);
        assert_eq!(
            q(json!({ "order": "email" }))["items"][0]["email"],
            "e0@ele.example"
        );
        assert_eq!(
            q(json!({ "limit": 0 }))["items"].as_array().unwrap().len(),
            1
        );
        assert_eq!(
            q(json!({ "limit": 1000 }))["items"]
                .as_array()
                .unwrap()
                .len(),
            8
        );
        assert_eq!(
            db.email_ids(&email_query(&json!({ "kind": "ele" })))
                .unwrap(),
            [6, 7, 8]
        );
    }

    #[test]
    fn free_for_breeze_lists_only_free_outlook() {
        let (db, _f) = temp_db();
        let free = outlook_row(&db, "a@outlook.example", RT);
        let taken = outlook_row(&db, "b@outlook.example", RT);
        set_account(&db, taken, KEY, "linked");
        db.mailbox_create("ele", "e@ele.example", "ele.example", "t")
            .unwrap();
        let ff = |k: &str| {
            db.email_list(&email_query(&json!({ "free_for": k })))
                .unwrap()
        };
        let b = ff(KEY);
        assert_eq!(
            (b["total"].as_i64(), b["items"][0]["id"].as_i64()),
            (Some(1), Some(free))
        );
        assert_eq!(
            ff("google")["total"],
            0,
            "no other platform has free emails yet"
        );
        assert_eq!(
            db.email_ids(&email_query(&json!({ "free_for": KEY })))
                .unwrap(),
            [free]
        );
    }

    #[tokio::test]
    async fn email_peek_skips_poll_when_gone_or_running() {
        let (db, _f) = temp_db();
        let db = Arc::new(db);
        let (srv, calls) = fakes().await;
        let ol = OutlookClient::new(db.clone(), OutlookUrls::on(&srv.site)).unwrap();
        let idle = outlook_row(&db, "a@outlook.example", RT);
        let d = email_detail(&db, None, &ol, idle).await.unwrap();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            (d["mails"].as_i64(), d["items"][0]["code"].as_str()),
            (Some(1), Some("482913"))
        );
        let gone = outlook_row(&db, "b@outlook.example", RT);
        db.mailbox_set_status(gone, "gone").unwrap();
        let running = outlook_row(&db, "c@outlook.example", RT);
        set_account(&db, running, KEY, "running");
        for id in [gone, running] {
            let d = email_detail(&db, None, &ol, id).await.unwrap();
            assert_eq!(d["items"], json!([]));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1, "not polled");
        // A failed read still shows the stored mail.
        let down = outlook_row(&db, "d@outlook.example", "RT-DOWN");
        db.mail_create(
            down,
            &store::NewMail {
                subject: "kept".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let d = email_detail(&db, None, &ol, down).await.unwrap();
        assert_eq!(d["items"][0]["subject"], "kept");
        assert!(email_detail(&db, None, &ol, 999).await.is_err());
    }

    #[test]
    fn email_mail_pages_by_cursor_without_shift() {
        let (db, _f) = temp_db();
        let mb = db
            .mailbox_create("ele", "a@ele.example", "ele.example", "t")
            .unwrap();
        let add = |i: u32| {
            db.mail_create(
                mb.id,
                &store::NewMail {
                    subject: format!("m{i}"),
                    received_at: Some(format!("2026-11-01T{:02}:{:02}:00", i / 60, i % 60)),
                    ..Default::default()
                },
            )
            .unwrap()
        };
        for i in 0..120 {
            add(i);
        }
        let (p1, more) = db.email_mail_page(mb.id, None).unwrap();
        assert_eq!((p1.len(), more), (50, true));
        assert_eq!(p1[0].subject.as_deref(), Some("m119"));
        add(500); // new mail lands on top between the calls
        let (p2, more) = db.email_mail_page(mb.id, Some(p1[49].id)).unwrap();
        assert_eq!((p2.len(), more), (50, true));
        assert_eq!(p2[0].subject.as_deref(), Some("m69"));
        let (p3, more) = db.email_mail_page(mb.id, Some(p2[49].id)).unwrap();
        assert_eq!((p3.len(), more), (20, false));
        let mut seen: Vec<i64> = p1.iter().chain(&p2).chain(&p3).map(|m| m.id).collect();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), 120, "no row twice, none skipped");
    }

    #[tokio::test]
    async fn test_mailbox_states() {
        let (db, _f) = temp_db();
        let db = Arc::new(db);
        let (srv, calls) = fakes().await;
        let ol = OutlookClient::new(db.clone(), OutlookUrls::on(&srv.site)).unwrap();
        let ele = EleClient::new(&format!("{}/ele/", srv.site), "KEY", db.clone()).unwrap();
        let active = outlook_row(&db, "a@outlook.example", RT);
        let dead = outlook_row(&db, "b@outlook.example", "RT-DEAD");
        let down_gone = outlook_row(&db, "c@outlook.example", "RT-DOWN");
        db.mailbox_set_status(down_gone, "gone").unwrap();
        let busy = outlook_row(&db, "d@outlook.example", RT);
        set_account(&db, busy, KEY, "running");
        let ele_ok = db
            .mailbox_create("ele", "e@ele.example", "ele.example", "tok")
            .unwrap()
            .id;
        let ele_404 = db
            .mailbox_create("ele", "f@ele.example", "ele.example", "gone-tok")
            .unwrap()
            .id;
        let t = |id, ele: Option<&EleClient>| test_one(&db, ele, &ol, id);
        assert_eq!(t(active, None).await, "active");
        assert_eq!(t(dead, None).await, "gone");
        assert_eq!(db.mailbox_get(dead).unwrap().unwrap().status, "gone");
        assert_eq!(
            t(down_gone, None).await,
            "failed",
            "a failure is never gone"
        );
        assert_eq!(calls.load(Ordering::SeqCst), 3);
        assert_eq!(t(busy, None).await, "active");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            3,
            "a running mailbox is not read"
        );
        assert_eq!(t(ele_ok, Some(&ele)).await, "active");
        assert_eq!(t(ele_404, Some(&ele)).await, "gone");
        assert_eq!(t(ele_ok, None).await, "failed", "Ele not set");
        assert_eq!(t(999, None).await, "failed");
    }

    #[tokio::test]
    async fn no_command_answer_holds_a_token_or_client_id() {
        let (db, _f) = temp_db();
        let db = Arc::new(db);
        let (srv, _) = fakes().await;
        let ol = OutlookClient::new(db.clone(), OutlookUrls::on(&srv.site)).unwrap();
        let text = format!(
            "a@outlook.example|pw|{RT}|{CID}|r\na@outlook.example|pw|{RT}|{CID}|r\nbad {RT} {CID}"
        );
        let mb = outlook_row(&db, "z@outlook.example", RT);
        set_account(&db, mb, KEY, "failed");
        let answers = [
            add_outlook(&db, &json!({ "text": text })).unwrap(),
            db.email_list(&email_query(&json!({}))).unwrap(),
            db.email_list(&email_query(&json!({ "free_for": KEY })))
                .unwrap(),
            json!(db.email_ids(&email_query(&json!({}))).unwrap()),
            email_detail(&db, None, &ol, mb).await.unwrap(),
            json!(test_one(&db, None, &ol, mb).await),
            json!({ "items": mails_json(&db.email_mail_page(mb, None).unwrap().0) }),
            make_options(&db).unwrap(),
            breeze_card(&db).unwrap(),
        ];
        for a in answers {
            let s = a.to_string();
            for secret in [RT, CID, "AT\""] {
                assert!(!s.contains(secret), "{secret} in {s}");
            }
        }
    }

    #[test]
    fn add_outlook_reports_duplicates_and_invalid_lines() {
        let (db, _f) = temp_db();
        outlook_row(&db, "a@outlook.example", RT);
        let text = "a@outlook.example|pw|T|C|r\n\nnot a line\nb@outlook.example|pw|T|C|r\nb@outlook.example|pw|T|C|r";
        assert_eq!(
            add_outlook(&db, &json!({ "text": text })).unwrap(),
            json!({ "added": 1, "duplicates": [1, 5],
                    "invalid": [[3, "not email|password|refresh_token|client_id|recovery"]] })
        );
        assert!(add_outlook(&db, &json!({})).is_err());
    }

    #[tokio::test]
    async fn add_ele_counts_and_keeps_what_was_made() {
        let (db, _f) = temp_db();
        let dead = EleClient::new("http://127.0.0.1:9/", "KEY", Arc::new(db)).unwrap();
        for bad in [0, 101] {
            assert!(add_ele(&dead, bad).await.is_err(), "{bad}");
        }
        let r = add_ele(&dead, 3).await.unwrap();
        assert_eq!(r["created"], 0);
        assert!(r["error"]
            .as_str()
            .unwrap()
            .starts_with("Ele API unavailable"));
    }

    /// 500,000 mailboxes and 1,000 accounts: the first window with its
    /// counts, unfiltered and with each filter. By hand on the box:
    /// `cargo test --locked --lib creator::pages::tests::emails_list_at_500k -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn emails_list_at_500k() {
        let (db, _f) = temp_db();
        db.exec_for_test(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i < 500000) \
             INSERT INTO mailboxes (provider, email, domain, token, status, client_id, created_at) \
             SELECT CASE WHEN i % 3 = 0 THEN 'ele' ELSE 'outlook' END, 'u' || i || '@outlook.example', \
             'outlook.example', 't' || i, CASE WHEN i % 5 = 0 THEN 'gone' ELSE 'active' END, 'c', \
             '2026-11-01T00:00:00+00:00' FROM n; \
             WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i < 1000) \
             INSERT INTO accounts (mailbox_id, platform, route, status, created_at, updated_at) \
             SELECT i * 7, 'breezeblue', 'email', CASE WHEN i % 2 = 0 THEN 'linked' ELSE 'failed' END, \
             '2026-11-01', '2026-11-01' FROM n; ANALYZE;",
        );
        let mut worst = Duration::ZERO;
        for (name, a) in [
            ("all", json!({})),
            ("outlook", json!({ "kind": "outlook" })),
            ("gone", json!({ "status": "gone" })),
            ("free breezeblue", json!({ "free_for": KEY })),
            ("search", json!({ "q": "u4999" })),
            ("oldest", json!({ "order": "oldest" })),
            ("email order", json!({ "order": "email" })),
        ] {
            let t = std::time::Instant::now();
            let r = db.email_list(&email_query(&a)).unwrap();
            let took = t.elapsed();
            eprintln!("{name}: {took:?} total={}", r["total"]);
            worst = worst.max(took);
        }
        let t = std::time::Instant::now();
        let free = db.breeze_free_outlook_count().unwrap();
        eprintln!("free count: {:?} ({free})", t.elapsed());
        assert!(worst < Duration::from_millis(300), "{worst:?}");
    }
}
