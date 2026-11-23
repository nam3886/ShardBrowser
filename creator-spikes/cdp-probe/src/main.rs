//! Probe: chromiumoxide against a ShardBrowser profile started by the launcher.
//! usage: cdp-probe <ws://.../devtools/browser/...>
use chromiumoxide::cdp::browser_protocol::network::GetCookiesParams;
use chromiumoxide::types::{Command, Method, MethodId};
use chromiumoxide::Browser;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

/// A command the generated bindings do not know: ShardX's own `Motion` domain.
#[derive(Serialize, Debug, Clone)]
struct MotionCreatePointer { x: f64, y: f64 }
#[derive(Deserialize, Debug, Clone)]
struct Raw(serde_json::Value);
impl Method for MotionCreatePointer {
    fn identifier(&self) -> MethodId { Cow::Borrowed("Motion.createPointer") }
}
impl Command for MotionCreatePointer { type Response = Raw; }

#[derive(Serialize, Debug, Clone)]
struct MotionGlideTo { x: f64, y: f64 }
impl Method for MotionGlideTo {
    fn identifier(&self) -> MethodId { Cow::Borrowed("Motion.glideTo") }
}
impl Command for MotionGlideTo { type Response = Raw; }

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let ws = std::env::args().nth(1).expect("ws url");
    let (browser, mut handler) = Browser::connect(&ws).await?;
    tokio::spawn(async move { while handler.next().await.is_some() {} });

    let page = browser.new_page("https://example.com").await?;
    page.wait_for_navigation().await?;
    println!("title = {:?}", page.get_title().await?);
    println!("url   = {:?}", page.url().await?);

    // CSS locator + text (chromiumoxide uses DOM.getDocument + DOM.querySelector node ids).
    for attempt in 1..=3 {
        match page.find_element("a").await {
            Ok(h1) => {
                println!("find_element try {attempt}: ok, text={:?}, box={:?}", h1.inner_text().await.map_err(|e| e.to_string()), h1.bounding_box().await.map_err(|e| e.to_string()));
                break;
            }
            Err(e) => println!("find_element try {attempt}: {e}"),
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    println!("html = {:?}", page.evaluate("document.documentElement.outerHTML.slice(0,300)").await.map(|v| v.into_value::<String>().ok()).map_err(|e| e.to_string()));
    let n = page.evaluate("document.querySelectorAll('a').length").await.map(|v| v.into_value::<i64>().ok());
    println!("evaluate links = {:?}", n.map_err(|e| e.to_string()));
    let rect = page.evaluate("JSON.stringify(document.querySelector('a').getBoundingClientRect())").await.map(|v| v.into_value::<String>().ok());
    println!("evaluate h1 rect = {:?}", rect.map_err(|e| e.to_string()));

    // Custom domain through the generic Command trait.
    let r1 = page.execute(MotionCreatePointer { x: 10.0, y: 10.0 }).await;
    println!("Motion.createPointer -> {:?}", r1.as_ref().map(|r| &r.result).map_err(|e| e.to_string()));
    let r2 = page.execute(MotionGlideTo { x: 120.0, y: 140.0 }).await;
    println!("Motion.glideTo -> {:?}", r2.as_ref().map(|r| &r.result).map_err(|e| e.to_string()));

    let c = page.execute(GetCookiesParams::builder().urls(vec!["https://example.com".to_string()]).build()).await;
    println!("cookies(example.com) = {:?}", c.map(|c| c.result.cookies.len()).map_err(|e| e.to_string()));
    println!("frames = {:?}", page.frames().await.map(|f| f.len()).map_err(|e| e.to_string()));
    page.close().await?;
    Ok(())
}
