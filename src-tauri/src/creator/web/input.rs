//! Clicks and typing. Every pointer move and key goes through the engine's
//! `Motion` domain (human trajectories made inside the browser, nothing
//! injected); the actionability checks run in the isolated world.

use std::time::{Duration, Instant};

use chromiumoxide::cdp::browser_protocol::dom::ScrollIntoViewIfNeededParams;
use rand::Rng;
use regex::Regex;
use serde::Deserialize;
use serde_json::json;
use shardx::motion::Motion;

use super::locator::{Locator, Name, Step};
use super::{cdp, Page, Rect, WebError, WebResult, World};

/// Python `overlays.HANDLER_CLICK_MS`: an overlay's own click is short.
pub const HANDLER_CLICK: Duration = Duration::from_secs(2);
const POLL: Duration = Duration::from_millis(100);
/// Python `overlays.COOKIE_ACCEPT_RE` / `COOKIE_ACCEPT_SEL`.
pub const COOKIE_ACCEPT_RE: &str = r"(?i)^\s*(accept|allow) all( cookies)?\s*$";
pub const COOKIE_ACCEPT_SEL: &str = "#onetrust-accept-btn-handler, #CybotCookiebotDialogBodyLevelButtonLevelOptinAllowAll, #CybotCookiebotDialogBodyButtonAccept";

/// Per page (shared by clones): whether the pointer exists, and the overlay triggers.
#[derive(Default)]
pub(crate) struct InputState {
    pointer: tokio::sync::Mutex<bool>,
    /// Steps only: a stored Locator would hold its page and keep it alive.
    overlays: std::sync::Mutex<Vec<Vec<Step>>>,
}

/// Where the element stands, one frame apart (stable = same box both times).
const STATE_JS: &str = "async function () {
    if (!this.isConnected) return { s: 'gone' };
    const box = () => { const r = this.getBoundingClientRect(); return [r.x, r.y, r.width, r.height]; };
    const a = box();
    await new Promise((done) => { requestAnimationFrame(() => done()); setTimeout(done, 100); });
    const b = box();
    if (!(b[2] > 0 && b[3] > 0 && this.checkVisibility({ visibilityProperty: true, opacityProperty: false })))
        return { s: 'hidden' };
    if (this.matches(':disabled') || this.closest('[aria-disabled=true]')) return { s: 'disabled' };
    if (a.some((v, i) => v !== b[i])) return { s: 'moving' };
    return { s: 'ok', x: b[0], y: b[1], width: b[2], height: b[3], vw: innerWidth, vh: innerHeight };
}";

/// The topmost element at the point is this one or inside it.
const HIT_JS: &str = "function (x, y) {
    const root = this.getRootNode();
    const hit = (root.elementFromPoint ? root : document).elementFromPoint(x, y);
    return !!hit && (hit === this || this.contains(hit));
}";

#[derive(Deserialize)]
struct State {
    s: String,
    #[serde(flatten)]
    rect: Option<Rect>,
    vw: Option<f64>,
    vh: Option<f64>,
}

/// A point uniformly inside the middle 60 % of the box.
fn aim(r: &Rect) -> (f64, f64) {
    let mut rng = rand::thread_rng();
    (
        r.x + r.width * rng.gen_range(0.2..0.8),
        r.y + r.height * rng.gen_range(0.2..0.8),
    )
}

