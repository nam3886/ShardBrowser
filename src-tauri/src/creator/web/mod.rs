//! The automation layer: a small Playwright-like surface over chromiumoxide, with
//! every pointer and key going through the engine's `Motion` domain.
//!
//! Errors never carry URLs, field values or page text (they end up in run events).

pub mod frames;
pub mod input;
pub mod locator;
pub mod watch;

#[cfg(test)]
mod probe;
#[cfg(test)]
pub mod testkit;

use std::fmt;
use std::path::Path;
use std::time::Duration;

use chromiumoxide::cdp::browser_protocol::page::{
    EventFrameNavigated, EventLifecycleEvent, NavigateParams,
};
use chromiumoxide::cdp::browser_protocol::target::{CloseTargetParams, GetTargetsParams};
use chromiumoxide::cdp::js_protocol::runtime::{CallFunctionOnParams, ExecutionContextId};
use chromiumoxide::handler::HandlerConfig;
use chromiumoxide::page::ScreenshotParams;
use chromiumoxide::Browser;
use futures_util::StreamExt;
use serde::de::DeserializeOwned;

pub use watch::Watch;

pub const BROWSER_HUNG: &str = "Browser stopped responding";
/// Python `tts_base.GOTO_TRIES` / `GOTO_GAP_S`.
pub const GOTO_TRIES: u32 = 3;
const GOTO_GAP: Duration = Duration::from_secs(3);
const GOTO_TIMEOUT: Duration = Duration::from_secs(60);
/// Python `session.CLEANUP_TIMEOUT`: opening the fresh tab and closing the restored ones.
const CLEANUP_TIMEOUT: Duration = Duration::from_secs(15);
/// Python `NAV_ERRORS`: a navigation failure worth another try.
const NAV_ERRORS: [&str; 3] = ["net::ERR_", "NS_ERROR_", "NS_BINDING_"];

#[derive(Debug, Clone, PartialEq)]
pub enum WebError {
    Timeout(&'static str),
    Hung,
    NotFound,
    Covered,
    Disabled,
    /// The browser's own error text (`net::ERR_…`), never the URL.
    Nav(String),
    Cdp(String),
}

impl fmt::Display for WebError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WebError::Timeout(what) => write!(f, "timed out: {what}"),
            WebError::Hung => f.write_str(BROWSER_HUNG),
            WebError::NotFound => f.write_str("element not found"),
            WebError::Covered => f.write_str("element is covered by another element"),
            WebError::Disabled => f.write_str("element is disabled"),
            WebError::Nav(e) => write!(f, "navigation failed: {e}"),
            WebError::Cdp(e) => write!(f, "browser error: {e}"),
        }
    }
}

impl std::error::Error for WebError {}

pub type WebResult<T> = Result<T, WebError>;

/// chromiumoxide's error text. Its messages are protocol errors ("Could not find
/// node with given id"), not page content.
pub(crate) fn cdp(e: impl fmt::Display) -> WebError {
    WebError::Cdp(e.to_string())
}

/// A handle that a navigation made stale: the context or object the call used
/// belongs to a document that is gone. Worth trying again on the new one.
pub(crate) fn stale(e: &WebError) -> bool {
    let WebError::Cdp(m) = e else { return false };
    [
        "Cannot find context with specified id",
        "Could not find object with given id",
        "Execution context was destroyed",
        "No node with given id found",
        "Inspected target navigated or closed",
    ]
    .iter()
    .any(|s| m.contains(s))
}

