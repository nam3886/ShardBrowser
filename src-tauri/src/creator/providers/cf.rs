//! Cloudflare Turnstile inside a form (Python `browser/cf.py` and the in-form half
//! of `chatgpt.cf_press`): the widget's `<iframe>` is found through the frame tree
//! (a closed shadow root hides it from selectors), its own response field says
//! whether it passed, and a pending one gets an in-page press at its checkbox.

use std::time::{Duration, Instant};

use rand::Rng;

use crate::creator::web::locator::Locator;
use crate::creator::web::{Page, Rect, World};

pub const CF_HOST: &str = "challenges.cloudflare.com";
/// Height 0 = not drawn yet: a press measured then lands too high.
const CF_DRAWN_H: f64 = 20.0;
/// Narrower than this, the press at x+22..34 would miss the widget.
const CF_MIN_W: f64 = 40.0;
/// Rest on the box before pressing (uniform, s).
const CF_DWELL: (f64, f64) = (0.3, 0.8);
const CF_POLL: Duration = Duration::from_millis(500);
/// Python `cf.QUICK_DRAW_S` / `QUICK_WAIT_S`: the quick round's draw wait before
/// its one press, and its verify wait after it.
pub const QUICK_DRAW: Duration = Duration::from_secs(3);
pub const QUICK_WAIT: Duration = Duration::from_secs(4);

/// On the Cloudflare `<iframe>`: its widget's own response field (in the nearest
/// ancestor that has one, a few levels up from the iframe or its shadow host)
/// holds a token. Only true/false comes back.
const TOKEN_JS: &str = "(f) => { const r = f.getRootNode(); let n = r instanceof ShadowRoot ? r.host : f;
    for (let i = 0; n && i < 4; i++, n = n.parentElement) {
        const t = n.querySelector(\"input[name='cf-turnstile-response']\"); if (t) return !!t.value; }
    return false; }";

/// The first visible Cloudflare `<iframe>` (Python `turnstile_frame`).
async fn turnstile_frame(page: &Page, host: &str) -> Option<Locator> {
    let w = page.watch();
    for el in w.call(page.frame_owners(host), Ok(vec![])).await.ok()? {
        if w.call(el.is_visible(), false).await {
            return Some(el);
        }
    }
    None
}

/// A visible Cloudflare widget whose own response field is still empty. Only a
/// yes/no comes back, never the token.
pub async fn turnstile_pending(page: &Page, host: &str) -> bool {
    match turnstile_frame(page, host).await {
        Some(el) => !page
            .watch()
            .call(el.evaluate::<bool>(TOKEN_JS), Ok(true))
            .await
            .unwrap_or(true),
        None => false,
    }
}

/// The widget's box once drawn, visible, and with the press zone inside the
/// viewport (Python `cf_widget_box`).
pub async fn widget_box(page: &Page, host: &str) -> Option<Rect> {
    let w = page.watch();
    let owners = w.call(page.frame_owners(host), Ok(vec![])).await.ok()?;
    let vp = page
        .eval::<(f64, f64)>("() => [innerWidth, innerHeight]", World::Isolated)
        .await
        .ok();
    for el in owners {
        if !w.call(el.is_visible(), false).await {
            continue;
        }
        let Ok(Some(b)) = w.call(el.bounding_box(), Ok(None)).await else {
            continue;
        };
        if b.height <= CF_DRAWN_H || b.width < CF_MIN_W {
            continue;
        }
        let mid = b.y + b.height / 2.0;
        if let Some((vw, vh)) = vp {
            if !(0.0 <= b.x && b.x + CF_MIN_W <= vw && 4.0 <= mid && mid < vh - 4.0) {
                continue; // a press off the viewport lands on nothing
            }
        }
        return Some(b);
    }
    None
}

/// One quick in-page press on a pending in-form widget (Python `cf_press(quick=True)`
/// for a form widget): waits up to `draw` for the box, presses its checkbox at
/// `x + U(22,34)`, `y + h/2 + U(-4,4)`, then up to `wait` for it to clear.
/// A widget whose token is already set is never pressed. -> (cleared, presses).
pub async fn press_turnstile(
    page: &Page,
    host: &str,
    draw: Duration,
    wait: Duration,
    closed: &(dyn Fn() -> bool + Sync),
) -> (bool, u32) {
    if !turnstile_pending(page, host).await {
        return (false, 0);
    }
    let end = Instant::now() + draw;
    let b = loop {
        if let Some(b) = widget_box(page, host).await {
            break b;
        }
        if closed() || Instant::now() >= end {
            return (false, 0);
        }
        if !turnstile_pending(page, host).await {
            return (true, 0);
        }
        tokio::time::sleep(CF_POLL).await;
    };
    let (x, y, dwell) = {
        let mut rng = rand::thread_rng();
        (
            b.x + rng.gen_range(22.0..34.0),
            b.y + b.height / 2.0 + rng.gen_range(-4.0..4.0),
            Duration::from_secs_f64(rng.gen_range(CF_DWELL.0..CF_DWELL.1)),
        )
    };
    // Navigated away mid-press, or hung: the check below tells.
    let _ = page.watch().act(page.press_at(x, y, b.width, dwell)).await;
    let end = Instant::now() + wait;
    while Instant::now() < end {
        if closed() {
            return (false, 1);
        }
        if !turnstile_pending(page, host).await {
            return (true, 1);
        }
        tokio::time::sleep(CF_POLL).await;
    }
    (false, 1)
}
