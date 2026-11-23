//! Our accounts' browser profiles are ordinary Shard profiles in the Shard
//! folder "Creator" (D17), made and changed through Shard's own code, as its
//! project runner does (`runner.rs` `make_profile`, `proxy.set`). A profile's
//! proxy is Shard's built-in inline proxy, taken from our pool: Shard's
//! Proxies list never gets an entry from us.

use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};

use super::store::{Db, Proxy};
use crate::space::proxy::{ProxyEntry, ProxyKind};
use crate::space::{self, Door, Ticket};

/// The Shard folder our profiles live in.
pub const FOLDER: &str = "Creator";
/// Owner rule 2026-11-22: English UI (another page language broke English selectors).
pub const PROFILE_LANGUAGE: &str = "en-US";
/// A stored profile id Shard no longer has (deleted there). Retryable: the
/// next run makes a new profile.
pub const PROFILE_GONE: &str = "ShardBrowser profile is gone";

/// The browser row's Shard profile id, made the first time the mailbox is
/// used: a Windows fingerprint with language en-US and timezone and
/// geolocation following the proxy (Python `_auto_fingerprint`), in the
/// folder "Creator" (made if missing), named after the mailbox. Its id is in
/// `browsers.path` before anything else runs. A stored id Shard no longer
/// has is cleared and the call fails `PROFILE_GONE`.
pub async fn ensure_profile(db: &Db, browser_id: i64) -> Result<String> {
    let br = db
        .browser_get(browser_id)?
        .ok_or_else(|| anyhow!("browser {browser_id} not found"))?;
    if let Some(id) = br.path.filter(|p| !p.is_empty()) {
        match space::profile::load_raw(&id) {
            Ok(_) => return Ok(id),
            // Gone only when Shard says so and its file is not here; any other
            // failure is Shard's own error (retryable) and the link is kept.
            Err(e) if gone(&e, &id) => {
                db.browser_set_path(browser_id, None)?;
                anyhow::bail!(PROFILE_GONE);
            }
            Err(e) => return Err(e),
        }
    }
    let name = db
        .mailbox_get(br.mailbox_id)?
        .map(|m| m.email)
        .unwrap_or_default();
    let id = make_profile(&name)?;
    db.browser_set_path(browser_id, Some(&id))?;
    // After the save: a refused save leaves no folder behind.
    if !space::folders::list()?.registry.iter().any(|f| f == FOLDER) {
        space::folders::add(Door("folder_add"), &Ticket::now(), FOLDER).await?;
    }
    crate::notify_store_changed("profiles");
    Ok(id)
}

fn gone(e: &anyhow::Error, id: &str) -> bool {
    matches!(
        e.downcast_ref::<space::CmdError>(),
        Some(space::CmdError::Missing(_))
    ) && crate::store::profiles_dir().is_ok_and(|d| !d.join(format!("{id}.json")).exists())
}

/// `runner.rs` `make_profile` for a Windows fingerprint, with our language,
/// timezone and geolocation.
fn make_profile(name: &str) -> Result<String> {
    let all = crate::fingerprints::list_all()?;
    let windows: Vec<_> = all
        .iter()
        .filter(|e| e.platform.eq_ignore_ascii_case("windows"))
        .collect();
    let pool = if windows.is_empty() {
        all.iter().collect()
    } else {
        windows
    };
    if pool.is_empty() {
        anyhow::bail!("the fingerprint library is empty");
    }
    let idx = (uuid::Uuid::new_v4().as_bytes()[0] as usize) % pool.len();
    let mut cfg = crate::build_fingerprint_config(None, &pool[idx].id).map_err(|e| anyhow!(e))?;
    cfg.remove("_meta");
    if !name.is_empty() {
        cfg.insert("name".into(), json!(name));
    }
    crate::ensure_default_noise(&mut cfg);
    auto_fingerprint(&mut cfg);
    cfg.insert("_meta".into(), json!({ "id": "", "folder": FOLDER }));
    let pm = crate::save_profile_here(None, Value::Object(cfg), false).map_err(|e| anyhow!(e))?;
    Ok(pm.id)
}

/// Python `_auto_fingerprint`: language en-US; timezone and geolocation follow the proxy IP.
fn auto_fingerprint(cfg: &mut serde_json::Map<String, Value>) {
    let nav = cfg.entry("navigator").or_insert_with(|| json!({}));
    if let Some(nav) = nav.as_object_mut() {
        nav.insert("language".into(), json!(PROFILE_LANGUAGE));
    }
    cfg.insert("timezone".into(), json!("auto"));
    let geo = cfg.entry("geolocation").or_insert_with(|| json!({}));
    if let Some(geo) = geo.as_object_mut() {
        geo.insert("mode".into(), json!("auto"));
    }
}

/// The profile's proxy, inline on the profile only (`runner.rs` `proxy.set`);
/// None takes it off. Shard's Proxies list is never touched.
pub fn set_proxy(profile_id: &str, proxy: Option<&Proxy>) -> Result<()> {
    let mut stored = space::profile::load_raw(profile_id)?;
    stored.meta.inline_proxy = proxy.map(entry).transpose()?;
    stored.meta.proxy_id = None;
    space::profile::save_raw(&mut stored)?;
    crate::notify_store_changed("profiles");
    Ok(())
}

