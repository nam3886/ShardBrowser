//! Creator: our feature module on top of the upstream launcher.
//!
//! Isolation rule: everything of ours lives under `src-tauri/src/creator/`.
//! Upstream sees exactly two lines in `lib.rs`: `mod creator;` and one entry
//! (`creator::creator_call`) in `generate_handler!`. Every new command goes
//! through the `creator_call` dispatcher, so adding commands never touches
//! upstream files again.

// Used by the providers and the runner from M2 Task 11 on.
#[allow(dead_code)]
pub mod mail;
pub mod shard;
// Used by the runner from M2 Task 12 on.
#[allow(dead_code)]
pub mod store;
// Used by the providers from M2 on.
#[allow(dead_code)]
pub mod web;

use chromiumoxide::Browser;
use futures_util::StreamExt;
use serde_json::{json, Value};

/// The one Tauri command we register. `cmd` picks the action, `args` carries its input.
#[tauri::command]
pub async fn creator_call(cmd: String, args: Option<Value>) -> Result<Value, String> {
    let args = args.unwrap_or(Value::Null);
    match cmd.as_str() {
        "profiles" => profiles().await,
        "open_example" => {
            let id = args["profile_id"].as_str().ok_or("profile_id required")?;
            open_example(id).await
        }
        other => Err(anyhow::anyhow!("unknown creator command: {other}")),
    }
    .map_err(|e| format!("{e:#}"))
}

async fn profiles() -> anyhow::Result<Value> {
    crate::space::ready().await;
    let list = crate::space::profile::list_all()?;
    Ok(json!(list
        .iter()
        .map(|p| json!({ "id": p.id, "name": p.name, "running": crate::is_profile_running(&p.id) }))
        .collect::<Vec<_>>()))
}

/// Open the profile with CDP (attach, restart or start: `shard::open_cdp`),
/// connect with chromiumoxide, open example.com and read its title back.
async fn open_example(profile_id: &str) -> anyhow::Result<Value> {
    let cdp = shard::open_cdp(profile_id).await?;

    let (browser, mut handler) = Browser::connect(&cdp.web_socket_debugger_url).await?;
    // chromiumoxide needs its handler polled for the whole session.
    let pump = tokio::spawn(async move { while handler.next().await.is_some() {} });

    let page = browser.new_page("https://example.com").await?;
    page.wait_for_navigation().await?;
    let title = page.get_title().await?.unwrap_or_default();
    let url = page.url().await?.unwrap_or_default();

    // Leave the browser running (the operator may keep using it); just drop our connection.
    drop(browser);
    pump.abort();
    Ok(json!({ "cdp": cdp.http_url, "url": url, "title": title }))
}

#[cfg(test)]
mod tests {
    /// We keep Shard's identity (app id, data folder, port, version); only the
    /// update channel is ours: a build that took upstream's feed would replace
    /// our app with stock ShardBrowser on its next update.
    #[test]
    fn updates_come_from_our_repo() {
        let conf: serde_json::Value =
            serde_json::from_str(include_str!("../../tauri.conf.json")).unwrap();
        let updater = &conf["plugins"]["updater"];
        assert_eq!(
            updater["endpoints"],
            serde_json::json!([
                "https://github.com/nam3886/ShardBrowser/releases/latest/download/latest.json"
            ])
        );
        // Our minisign key (id B01CBEFB9E8DC521); the private half stays in ~/.tauri.
        assert_eq!(updater["pubkey"], "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IEIwMUNCRUZCOUU4REM1MjEKUldRaHhZMmUrNzRjc01kYmdBcmRVSTZPK3ArTWlOQ2p4WlJvOXA3UGl3N1VwY1dodnl1SVVOM00K");
        assert!(include_str!("../runtime.rs")
            .contains(r#"const LAUNCHER_RELEASE_REPO: &str = "nam3886/ShardBrowser";"#));
    }
}
