//! Proxy pick for a new account (Python `proxy/assign.py`) and the proxy of an
//! existing browser (Python `engine/account.py` `ensure_proxy`), on OUR
//! `proxies` table, never Shard's. No mover: a browser whose proxy is off,
//! failing or resting fails with `PROXY_UNUSABLE` (retryable); only a new
//! account gets a pick.

use std::fmt;

use anyhow::Result;

use super::store::{now, parse_iso, Db, Proxy};

/// The most `proxy_rest_seconds` may be: 90 days.
pub const MAX_REST_SECONDS: i64 = 90 * 86400;
/// Python `DEFAULTS["proxy_rest_seconds"]`: a week.
const DEFAULT_REST_SECONDS: i64 = 604_800;

/// Python `account.PROXY_UNUSABLE`: not a sign-up failure.
pub const PROXY_UNUSABLE: &str =
    "Browser proxy unusable (off, failing or resting): change its proxy";
/// The browser's proxy was deleted and no other is usable.
pub const PROXY_DELETED: &str = "proxy deleted";

/// No proxy a new account may use (Python `NoHealthyProxy`; the runner's
/// error text is "No healthy proxy").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NoHealthyProxy(pub String);

impl fmt::Display for NoHealthyProxy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for NoHealthyProxy {}

/// Setting `proxy_rest_seconds`, 0..90 days; unreadable -> the default.
pub fn rest_seconds(db: &Db) -> Result<i64> {
    let n = db
        .settings_get("proxy_rest_seconds")?
        .and_then(|v| v.trim().parse::<f64>().ok())
        .map_or(DEFAULT_REST_SECONDS, |v| v as i64);
    Ok(n.clamp(0, MAX_REST_SECONDS))
}

fn now_micros() -> i64 {
    parse_iso(&now()).unwrap_or(0)
}

/// Until when (micros) a proxy Google refused rests; None when it is not resting.
pub fn rest_until(p: &Proxy, rest_secs: i64) -> Option<i64> {
    if rest_secs <= 0 {
        return None;
    }
    let until = parse_iso(p.refused_at.as_deref()?)? + rest_secs * 1_000_000;
    (until > now_micros()).then_some(until)
}

/// A proxy new accounts may use: active and not failing (unknown and slow
/// count), and not resting.
pub fn usable(p: &Proxy, rest_secs: i64) -> bool {
    p.active && p.status != "failing" && rest_until(p, rest_secs).is_none()
}

/// The proxy for a new account. `mode`: `none` (no proxy), `rotate` (oldest
/// `last_used_at`, never used first, then id) or `least` (fewest browsers,
/// then id); `kind`: "" any, or `static` / `rotating`. The picked proxy's
/// `last_used_at` becomes now. No usable one: a `NoHealthyProxy` error.
pub fn pick_proxy(db: &Db, mode: &str, kind: &str) -> Result<Option<Proxy>> {
    if mode == "none" {
        return Ok(None);
    }
    let rest = rest_seconds(db)?;
    let cands: Vec<Proxy> = db
        .proxies_list()?
        .into_iter()
        .filter(|p| usable(p, rest) && (kind.is_empty() || p.kind == kind))
        .collect();
    let best = if mode == "least" {
        let counts = db.browsers_per_proxy()?;
        cands
            .into_iter()
            .min_by_key(|p| (counts.get(&p.id).copied().unwrap_or(0), p.id))
    } else {
        cands.into_iter().min_by(|a, b| {
            let key = |p: &Proxy| (p.last_used_at.is_some(), p.last_used_at.clone(), p.id);
            key(a).cmp(&key(b))
        })
    };
    let Some(best) = best else {
        let kind = if kind.is_empty() {
            String::new()
        } else {
            format!("{kind} ")
        };
        return Err(NoHealthyProxy(format!("no usable {kind}proxy available")).into());
    };
    // ponytail: pick and touch are two statements; make them one transaction
    // when runs go in parallel (M5), or two runs can take the same proxy.
    db.proxy_touch(best.id)?;
    Ok(db.proxy_get(best.id)?)
}

