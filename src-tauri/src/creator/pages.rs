//! Our pages' commands (`creator_call`), answering in the shapes of
//! `src/creator/types.ts`, as `npm run dev:mock` does from `mock.ts`. Only
//! what Breeze Blue needs (M2); the other pages stay on mock data until M7.
//! Never a secret in a list or a Peek: a password or key is read only when
//! Copy is pressed (`account_secret`), the Ele key is only "saved" or not.

use std::path::Path;
use std::sync::atomic::Ordering::SeqCst;

use anyhow::{anyhow, bail, Result};
use base64::Engine as _;
use serde_json::{json, Value};

use super::providers::breezeblue::KEY;
use super::proxy;
use super::runner::{self, RunState, RUN, SECRET_SETTING};
use super::store::{self, AccountQuery, Db};

const DAY: i64 = 86400;

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
            runner::start(count, mode).map(|()| Value::Null)
        }
        "breeze_stop" => {
            RUN.stop.store(true, SeqCst);
            Ok(Value::Null)
        }
        "run_state" => runner::db().and_then(|db| run_state(&db, &RUN)),
        "make_check" => runner::db().and_then(|db| make_check(&db, args)),
        // Ele makes a new mailbox for each account: no count of free ones (null).
        "make_options" => Ok(json!({ "email:ele": null })),
        "platforms" => Ok(json!([breeze_card()])),
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
        _ => return None,
    })
}

/// `PlatformCard` for Breeze Blue, the one platform M2 makes.
fn breeze_card() -> Value {
    json!({
        "key": KEY, "label": "Breeze Blue", "category": "voice", "ready": true, "routes": ["email"],
        "account_rule": "separate", "rule_fixed": false, "starred": true, "secrets": ["api_key"],
        "free": 0, "options": [],
    })
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
    if !ele_set(db)? {
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
        let web = url::Url::parse(url)
            .is_ok_and(|u| ["http", "https"].contains(&u.scheme()) && u.host_str().is_some_and(|h| !h.is_empty()));
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
    let Some(png) = path.and_then(|p| inside(Path::new(&p))).and_then(|p| std::fs::read(p).ok()) else {
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
        assert!(set(json!({ "proxy_rest_days": i64::MAX }), "").is_ok(), "no overflow");
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
        assert!(shot(&inside).as_str().unwrap().starts_with("data:image/png;base64,"));
        assert_eq!(shot(&outside), Value::Null);
        assert_eq!(shot(&screens.join("..").join("secret.png")), Value::Null);
        assert_eq!(shot(&screens.join("gone.png")), Value::Null);
        let _ = std::fs::remove_dir_all(&base);
    }
}