/// Our proxy as Shard's entry: kind http with explicit fields (Python
/// `ensure_proxy`: a bare `host:port:user:pass` would be read as SOCKS5).
fn entry(p: &Proxy) -> Result<ProxyEntry> {
    let port = u16::try_from(p.port).context("proxy port out of range")?;
    Ok(ProxyEntry {
        id: String::new(),
        name: format!("{}:{}", p.host, p.port),
        kind: ProxyKind::Http,
        host: p.host.clone(),
        port,
        username: p.username.clone().unwrap_or_default(),
        password: p.password.clone().unwrap_or_default(),
        country: String::new(),
        notes: String::new(),
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::creator::store::temp_db;

    /// A Shard data root of the test's own with one Windows fingerprint in its library.
    pub(crate) fn shard_root(tag: &str) -> crate::store::TestRoot {
        let root = crate::store::test_root(tag).with_config();
        let lib = json!({
            "id": "win-a", "label": "Win A", "platform": "Windows", "chrome": "157", "gpu": "A", "tag_color": "",
            "payload": { "navigator": { "platform": "Win32", "user_agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64)" },
                         "screen": { "width": 1920, "height": 1080 } },
        });
        std::fs::write(
            crate::store::fingerprints_dir().unwrap().join("win-a.json"),
            lib.to_string(),
        )
        .unwrap();
        root
    }

    fn browser(db: &Db) -> i64 {
        let mb = db.mailbox_create("ele", "u@a.com", "a.com", "t").unwrap();
        db.browser_create(mb.id, None, "rotate").unwrap()
    }

    #[tokio::test]
    async fn ensure_profile_makes_creator_folder_once() {
        let _serial = crate::migrate::serial();
        let _root = shard_root("creator-folder");
        let (db, _f) = temp_db();
        let (a, b) = (browser(&db), {
            let mb = db.mailbox_create("ele", "v@a.com", "a.com", "t").unwrap();
            db.browser_create(mb.id, None, "rotate").unwrap()
        });
        let pa = ensure_profile(&db, a).await.unwrap();
        let pb = ensure_profile(&db, b).await.unwrap();
        assert_ne!(pa, pb);
        let folders = space::folders::list().unwrap().registry;
        assert_eq!(
            folders.iter().filter(|f| *f == FOLDER).count(),
            1,
            "{folders:?}"
        );
        for id in [&pa, &pb] {
            assert_eq!(space::profile::load_raw(id).unwrap().meta.folder, FOLDER);
        }
        // Again: the same profile, nothing new.
        assert_eq!(ensure_profile(&db, a).await.unwrap(), pa);
        assert_eq!(space::profile::list_all().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn ensure_profile_writes_id_before_launch() {
        let _serial = crate::migrate::serial();
        let _root = shard_root("creator-id");
        let (db, _f) = temp_db();
        let br = browser(&db);
        let id = ensure_profile(&db, br).await.unwrap();
        assert_eq!(
            db.browser_get(br).unwrap().unwrap().path.as_deref(),
            Some(id.as_str())
        );
        let p = space::profile::load_raw(&id).unwrap();
        assert_eq!(p.config["name"], "u@a.com");
        assert_eq!(p.config["navigator"]["language"], PROFILE_LANGUAGE);
        assert_eq!(p.config["navigator"]["platform"], "Win32");
        assert_eq!(p.config["timezone"], "auto");
        assert_eq!(p.config["geolocation"]["mode"], "auto");
        assert!(p.config["noise"].is_object());
        assert!(!crate::is_profile_running(&id));
    }

    #[tokio::test]
    async fn set_proxy_is_inline_and_leaves_shards_proxy_list_alone() {
        let _serial = crate::migrate::serial();
        let _root = shard_root("creator-proxy");
        let (db, _f) = temp_db();
        let id = ensure_profile(&db, browser(&db)).await.unwrap();
        let pid = db
            .proxy_add("10.0.0.1", 8080, Some("user"), Some("pw"), "static")
            .unwrap();
        let proxy = db.proxy_get(pid).unwrap().unwrap();
        set_proxy(&id, Some(&proxy)).unwrap();
        let p = space::profile::load_raw(&id).unwrap();
        let inline = p.meta.inline_proxy.expect("inline proxy");
        assert_eq!(
            (inline.kind, inline.host.as_str(), inline.port),
            (ProxyKind::Http, "10.0.0.1", 8080)
        );
        assert_eq!(
            (inline.username.as_str(), inline.password.as_str()),
            ("user", "pw")
        );
        assert_eq!(p.meta.proxy_id, None);
        assert!(space::proxy::list().unwrap().is_empty());
        set_proxy(&id, None).unwrap();
        assert!(space::profile::load_raw(&id)
            .unwrap()
            .meta
            .inline_proxy
            .is_none());
    }

    #[tokio::test]
    async fn deleted_profile_is_gone_then_recreated() {
        let _serial = crate::migrate::serial();
        let _root = shard_root("creator-gone");
        let (db, _f) = temp_db();
        let br = browser(&db);
        let old = ensure_profile(&db, br).await.unwrap();
        space::profile::delete(&old).unwrap();
        let err = ensure_profile(&db, br).await.unwrap_err();
        assert_eq!(err.to_string(), PROFILE_GONE);
        assert_eq!(db.browser_get(br).unwrap().unwrap().path, None);
        let new = ensure_profile(&db, br).await.unwrap();
        assert_ne!(new, old);
    }

    #[tokio::test]
    async fn another_load_error_keeps_the_link() {
        let _serial = crate::migrate::serial();
        let _root = shard_root("creator-unreadable");
        let (db, _f) = temp_db();
        let br = browser(&db);
        let id = ensure_profile(&db, br).await.unwrap();
        let file = crate::store::profiles_dir()
            .unwrap()
            .join(format!("{id}.json"));
        std::fs::write(&file, b"not json").unwrap();
        let err = ensure_profile(&db, br).await.unwrap_err();
        assert_ne!(err.to_string(), PROFILE_GONE);
        assert_eq!(
            db.browser_get(br).unwrap().unwrap().path.as_deref(),
            Some(id.as_str())
        );
    }
}