/// `f` again while its error is `stale` (a navigation in between), at most ~2 s.
pub(crate) async fn retry_stale<T>(mut f: impl AsyncFnMut() -> WebResult<T>) -> WebResult<T> {
    let mut tries = 0;
    loop {
        match f().await {
            Err(e) if stale(&e) && tries < 20 => tries += 1,
            r => return r,
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// CSS px in viewport coordinates.
#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Where page script runs. `Isolated` (the default) cannot see the page's globals
/// and the page cannot see it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum World {
    #[default]
    Isolated,
    Main,
}

/// One CDP connection to a running browser. Dropping it leaves the browser running.
pub struct Session {
    browser: Browser,
    pump: tokio::task::JoinHandle<()>,
}

impl Drop for Session {
    fn drop(&mut self) {
        self.pump.abort();
    }
}

impl Session {
    pub async fn connect(ws_url: &str) -> WebResult<Session> {
        let config = HandlerConfig {
            // A normal browser refuses a bad certificate; so do we.
            ignore_https_errors: false,
            // Python's goto budget: chromiumoxide's own navigation watcher must outlive ours.
            request_timeout: GOTO_TIMEOUT,
            ..HandlerConfig::default()
        };
        let (browser, mut handler) = Browser::connect_with_config(ws_url, config)
            .await
            .map_err(cdp)?;
        let pump = tokio::spawn(async move { while handler.next().await.is_some() {} });
        Ok(Session { browser, pump })
    }

    /// The profile opened with CDP (`shard::open_cdp`), connected.
    pub async fn from_profile(profile_id: &str) -> anyhow::Result<Session> {
        let cdp = crate::creator::shard::open_cdp(profile_id).await?;
        Ok(Session::connect(&cdp.web_socket_debugger_url).await?)
    }

    /// A fresh blank tab, every other tab closed (Python `_one_fresh_page`: the
    /// browser restores the last run's tabs; left open, a flow would drive a
    /// background tab). The new tab comes first so the window never closes.
    pub async fn page(&self) -> WebResult<Page> {
        let work = async {
            let inner = self.browser.new_page("about:blank").await.map_err(cdp)?;
            let targets = self
                .browser
                .execute(GetTargetsParams::default())
                .await
                .map_err(cdp)?;
            for t in &targets.result.target_infos {
                if t.r#type == "page" && &t.target_id != inner.target_id() {
                    // A tab that is gone already, or will not close, is no reason to fail.
                    let _ = self
                        .browser
                        .execute(CloseTargetParams::new(t.target_id.clone()))
                        .await;
                }
            }
            Ok(Page::new(inner))
        };
        tokio::time::timeout(CLEANUP_TIMEOUT, work)
            .await
            .map_err(|_| WebError::Timeout("open a tab"))?
    }
}

/// One tab. Clones share the connection and the hang watch.
#[derive(Clone)]
pub struct Page {
    pub(crate) inner: chromiumoxide::Page,
    pub(crate) watch: Watch,
    pub(crate) input: std::sync::Arc<input::InputState>,
}

impl Page {
    fn new(inner: chromiumoxide::Page) -> Self {
        Page {
            inner,
            watch: Watch::default(),
            input: Default::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_watch(mut self, watch: Watch) -> Self {
        self.watch = watch;
        self
    }

    pub fn watch(&self) -> &Watch {
        &self.watch
    }

    pub fn hung(&self) -> bool {
        self.watch.hung()
    }

    /// Navigate and wait for `DOMContentLoaded` (60 s); a navigation error
    /// (`net::ERR_…`) or timeout is tried again, `GOTO_TRIES` in all, 3 s apart.
    pub async fn goto(&self, url: &str) -> WebResult<()> {
        let mut last = WebError::Timeout("navigation");
        for i in 0..GOTO_TRIES {
            if i > 0 {
                tokio::time::sleep(GOTO_GAP).await;
            }
            match self.goto_once(url).await {
                Ok(()) => return Ok(()),
                Err(e @ (WebError::Nav(_) | WebError::Timeout(_))) => last = e,
                Err(e) => return Err(e),
            }
        }
        Err(last)
    }

    async fn goto_once(&self, url: &str) -> WebResult<()> {
        let mut main = self.inner.mainframe().await.map_err(cdp)?;
        let mut committed = self
            .inner
            .event_listener::<EventFrameNavigated>()
            .await
            .map_err(cdp)?;
        let mut lifecycle = self
            .inner
            .event_listener::<EventLifecycleEvent>()
            .await
            .map_err(cdp)?;
        // ponytail: chromiumoxide answers Page.navigate only after `load`; we return at
        // DOMContentLoaded and its watcher finishes in the background (a goto right
        // after a slow page waits for it, at most request_timeout).
        let nav = self.inner.execute(NavigateParams::new(url));
        tokio::pin!(nav);
        let mut error_page = false;
        let wait = async {
            loop {
                tokio::select! {
                    r = &mut nav => {
                        let r = r.map_err(|e| nav_error(&e.to_string()))?;
                        return match r.result.error_text {
                            Some(e) => Err(nav_error(&e)),
                            None => Ok(()),
                        };
                    }
                    Some(ev) = committed.next() => {
                        if ev.frame.parent_id.is_none() {
                            main = Some(ev.frame.id.clone());
                            error_page = ev.frame.unreachable_url.is_some();
                        }
                    }
                    Some(ev) = lifecycle.next() => {
                        let is_main = Some(&ev.frame_id) == main.as_ref();
                        if is_main && ev.name == "DOMContentLoaded" && !error_page {
                            return Ok(());
                        }
                    }
                }
            }
        };
        tokio::time::timeout(GOTO_TIMEOUT, wait)
            .await
            .map_err(|_| WebError::Timeout("navigation"))?
    }

    /// Reload with its own deadline (as Python's `page.reload(timeout=)`): a slow
    /// load is a timeout, not a hung browser.
    pub async fn reload(&self) -> WebResult<()> {
        let f = async { self.inner.reload().await.map(|_| ()).map_err(cdp) };
        tokio::time::timeout(GOTO_TIMEOUT, f)
            .await
            .map_err(|_| WebError::Timeout("reload"))?
    }

    pub async fn url(&self) -> String {
        let f = async { self.inner.url().await.ok().flatten().unwrap_or_default() };
        self.watch.call(f, String::new()).await
    }

    pub async fn title(&self) -> String {
        self.eval::<String>("() => document.title", World::Isolated)
            .await
            .unwrap_or_default()
    }

    /// `document.body.innerText`, "" on any failure.
    pub async fn body_text(&self) -> String {
        let js = "() => document.body ? document.body.innerText : ''";
        let f = self.eval_raw::<String>(js, World::Isolated);
        self.watch
            .call(f, Ok(String::new()))
            .await
            .unwrap_or_default()
    }

    /// Run a JS function (`"() => …"`) in the page's main frame and return its value.
    pub async fn eval<T: DeserializeOwned>(&self, js_fn: &str, world: World) -> WebResult<T> {
        self.watch.act(self.eval_raw(js_fn, world)).await
    }

    pub(crate) async fn eval_raw<T: DeserializeOwned>(
        &self,
        js_fn: &str,
        world: World,
    ) -> WebResult<T> {
        retry_stale(async || {
            let ctx = self.context(world).await?;
            let call = CallFunctionOnParams::builder()
                .function_declaration(js_fn)
                .execution_context_id(ctx)
                .return_by_value(true)
                .await_promise(true)
                .build()
                .map_err(WebError::Cdp)?;
            self.call_fn(call).await
        })
        .await
    }

    pub(crate) async fn call_fn<T: DeserializeOwned>(
        &self,
        call: CallFunctionOnParams,
    ) -> WebResult<T> {
        let r = self.inner.execute(call).await.map_err(cdp)?;
        if let Some(ex) = &r.result.exception_details {
            // The exception's own text (e.g. "TypeError: …"): our script's failure, not page data.
            let text = ex
                .exception
                .as_ref()
                .and_then(|e| e.description.clone())
                .unwrap_or_else(|| ex.text.clone());
            return Err(WebError::Cdp(text.lines().next().unwrap_or("").to_string()));
        }
        let v = r.result.result.value.clone().unwrap_or_default();
        serde_json::from_value(v).map_err(cdp)
    }

    /// The main frame's execution context in `world`, asked of the browser on each
    /// call (the patched chromiumoxide: `Page.createIsolatedWorld`, or a main-world
    /// object's context id). None until the main frame is known, so this waits a
    /// moment; a context that a navigation destroys later is `stale`.
    pub(crate) async fn context(&self, world: World) -> WebResult<ExecutionContextId> {
        for _ in 0..50 {
            let ctx = match world {
                World::Isolated => self.inner.secondary_execution_context().await,
                World::Main => self.inner.execution_context().await,
            }
            .map_err(cdp)?;
            if let Some(ctx) = ctx {
                return Ok(ctx);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(WebError::Timeout("page script context"))
    }

    pub async fn screenshot(&self, path: &Path) -> WebResult<()> {
        let f = async {
            self.inner
                .save_screenshot(ScreenshotParams::builder().build(), path)
                .await
                .map(|_| ())
                .map_err(cdp)
        };
        self.watch.act(f).await
    }
}

fn nav_error(text: &str) -> WebError {
    match NAV_ERRORS.iter().find_map(|p| text.find(p)) {
        Some(at) => WebError::Nav(
            text[at..]
                .split_whitespace()
                .next()
                .unwrap_or("")
                .to_string(),
        ),
        None if text.contains("Timeout") || text.contains("timed out") => {
            WebError::Timeout("navigation")
        }
        None => WebError::Cdp(text.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::testkit::{FixtureServer, TestBrowser};
    use super::*;

    #[test]
    fn nav_error_keeps_only_the_browsers_code() {
        assert_eq!(
            nav_error("net::ERR_CONNECTION_CLOSED at http://x/secret"),
            WebError::Nav("net::ERR_CONNECTION_CLOSED".into())
        );
        assert!(matches!(nav_error("Request Timeout"), WebError::Timeout(_)));
        assert!(!WebError::Nav("net::ERR_X".into())
            .to_string()
            .contains("http"));
    }

    #[tokio::test]
    async fn goto_reads_title_and_body() {
        let Some(b) = TestBrowser::launch().await else {
            return;
        };
        let site = FixtureServer::start().await;
        let page = b.page().await;
        page.goto(&format!("{}/basic.html", site.site))
            .await
            .unwrap();
        assert_eq!(page.title().await, "Basic");
        assert!(page.body_text().await.contains("hello"));
        assert!(page.url().await.ends_with("/basic.html"));
    }

    #[tokio::test]
    async fn eval_isolated_cannot_see_page_globals() {
        let Some(b) = TestBrowser::launch().await else {
            return;
        };
        let site = FixtureServer::start().await;
        let page = b.page().await;
        page.goto(&format!("{}/basic.html", site.site))
            .await
            .unwrap();
        let js = "() => typeof window.secret";
        assert_eq!(
            page.eval::<String>(js, World::Isolated).await.unwrap(),
            "undefined"
        );
        assert_eq!(
            page.eval::<String>(js, World::Main).await.unwrap(),
            "number"
        );
        // One isolated world per document, not a new one per call (no Runtime.enable:
        // the context comes from Page.createIsolatedWorld each time).
        let mark = "() => (globalThis.__n = (globalThis.__n || 0) + 1)";
        page.eval::<u32>(mark, World::Isolated).await.unwrap();
        assert_eq!(page.eval::<u32>(mark, World::Isolated).await.unwrap(), 2);
        // The isolated world is there again after a navigation.
        page.reload().await.unwrap();
        assert_eq!(
            page.eval::<String>(js, World::Isolated).await.unwrap(),
            "undefined"
        );
    }

    #[tokio::test]
    async fn goto_retries_net_errors() {
        let Some(b) = TestBrowser::launch().await else {
            return;
        };
        // Tries at ~0 s and ~3 s fail, the one at ~6 s lands.
        let (url, hits) = testkit::flaky_server(Duration::from_millis(4500)).await;
        let page = b.page().await;
        let t = std::time::Instant::now();
        page.goto(&url).await.unwrap();
        assert!(t.elapsed() >= 2 * GOTO_GAP, "three tries, two gaps");
        assert!(hits.load(std::sync::atomic::Ordering::SeqCst) >= 3);
        assert_eq!(page.title().await, "Flaky");
    }

    #[tokio::test]
    async fn goto_gives_up_after_three_tries() {
        let Some(b) = TestBrowser::launch().await else {
            return;
        };
        let (url, _) = testkit::flaky_server(Duration::from_secs(60)).await;
        let page = b.page().await;
        let t = std::time::Instant::now();
        let r = page.goto(&url).await;
        assert!(
            matches!(r, Err(WebError::Nav(ref e)) if e.starts_with("net::ERR_")),
            "{r:?}"
        );
        let took = t.elapsed();
        assert!(
            took >= 2 * GOTO_GAP && took < 3 * GOTO_GAP + Duration::from_secs(2),
            "{took:?}"
        );
    }

    #[tokio::test]
    async fn frozen_renderer_marks_the_page_hung() {
        let Some(b) = TestBrowser::launch().await else {
            return;
        };
        let site = FixtureServer::start().await;
        let quick = Duration::from_millis(300);
        let page = b
            .page()
            .await
            .with_watch(Watch::with_timeouts(quick, quick));
        page.goto(&format!("{}/hang.html", site.site))
            .await
            .unwrap();
        page.eval::<()>("() => window.freeze()", World::Main)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        for _ in 0..3 {
            assert_eq!(page.body_text().await, "");
        }
        assert!(page.hung());
        assert_eq!(
            page.eval::<u8>("() => 1", World::Isolated).await,
            Err(WebError::Hung)
        );
        assert_eq!(WebError::Hung.to_string(), BROWSER_HUNG);
    }
}

#[cfg(test)]
mod engine_tests {
    use super::testkit::{FixtureServer, TestBrowser};
    use super::*;
    use axum::http::header;
    use axum::routing::get;

    const SW_PAGE: &str = r#"<!doctype html><title>sw</title><script>
      const t0 = performance.now();
      navigator.serviceWorker.register("/sw.js").then(async (reg) => {
        const w = reg.installing || reg.waiting || reg.active;
        const ch = new MessageChannel();
        ch.port1.onmessage = () => { document.title = "ms:" + Math.round(performance.now() - t0); };
        w.postMessage("ping", [ch.port2]);
      });
    </script>"#;
    const SW_JS: &str = "self.onmessage = (e) => e.ports[0].postMessage('pong');";

    /// Calls made while client-side redirects replace the document keep working:
    /// a context or handle from the page that just went away is tried again.
    #[tokio::test]
    async fn calls_survive_client_side_redirects() {
        let Some(b) = TestBrowser::launch().await else {
            return;
        };
        let site = FixtureServer::start().await;
        let page = b.page().await;
        page.goto(&format!("{}/bounce.html", site.site))
            .await
            .unwrap();
        for i in 0..40 {
            let world = if i % 2 == 0 {
                World::Isolated
            } else {
                World::Main
            };
            page.eval::<String>("() => location.search", world)
                .await
                .unwrap_or_else(|e| panic!("eval {i}: {e}"));
            page.css("#plain")
                .count()
                .await
                .unwrap_or_else(|e| panic!("count {i}: {e}"));
        }
        page.css("#plain")
            .click(Duration::from_secs(10))
            .await
            .unwrap();
        // The chain ends at n=15; a click there sticks.
        while page
            .eval::<String>("() => location.search", World::Isolated)
            .await
            .unwrap()
            != "?n=15"
        {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        page.css("#plain")
            .click(Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(page.title().await, "clicked 15");
    }

    /// D13 regression: the page sees no CDP client (no Runtime.enable, no driver
    /// frames on its stacks). Fails if the vendored chromiumoxide loses its patch.
    #[tokio::test]
    async fn detect_page_sees_no_client() {
        let Some(b) = TestBrowser::launch().await else {
            return;
        };
        let site = FixtureServer::start().await;
        let page = b.page().await;
        page.goto(&format!("{}/detect.html", site.site))
            .await
            .unwrap();
        let mut title = String::new();
        for _ in 0..50 {
            title = page.title().await;
            if title.starts_with('{') {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let v: serde_json::Value = serde_json::from_str(&title).expect("the probe reported");
        assert_eq!(v["prepareStackTraceOnLog"], 0, "{title}");
        assert_eq!(v["stackReads"], 0, "{title}");
        assert_eq!(v["stackSources"], serde_json::json!([]), "{title}");
        assert_eq!(v["automationGlobals"], serde_json::json!([]), "{title}");
    }

    /// A page's service worker answers right away: the client must not leave it
    /// paused (auto-attach waits for the debugger on start). With no client the
    /// engine answers in ~40 ms.
    #[tokio::test]
    async fn service_worker_starts_promptly() {
        let Some(b) = TestBrowser::launch().await else {
            return;
        };
        let routes = axum::Router::new()
            .route(
                "/sw.html",
                get(|| async { ([(header::CONTENT_TYPE, "text/html")], SW_PAGE) }),
            )
            .route(
                "/sw.js",
                get(|| async { ([(header::CONTENT_TYPE, "text/javascript")], SW_JS) }),
            );
        let site = FixtureServer::start_with(routes).await;
        let page = b.page().await;
        page.goto(&format!("{}/sw.html", site.site)).await.unwrap();
        let mut title = String::new();
        for _ in 0..80 {
            title = page.title().await;
            if title.starts_with("ms:") {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let ms: u64 = title.trim_start_matches("ms:").parse().unwrap_or(u64::MAX);
        assert!(ms < 2000, "service worker answered after {title}");
    }
}