impl Page {
    fn motion(&self) -> Motion<'_> {
        Motion::new(&self.inner)
    }

    /// Before the first move: the pointer at a random point of the viewport (no seed:
    /// the profile's own motor signature).
    async fn pointer(&self) -> WebResult<()> {
        let mut made = self.input.pointer.lock().await;
        if !*made {
            let (w, h): (f64, f64) = self
                .eval_raw("() => [innerWidth, innerHeight]", World::Isolated)
                .await?;
            let mut rng = rand::thread_rng();
            let (x, y) = (
                rng.gen_range(0.0..w.max(1.0)),
                rng.gen_range(0.0..h.max(1.0)),
            );
            self.motion().create_pointer(x, y).await.map_err(cdp)?;
            *made = true;
        }
        Ok(())
    }

    async fn press_at(&self, x: f64, y: f64, width: f64) -> WebResult<()> {
        self.pointer().await?;
        let m = self.motion();
        m.glide_to(x, y, Some(width)).await.map_err(cdp)?;
        m.tap("left", 1).await.map_err(cdp)?;
        Ok(())
    }

    /// One key ("Enter", "Tab") on whatever has focus.
    pub async fn press(&self, key: &str) -> WebResult<()> {
        self.watch
            .act(async {
                self.motion()
                    .press_key(key, &[])
                    .await
                    .map(|_| ())
                    .map_err(cdp)
            })
            .await
    }

    /// Before every click/fill, a visible trigger is clicked once (Python
    /// `add_locator_handler`, e.g. the cookie banner).
    pub fn add_overlay(&self, trigger: Locator) {
        self.input.overlays.lock().unwrap().push(trigger.steps());
    }

    async fn clear_overlays(&self) {
        let triggers: Vec<Vec<Step>> = self.input.overlays.lock().unwrap().clone();
        for steps in triggers {
            let t = Locator::with_steps(self, steps);
            if let Ok(Some(btn)) = t.first_visible().await {
                // Boxed: click_on clears overlays too (not this one: `false`).
                let _ = Box::pin(btn.click_on(HANDLER_CLICK, false)).await;
            }
        }
    }
}

impl Locator {
    /// Wait until the first match is visible, enabled, stable and the topmost
    /// element at the aimed point, then glide there and press. Overlays are
    /// cleared first and again whenever something covers the element.
    /// Past `timeout`: `Covered` when something stayed on top of it.
    pub async fn click(&self, timeout: Duration) -> WebResult<()> {
        self.click_on(timeout, true).await
    }

    async fn click_on(&self, timeout: Duration, overlays: bool) -> WebResult<()> {
        let page = self.page.clone();
        let work = async {
            if overlays {
                page.clear_overlays().await;
            }
            let end = Instant::now() + timeout;
            let mut why;
            loop {
                match self.try_click().await {
                    Ok(()) => return Ok(()),
                    Err(WebError::Covered) if overlays => {
                        why = WebError::Covered;
                        page.clear_overlays().await;
                    }
                    // A navigation mid-attempt: try again on the new document.
                    Err(e) if super::stale(&e) => why = e,
                    Err(e @ (WebError::Hung | WebError::Cdp(_))) => return Err(e),
                    Err(e) => why = e,
                }
                if Instant::now() >= end {
                    return Err(why);
                }
                tokio::time::sleep(POLL).await;
            }
        };
        // The click's own wait plus the act deadline: a long timeout is not a hang.
        page.watch.act_within(timeout, work).await
    }

    /// One attempt; the error says why it could not press yet.
    async fn try_click(&self) -> WebResult<()> {
        let target = self
            .with(async |h, nodes| {
                let Some(n) = nodes.into_iter().next() else {
                    return Err(WebError::NotFound);
                };
                let st: State = h.value(&n, STATE_JS, &[]).await?;
                match st.s.as_str() {
                    "ok" => {}
                    "disabled" => return Err(WebError::Disabled),
                    "moving" => return Err(WebError::Timeout("element is moving")),
                    "gone" => return Err(WebError::NotFound),
                    _ => return Err(WebError::Timeout("element is not visible")),
                }
                let r = st.rect.ok_or(WebError::NotFound)?;
                let (x, y) = aim(&r);
                if x < 0.0 || y < 0.0 || x > st.vw.unwrap_or(0.0) || y > st.vh.unwrap_or(0.0) {
                    let p = ScrollIntoViewIfNeededParams {
                        object_id: Some(n.clone()),
                        ..Default::default()
                    };
                    h.page.inner.execute(p).await.map_err(cdp)?;
                    return Err(WebError::Timeout("element was scrolled into view"));
                }
                if !h.value::<bool>(&n, HIT_JS, &[json!(x), json!(y)]).await? {
                    return Err(WebError::Covered);
                }
                Ok((x, y, r.width))
            })
            .await?;
        let (x, y, width) = target;
        let page = &self.page;
        page.pointer().await?;
        page.motion()
            .glide_to(x, y, Some(width))
            .await
            .map_err(cdp)?;
        // Something may have come over it during the glide (a banner at 300 ms).
        let still = self
            .with(async |h, nodes| match nodes.first() {
                Some(n) => h.value::<bool>(n, HIT_JS, &[json!(x), json!(y)]).await,
                None => Err(WebError::NotFound),
            })
            .await?;
        if !still {
            return Err(WebError::Covered);
        }
        page.motion().tap("left", 1).await.map_err(cdp)?;
        Ok(())
    }

