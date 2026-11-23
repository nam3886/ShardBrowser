//! Creator: our feature module on top of the upstream launcher.
//!
//! Isolation rule: everything of ours lives under `src-tauri/src/creator/`.
//! Upstream sees exactly two lines in `lib.rs`: `mod creator;` and one entry
//! (`creator::creator_call`) in `generate_handler!`. Every new command goes
//! through the `creator_call` dispatcher, so adding commands never touches
//! upstream files again.

pub mod errors;
// Used by the providers and the runner from M2 Tasks 11-12 on.
#[allow(dead_code)]
pub mod mail;
pub mod pages;
#[allow(dead_code)]
pub mod profile;
#[allow(dead_code)]
pub mod providers;
#[allow(dead_code)]
pub mod proxy;
pub mod runner;
#[allow(dead_code)]
pub mod shard;
#[allow(dead_code)]
pub mod store;
#[cfg(test)]
mod testsite;
// Used by the providers from M2 on.
#[allow(dead_code)]
pub mod web;

use serde_json::Value;

/// The one Tauri command we register. `cmd` picks the action, `args` carries its input.
#[tauri::command]
pub async fn creator_call(cmd: String, args: Option<Value>) -> Result<Value, String> {
    let args = args.unwrap_or(Value::Null);
    match pages::call(&cmd, &args).await {
        Some(r) => r,
        None => Err(anyhow::anyhow!("unknown creator command: {cmd}")),
    }
    .map_err(|e| format!("{e:#}"))
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
