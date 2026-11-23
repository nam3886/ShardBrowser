//! D13 stealth probe: what `fixtures/detect.html` sees of a CDP client, on the
//! same browser, in three conditions:
//! - `unattached`: a tab opened over the browser endpoint, no client on it (the baseline);
//! - `attached_no_runtime`: a raw client attached to the tab (Page domain only, no Runtime.enable);
//! - `chromiumoxide`: our Session/Page on the patched chromiumoxide (no Runtime.enable,
//!   see `creator-vendor/chromiumoxide/PATCHES.md`).
//!
//! Run against a real profile started with CDP by the launcher:
//! `CREATOR_PROBE_WS=ws://127.0.0.1:<port>/devtools/browser/<id> cargo test … stealth_probe -- --ignored --nocapture`;
//! without it, against the bare test engine. Reports, does not assert.

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio_tungstenite::tungstenite::Message;

use super::testkit::{Bare, FixtureServer};
use super::Session;

type Ws =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// A bare CDP connection: one call at a time, events ignored.
struct Raw {
    ws: Ws,
    id: u64,
}

impl Raw {
    async fn connect(url: &str) -> Raw {
        let (ws, _) = tokio_tungstenite::connect_async(url)
            .await
            .expect("connect");
        Raw { ws, id: 0 }
    }

    async fn call(&mut self, method: &str, params: Value, session: Option<&str>) -> Value {
        self.id += 1;
        let mut msg = json!({ "id": self.id, "method": method, "params": params });
        if let Some(s) = session {
            msg["sessionId"] = json!(s);
        }
        self.ws.send(Message::Text(msg.to_string())).await.unwrap();
        while let Some(Ok(m)) = self.ws.next().await {
            if let Message::Text(t) = m {
                let v: Value = serde_json::from_str(&t).unwrap();
                if v["id"] == json!(self.id) {
                    return v["result"].clone();
                }
            }
        }
        panic!("connection closed");
    }
}

fn http_base(ws: &str) -> String {
    let port = ws
        .trim_start_matches("ws://")
        .split('/')
        .next()
        .unwrap()
        .to_string();
    format!("http://{port}")
}

/// The tab's title once the probe wrote its JSON there, read from `/json/list`
/// (which attaches nothing).
async fn reported(base: &str, target: &str) -> String {
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let list: Vec<Value> = reqwest::get(format!("{base}/json/list"))
            .await
            .unwrap()
            .json()
            .await
            .unwrap_or_default();
        if let Some(t) = list
            .iter()
            .find(|t| t["id"] == json!(target))
            .and_then(|t| t["title"].as_str())
            .filter(|t| t.starts_with('{'))
        {
            // /json/list HTML-escapes titles.
            return t.replace("&quot;", "\"").replace("&amp;", "&");
        }
    }
    panic!("the probe page never reported");
}

async fn raw_condition(ws: &str, url: &str, attach: bool) -> String {
    let mut raw = Raw::connect(ws).await;
    let first = if attach { "about:blank" } else { url };
    let t = raw
        .call("Target.createTarget", json!({ "url": first }), None)
        .await;
    let target = t["targetId"].as_str().unwrap().to_string();
    if attach {
        let a = raw
            .call(
                "Target.attachToTarget",
                json!({ "targetId": target, "flatten": true }),
                None,
            )
            .await;
        let session = a["sessionId"].as_str().unwrap().to_string();
        raw.call("Page.enable", json!({}), Some(&session)).await;
        raw.call("Page.navigate", json!({ "url": url }), Some(&session))
            .await;
    }
    let out = reported(&http_base(ws), &target).await;
    raw.call("Target.closeTarget", json!({ "targetId": target }), None)
        .await;
    out
}

#[tokio::test]
#[ignore]
async fn stealth_probe() {
    let site = FixtureServer::start().await;
    let url = format!("{}/detect.html", site.site);
    let bare;
    let ws = match std::env::var("CREATOR_PROBE_WS") {
        Ok(ws) => ws,
        Err(_) => {
            let Some(b) = Bare::launch().await else {
                return;
            };
            bare = b;
            bare.ws.clone()
        }
    };
    println!(
        "PROBE unattached          {}",
        raw_condition(&ws, &url, false).await
    );
    println!(
        "PROBE attached_no_runtime {}",
        raw_condition(&ws, &url, true).await
    );
    let session = Session::connect(&ws).await.unwrap();
    let page = session.page().await.unwrap();
    page.goto(&url).await.unwrap();
    tokio::time::sleep(Duration::from_millis(1500)).await;
    println!("PROBE chromiumoxide       {}", page.title().await);
}

/// The public detection suites, each with the time it needs to finish its checks.
const SUITES: [(&str, &str, u64); 5] = [
    ("rebrowser", "https://bot-detector.rebrowser.net/", 4),
    (
        "deviceandbrowserinfo",
        "https://deviceandbrowserinfo.com/are_you_a_bot",
        12,
    ),
    ("creepjs", "https://abrahamjuliot.github.io/creepjs/", 25),
    (
        "browserscan",
        "https://www.browserscan.net/bot-detection",
        15,
    ),
    ("sannysoft", "https://bot.sannysoft.com", 8),
];
const EXTRACT: &str = include_str!("fixtures/bench_extract.js");