    /// Glide to the first match's box and press, whatever is on top (Python
    /// `mouse.click_box`, the covered-click fallback for overlays).
    pub async fn press_box(&self) -> WebResult<()> {
        let page = self.page.clone();
        page.watch
            .act(async {
                let r = self.bounding_box().await?.ok_or(WebError::NotFound)?;
                let (x, y) = aim(&r);
                page.press_at(x, y, r.width).await
            })
            .await
    }

    /// Click it, select all, delete, type `value` key by key.
    pub async fn fill(&self, value: &str, timeout: Duration) -> WebResult<()> {
        self.click(timeout).await?;
        let page = self.page.clone();
        page.watch
            .act(async {
                // The host's shortcut, not the fingerprint's platform: the keys go
                // to the real OS widget.
                let m = page.motion();
                let all = if cfg!(target_os = "macos") {
                    "Meta"
                } else {
                    "Control"
                };
                m.press_key("a", &[all]).await.map_err(cdp)?;
                m.press_key("Backspace", &[]).await.map_err(cdp)?;
                m.enter_text(value, false).await.map_err(cdp)?;
                Ok(())
            })
            .await
    }

    /// An in-page `el.click()` (untrusted). Only where Python does exactly that:
    /// a covered submit button (`tts_base._submit`).
    pub async fn js_click(&self) -> WebResult<()> {
        self.evaluate::<serde_json::Value>("(el) => el.click()")
            .await
            .map(|_| ())
    }
}

/// Python `overlays.cookie_accept`: the accept-all button by its English name, or
/// by a cookie vendor's own button id.
pub fn cookie_accept(page: &Page) -> [Locator; 2] {
    [
        page.role("button", Name::Re(Regex::new(COOKIE_ACCEPT_RE).unwrap())),
        page.css(COOKIE_ACCEPT_SEL),
    ]
}

/// One poll-loop check: press a visible cookie "Accept all" (covered -> a press
/// on its box). Never fails. (Python `accept_cookies` minus OneTrust.)
pub async fn accept_cookies(page: &Page) -> bool {
    for loc in cookie_accept(page) {
        let Ok(Some(btn)) = page.watch.call(loc.first_visible(), Ok(None)).await else {
            continue;
        };
        return match btn.click_on(HANDLER_CLICK, false).await {
            Ok(()) => true,
            Err(WebError::Covered | WebError::Timeout(_)) => btn.press_box().await.is_ok(),
            Err(_) => false,
        };
    }
    false
}

#[cfg(test)]
mod tests {
    use super::super::testkit::{FixtureServer, TestBrowser};
    use super::*;

    async fn act() -> Option<(TestBrowser, FixtureServer, Page)> {
        let b = TestBrowser::launch().await?;
        let site = FixtureServer::start().await;
        let page = b.page().await;
        page.goto(&format!("{}/act.html", site.site)).await.unwrap();
        Some((b, site, page))
    }

    #[derive(Deserialize, Debug)]
    struct Click {
        id: String,
        trusted: bool,
        moves: u32,
    }

    async fn clicks(page: &Page) -> Vec<Click> {
        page.eval("() => window.clicks", World::Main).await.unwrap()
    }

    #[test]
    fn aim_stays_in_the_middle() {
        let r = Rect {
            x: 10.0,
            y: 20.0,
            width: 100.0,
            height: 50.0,
        };
        for _ in 0..200 {
            let (x, y) = aim(&r);
            assert!((30.0..=90.0).contains(&x) && (30.0..=60.0).contains(&y));
        }
    }