/// The proxy an existing browser runs on (Python `ensure_proxy`). Only a
/// MISSING proxy is replaced (no automatic move): one that is off, failing or
/// resting fails with `PROXY_UNUSABLE`; a deleted one is picked again
/// (`PROXY_DELETED` when none is usable); mode `none` runs direct.
pub fn browser_proxy(db: &Db, browser_id: i64, kind: &str) -> Result<Option<Proxy>> {
    let br = db
        .browser_get(browser_id)?
        .ok_or_else(|| anyhow::anyhow!("browser {browser_id} not found"))?;
    let proxy = match br.proxy_id {
        Some(id) => db.proxy_get(id)?,
        None => None,
    };
    if let Some(p) = proxy {
        if usable(&p, rest_seconds(db)?) {
            return Ok(Some(p));
        }
        if br.proxy_mode != "none" {
            anyhow::bail!(PROXY_UNUSABLE);
        }
    } else if br.proxy_mode == "none" && br.proxy_id.is_none() {
        return Ok(None);
    }
    // Missing (deleted), or mode none dropping a stale unusable proxy.
    let old = br.proxy_id;
    let picked = match pick_proxy(db, &br.proxy_mode, kind) {
        Ok(p) => p,
        Err(e) if old.is_none() && e.is::<NoHealthyProxy>() => anyhow::bail!(PROXY_DELETED),
        Err(e) => return Err(e),
    };
    db.browser_set_proxy(br.id, picked.as_ref().map(|p| p.id))?;
    let note = match (&picked, old) {
        (_, Some(old)) if br.proxy_mode == "none" => {
            format!("proxy mode none: dropped stale proxy {old}")
        }
        (Some(p), _) => format!("browser had no proxy: using {}", p.id),
        (None, _) => "browser had no proxy: using none".to_string(),
    };
    db.event(
        Some(br.id),
        Some(br.mailbox_id),
        "warn",
        Some("proxy"),
        &note,
    )?;
    Ok(picked)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::creator::store::temp_db;

    fn set(db: &Db, id: i64, sql: &str) {
        db.exec_for_test(&format!("UPDATE proxies SET {sql} WHERE id={id}"));
    }

    #[test]
    fn none_mode_returns_none() {
        let (db, _f) = temp_db();
        assert_eq!(pick_proxy(&db, "none", "").unwrap(), None);
    }

    #[test]
    fn rotate_picks_oldest_null_first() {
        let (db, _f) = temp_db();
        for h in ["a", "b", "c"] {
            db.proxy_add(h, 1, None, None, "static").unwrap();
        }
        set(&db, 1, "last_used_at='2026-01-02T00:00:00.000000+00:00'");
        set(&db, 2, "last_used_at='2026-01-01T00:00:00.000000+00:00'");
        assert_eq!(pick_proxy(&db, "rotate", "").unwrap().unwrap().host, "c");
        assert_eq!(pick_proxy(&db, "rotate", "").unwrap().unwrap().host, "b");
        assert_eq!(pick_proxy(&db, "rotate", "").unwrap().unwrap().host, "a");
        // The pick was stamped: c is the oldest again.
        let c = pick_proxy(&db, "rotate", "").unwrap().unwrap();
        assert_eq!(c.host, "c");
        assert!(c.last_used_at.is_some());
    }

    #[test]
    fn least_picks_fewest_browsers() {
        let (db, _f) = temp_db();
        for h in ["a", "b"] {
            db.proxy_add(h, 1, None, None, "static").unwrap();
        }
        for i in 0..2 {
            let mb = db
                .mailbox_create("ele", &format!("{i}@x"), "x", "t")
                .unwrap();
            db.browser_create(mb.id, Some(1), "least").unwrap();
        }
        let mb = db.mailbox_create("ele", "z@x", "x", "t").unwrap();
        db.browser_create(mb.id, Some(2), "least").unwrap();
        assert_eq!(pick_proxy(&db, "least", "").unwrap().unwrap().host, "b");
    }

    #[test]
    fn failing_inactive_and_resting_are_skipped() {
        let (db, _f) = temp_db();
        for h in ["failing", "off", "resting", "slow", "rotating"] {
            db.proxy_add(
                h,
                1,
                None,
                None,
                if h == "rotating" {
                    "rotating"
                } else {
                    "static"
                },
            )
            .unwrap();
        }
        set(&db, 1, "status='failing'");
        set(&db, 2, "active=0");
        set(&db, 3, &format!("refused_at='{}'", now()));
        set(&db, 4, "status='slow'");
        let mut got = Vec::new();
        for _ in 0..4 {
            got.push(pick_proxy(&db, "rotate", "").unwrap().unwrap().host);
        }
        assert_eq!(got, ["slow", "rotating", "slow", "rotating"]);
        assert_eq!(
            pick_proxy(&db, "rotate", "static").unwrap().unwrap().host,
            "slow"
        );
        // Rest 0 = never rests; a refusal older than the rest is over.
        db.settings_set("proxy_rest_seconds", "0").unwrap();
        assert!(usable(
            &db.proxy_get(3).unwrap().unwrap(),
            rest_seconds(&db).unwrap()
        ));
        db.settings_set("proxy_rest_seconds", "60").unwrap();
        set(&db, 3, "refused_at='2026-01-01T00:00:00.000000+00:00'");
        assert!(usable(
            &db.proxy_get(3).unwrap().unwrap(),
            rest_seconds(&db).unwrap()
        ));
    }

    #[test]
    fn no_usable_is_an_error_naming_the_kind() {
        let (db, _f) = temp_db();
        db.proxy_add("a", 1, None, None, "static").unwrap();
        let err = pick_proxy(&db, "rotate", "rotating").unwrap_err();
        assert_eq!(
            err.downcast_ref::<NoHealthyProxy>().unwrap().0,
            "no usable rotating proxy available"
        );
        set(&db, 1, "status='failing'");
        assert_eq!(
            pick_proxy(&db, "least", "").unwrap_err().to_string(),
            "no usable proxy available"
        );
    }

    #[test]
    fn rest_seconds_is_clamped_with_a_week_by_default() {
        let (db, _f) = temp_db();
        assert_eq!(rest_seconds(&db).unwrap(), 604_800);
        db.settings_set("proxy_rest_seconds", "99999999999")
            .unwrap();
        assert_eq!(rest_seconds(&db).unwrap(), MAX_REST_SECONDS);
        db.settings_set("proxy_rest_seconds", "-5").unwrap();
        assert_eq!(rest_seconds(&db).unwrap(), 0);
        db.settings_set("proxy_rest_seconds", "soon").unwrap();
        assert_eq!(rest_seconds(&db).unwrap(), 604_800);
    }

    #[test]
    fn an_existing_browser_on_a_failing_proxy_fails_with_its_proxy_error() {
        let (db, _f) = temp_db();
        db.proxy_add("bad", 1, None, None, "static").unwrap();
        db.proxy_add("good", 1, None, None, "static").unwrap();
        let mb = db.mailbox_create("ele", "u@x", "x", "t").unwrap();
        let br = db.browser_create(mb.id, Some(1), "rotate").unwrap();
        assert_eq!(browser_proxy(&db, br, "").unwrap().unwrap().host, "bad");
        set(&db, 1, "status='failing'");
        assert_eq!(
            browser_proxy(&db, br, "").unwrap_err().to_string(),
            PROXY_UNUSABLE
        );
        // Nothing moved it.
        assert_eq!(db.browser_get(br).unwrap().unwrap().proxy_id, Some(1));
    }

    #[test]
    fn a_browser_whose_proxy_was_deleted_gets_a_new_one_or_fails() {
        let (db, _f) = temp_db();
        db.proxy_add("good", 1, None, None, "static").unwrap();
        let mb = db.mailbox_create("ele", "u@x", "x", "t").unwrap();
        let br = db.browser_create(mb.id, None, "rotate").unwrap();
        assert_eq!(browser_proxy(&db, br, "").unwrap().unwrap().host, "good");
        assert_eq!(db.browser_get(br).unwrap().unwrap().proxy_id, Some(1));
        set(&db, 1, "active=0");
        db.browser_set_proxy(br, None).unwrap();
        assert_eq!(
            browser_proxy(&db, br, "").unwrap_err().to_string(),
            PROXY_DELETED
        );
        // Mode none runs direct.
        let mb = db.mailbox_create("ele", "v@x", "x", "t").unwrap();
        let direct = db.browser_create(mb.id, None, "none").unwrap();
        assert_eq!(browser_proxy(&db, direct, "").unwrap(), None);
    }
}