/// The rebrowser page's own instructions, done the same way by every client:
/// main world `dummyFn()` and `getElementById`, isolated world `getElementsByClassName`.
/// (No exposeFunction, no init script: we have neither.)
const RB_MAIN: [&str; 2] = [
    "() => window.dummyFn()",
    "() => !!document.getElementById('detections-json')",
];
const RB_ISOLATED: &str = "() => document.getElementsByClassName('div').length";

/// The interaction fixture: click, type, click (as a provider flow does).
async fn interact_ours(page: &super::Page) {
    let t = Duration::from_secs(10);
    page.css("#a").click(t).await.unwrap();
    page.css("#name").fill("hello", t).await.unwrap();
    page.css("#b").click(t).await.unwrap();
}

/// Stealth bench (D13): every suite in `SUITES` plus `fixtures/interact.html`, read
/// from the DOM, for two clients on the running profile at `CREATOR_PROBE_WS`:
/// `baseline` (a tab opened by Target.createTarget with no client on
/// it; read after the wait by a late raw client) and `ours` (Session/Page).
/// patchright runs the same list from `creator-spikes/stealth-bench`. Results go to
/// `CREATOR_BENCH_OUT/<client>.json`, screenshots next to them.
#[tokio::test]
#[ignore]
async fn stealth_bench() {
    let ws = std::env::var("CREATOR_PROBE_WS").expect("CREATOR_PROBE_WS");
    let out =
        std::path::PathBuf::from(std::env::var("CREATOR_BENCH_OUT").expect("CREATOR_BENCH_OUT"));
    std::fs::create_dir_all(&out).unwrap();
    let site = FixtureServer::start().await;
    let mut suites: Vec<(&str, String, u64)> = SUITES
        .iter()
        .map(|(n, u, w)| (*n, u.to_string(), *w))
        .collect();
    suites.push(("detect", format!("{}/detect.html", site.site), 2));
    suites.push(("interact", format!("{}/interact.html", site.site), 1));

    let mut baseline = serde_json::Map::new();
    for (name, url, wait) in &suites {
        if *name == "interact" {
            continue; // nothing to measure without input
        }
        baseline.insert(
            name.to_string(),
            raw_read(&ws, url, *wait, &out.join(format!("baseline-{name}.png"))).await,
        );
    }
    std::fs::write(
        out.join("baseline.json"),
        serde_json::to_string_pretty(&baseline).unwrap(),
    )
    .unwrap();

    let session = Session::connect(&ws).await.unwrap();
    let page = session.page().await.unwrap();
    let mut ours = serde_json::Map::new();
    for (name, url, wait) in &suites {
        page.goto(url).await.unwrap();
        tokio::time::sleep(Duration::from_secs(*wait)).await;
        if *name == "rebrowser" {
            for js in RB_MAIN {
                let _ = page.eval::<Value>(js, super::World::Main).await;
            }
            let _ = page
                .eval::<Value>(RB_ISOLATED, super::World::Isolated)
                .await;
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
        if *name == "interact" {
            interact_ours(&page).await;
        }
        let r: Value = page.eval(EXTRACT, super::World::Isolated).await.unwrap();
        let _ = page.screenshot(&out.join(format!("ours-{name}.png"))).await;
        ours.insert(name.to_string(), r);
    }
    std::fs::write(
        out.join("ours.json"),
        serde_json::to_string_pretty(&ours).unwrap(),
    )
    .unwrap();
    println!("BENCH written to {}", out.display());
}

/// A tab nobody is attached to while it loads; after `wait` a raw client attaches
/// (Page domain only, no Runtime.enable) to read the DOM and take a screenshot.
async fn raw_read(ws: &str, url: &str, wait: u64, shot: &std::path::Path) -> Value {
    use base64::Engine;
    let mut raw = Raw::connect(ws).await;
    let t = raw
        .call("Target.createTarget", json!({ "url": url }), None)
        .await;
    let target = t["targetId"].as_str().unwrap().to_string();
    tokio::time::sleep(Duration::from_secs(wait + 3)).await;
    let a = raw
        .call(
            "Target.attachToTarget",
            json!({ "targetId": target, "flatten": true }),
            None,
        )
        .await;
    let s = a["sessionId"].as_str().unwrap().to_string();
    let w = raw
        .call(
            "Page.createIsolatedWorld",
            json!({ "frameId": target, "worldName": "bench" }),
            Some(&s),
        )
        .await;
    let r = raw
        .call(
            "Runtime.callFunctionOn",
            json!({ "functionDeclaration": EXTRACT, "executionContextId": w["executionContextId"],
                    "returnByValue": true }),
            Some(&s),
        )
        .await;
    let png = raw
        .call("Page.captureScreenshot", json!({}), Some(&s))
        .await;
    if let Some(b) = png["data"].as_str() {
        let _ = std::fs::write(
            shot,
            base64::engine::general_purpose::STANDARD
                .decode(b)
                .unwrap_or_default(),
        );
    }
    raw.call("Target.closeTarget", json!({ "targetId": target }), None)
        .await;
    r["result"]["value"].clone()
}