    #[tokio::test]
    async fn click_is_trusted_and_moved() {
        let Some((_b, _s, page)) = act().await else {
            return;
        };
        page.css("#plain")
            .click(Duration::from_secs(5))
            .await
            .unwrap();
        let c = clicks(&page).await;
        assert_eq!(c.len(), 1, "{c:?}");
        assert_eq!(c[0].id, "plain");
        assert!(c[0].trusted);
        assert!(c[0].moves >= 5, "{c:?}");
    }

    #[tokio::test]
    async fn click_waits_for_enabled_and_stable() {
        let Some((_b, _s, page)) = act().await else {
            return;
        };
        page.css("#late")
            .click(Duration::from_secs(5))
            .await
            .unwrap();
        page.css("#moving")
            .click(Duration::from_secs(5))
            .await
            .unwrap();
        let ids: Vec<String> = clicks(&page).await.into_iter().map(|c| c.id).collect();
        assert_eq!(ids, ["late", "moving"]);
        let r = page.css("#never").click(Duration::from_millis(500)).await;
        assert_eq!(r, Err(WebError::Disabled));
        let r = page.css("#nothing").click(Duration::from_millis(300)).await;
        assert_eq!(r, Err(WebError::NotFound));
    }

    #[tokio::test]
    async fn covered_click_times_out_as_covered() {
        let Some((_b, _s, page)) = act().await else {
            return;
        };
        page.eval::<()>("() => window.cover()", World::Main)
            .await
            .unwrap();
        let r = page.css("#plain").click(Duration::from_secs(1)).await;
        assert_eq!(r, Err(WebError::Covered));
        assert!(clicks(&page).await.is_empty());
    }

    #[tokio::test]
    async fn js_click_reaches_covered_button() {
        let Some((_b, _s, page)) = act().await else {
            return;
        };
        page.eval::<()>("() => window.cover()", World::Main)
            .await
            .unwrap();
        page.css("#plain").js_click().await.unwrap();
        let c = clicks(&page).await;
        assert_eq!(c.len(), 1);
        assert!(!c[0].trusted);
    }

    #[tokio::test]
    async fn fill_replaces_existing_value() {
        let Some((_b, _s, page)) = act().await else {
            return;
        };
        let input = page.css("input[name=email]");
        assert_eq!(input.input_value().await.unwrap(), "old");
        input
            .fill("new@x.io", Duration::from_secs(5))
            .await
            .unwrap();
        let typed = input.input_value().await.unwrap();
        assert_eq!(typed, "new@x.io", "typed {} chars", typed.len());
        page.press("Enter").await.unwrap();
        let submitted: String = page
            .eval("() => window.submitted", World::Main)
            .await
            .unwrap();
        assert_eq!(submitted, "new@x.io");
    }

    #[tokio::test]
    async fn overlay_hook_clears_banner_before_click() {
        let Some((_b, _s, page)) = act().await else {
            return;
        };
        for t in cookie_accept(&page) {
            page.add_overlay(t);
        }
        // Up before the click, and coming up during the glide.
        for wait in [500, 0] {
            page.eval::<()>(
                "() => { window.clicks = []; window.banner(300); }",
                World::Main,
            )
            .await
            .unwrap();
            tokio::time::sleep(Duration::from_millis(wait)).await;
            page.css("#plain")
                .click(Duration::from_secs(8))
                .await
                .unwrap();
            let ids: Vec<String> = clicks(&page).await.into_iter().map(|c| c.id).collect();
            assert_eq!(ids, ["accept", "plain"], "banner after {wait} ms");
            assert!(!page.css("#banner").is_visible().await);
        }
    }

    #[tokio::test]
    async fn accept_cookies_presses_the_banner() {
        let Some((_b, _s, page)) = act().await else {
            return;
        };
        assert!(!accept_cookies(&page).await);
        page.eval::<()>("() => window.banner(0)", World::Main)
            .await
            .unwrap();
        assert!(accept_cookies(&page).await);
        assert!(!page.css("#banner").is_visible().await);
    }
}
