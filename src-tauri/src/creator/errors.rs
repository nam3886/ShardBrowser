//! Fixed error codes for failed accounts (Python `engine/errors.py`
//! `classify`), the subset a Breeze Blue email run can produce. Exact texts
//! first, then the rules in Python's order; a text no rule names is `OTHER`.

use std::sync::LazyLock;

use regex::Regex;

use super::profile::PROFILE_GONE;
use super::providers::breezeblue as bb;
use super::providers::tts;
use super::proxy::{PROXY_DELETED, PROXY_UNUSABLE};
use super::runner::{INTERRUPTED, NO_ADDRESS, NO_BROWSER, NO_PROXY};
use super::shard::GEO_FAILED;
use super::web::BROWSER_HUNG;

/// Python `ele.MAILBOX_GONE`.
const MAILBOX_GONE: &str = "mailbox gone";

const EXACT: &[(&str, &str)] = &[
    (INTERRUPTED, "INTERRUPTED"),
    ("cancelled", "INTERRUPTED"),
    (PROXY_UNUSABLE, "PROXY_UNUSABLE"),
    (PROXY_DELETED, "PROXY_UNUSABLE"),
    (NO_PROXY, "NO_PROXY"),
    ("No healthy proxy", "NO_PROXY"),
    (bb::CAPTCHA_UNSOLVED, "CAPTCHA_UNSOLVED"),
    (MAILBOX_GONE, "MAILBOX_GONE"),
    (bb::NO_VERIFY_MAIL, "NO_MAIL"),
    (
        "Breeze Blue sign-up did not go through",
        "SIGNUP_NOT_COMPLETED",
    ),
    (bb::NO_SIGNIN, "NOT_SIGNED_IN"),
    (bb::EMAIL_EXISTS, "ACCOUNT_EXISTS"),
    (BROWSER_HUNG, "BROWSER_HUNG"),
    (NO_BROWSER, "NO_BROWSER"),
    (NO_ADDRESS, "NO_BROWSER"),
    ("signup failed", "SIGNUP_FAILED"),
];

enum Rule {
    Prefix(&'static str),
    Contains(&'static str),
    Re(&'static LazyLock<Regex>),
}

static GONE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)^mailbox gone").unwrap());
/// A step name, then an error kind: "sign up by email: TimeoutError".
static STEP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:[a-z][\w .-]*|[A-Z]\w* [\w .-]*): [A-Z]\w*$").unwrap());
static CLASS_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^[A-Z]\w*: ").unwrap());

/// Python `RULES`, in its order (only the ones a Breeze run can hit).
const RULES: &[(Rule, &str)] = &[
    (Rule::Prefix("window closed during "), "WINDOW_CLOSED"),
    (Rule::Prefix("page load: "), "PAGE_LOAD_FAILED"),
    (
        Rule::Contains("ShardBrowser refused the proxy"),
        "PROXY_CHECK_FAILED",
    ),
    (Rule::Contains(GEO_FAILED), "PROXY_CHECK_FAILED"),
    (Rule::Contains("is already running"), "PROFILE_RUNNING"),
    (
        Rule::Contains("still opening or being saved"),
        "PROFILE_RUNNING",
    ),
    (Rule::Contains(PROFILE_GONE), "NO_BROWSER"),
    (Rule::Re(&GONE_RE), "MAILBOX_GONE"),
    (Rule::Contains("Ele API unavailable"), "MAIL_API_DOWN"),
    (Rule::Prefix(tts::NO_KEY), "API_KEY_FAILED"),
    (Rule::Prefix(tts::KEY_TEST_FAILED), "API_KEY_FAILED"),
    // Shard's launch refusals (`launch.rs`, `runtime.rs`) and our CDP wait
    // (`shard::open_cdp`; Python's `CDP_NOT_READY` text is in this group too).
    (Rule::Contains("engine not installed yet"), "ENGINE_UNAVAILABLE"),
    (Rule::Contains("engine is being updated"), "ENGINE_UNAVAILABLE"),
    (Rule::Contains("moving or resetting its data"), "ENGINE_UNAVAILABLE"),
    (Rule::Contains("did not open with CDP in time"), "ENGINE_UNAVAILABLE"),
    (Rule::Prefix("run: "), "APP_ERROR"),
    (Rule::Prefix("TimeoutError: "), "STEP_FAILED"),
    (Rule::Re(&STEP_RE), "STEP_FAILED"),
    (Rule::Re(&CLASS_RE), "APP_ERROR"),
];

/// The code for a stored error text; None for no text, `OTHER` when no rule matches.
pub fn error_code(err: &str) -> Option<&'static str> {
    if err.is_empty() {
        return None;
    }
    if let Some((_, code)) = EXACT.iter().find(|(t, _)| *t == err) {
        return Some(code);
    }
    let hit = |r: &Rule| match r {
        Rule::Prefix(p) => err.starts_with(p),
        Rule::Contains(p) => err.contains(p),
        Rule::Re(re) => re.is_match(err),
    };
    Some(
        RULES
            .iter()
            .find(|(r, _)| hit(r))
            .map_or("OTHER", |(_, c)| c),
    )
}

/// Python `Code.streak`: whether a failure with this code counts toward a
/// queue's fail streak (M5). Not the site's fault, or nothing a retry fixes:
/// no.
pub fn counts_toward_streak(code: &str) -> bool {
    !matches!(
        code,
        "INTERRUPTED"
            | "PROXY_UNUSABLE"
            | "NO_PROXY"
            | "MAILBOX_GONE"
            | "ACCOUNT_EXISTS"
            | "PROFILE_RUNNING"
            | "NO_BROWSER"
            | "ENGINE_UNAVAILABLE"
    )
}

#[cfg(test)]
mod tests {
    use super::{counts_toward_streak, error_code};

    #[test]
    fn engine_and_owner_causes_never_count_toward_the_streak() {
        for code in ["ENGINE_UNAVAILABLE", "INTERRUPTED", "NO_PROXY", "ACCOUNT_EXISTS"] {
            assert!(!counts_toward_streak(code), "{code}");
        }
        for code in ["CAPTCHA_UNSOLVED", "NO_MAIL", "OTHER"] {
            assert!(counts_toward_streak(code), "{code}");
        }
    }

    #[test]
    fn breeze_texts_get_pythons_codes() {
        for (text, code) in [
            ("captcha not solved", "CAPTCHA_UNSOLVED"),
            ("no Breeze Blue verify mail", "NO_MAIL"),
            ("Breeze Blue did not sign in", "NOT_SIGNED_IN"),
            (
                "A Breeze Blue account already exists for this email",
                "ACCOUNT_EXISTS",
            ),
            (
                "Breeze Blue sign-up did not go through",
                "SIGNUP_NOT_COMPLETED",
            ),
            ("No API key shown", "API_KEY_FAILED"),
            ("No API key shown: TimeoutError", "API_KEY_FAILED"),
            ("API key test failed: HTTP 500", "API_KEY_FAILED"),
            (
                "page load: net::ERR_PROXY_CONNECTION_FAILED",
                "PAGE_LOAD_FAILED",
            ),
            ("sign up by email: TimeoutError", "STEP_FAILED"),
            ("run: CdpError", "APP_ERROR"),
            ("window closed during sign-up", "WINDOW_CLOSED"),
            ("Browser stopped responding", "BROWSER_HUNG"),
            ("proxy location lookup failed", "PROXY_CHECK_FAILED"),
            ("ShardBrowser profile is gone", "NO_BROWSER"),
            ("mail: Ele API unavailable: inbox HTTP 500", "MAIL_API_DOWN"),
            ("Interrupted", "INTERRUPTED"),
            (
                "Browser proxy unusable (off, failing or resting): change its proxy",
                "PROXY_UNUSABLE",
            ),
            ("proxy deleted", "PROXY_UNUSABLE"),
            ("No healthy proxy for a new browser", "NO_PROXY"),
            ("profile x is already running", "PROFILE_RUNNING"),
            (
                "ShardBrowser browser engine not installed yet — open Settings to download",
                "ENGINE_UNAVAILABLE",
            ),
            (
                "The ShardBrowser engine is being updated. Try again in a moment.",
                "ENGINE_UNAVAILABLE",
            ),
            (
                "ShardBrowser is moving or resetting its data. Try again when that finishes.",
                "ENGINE_UNAVAILABLE",
            ),
            ("profile abc did not open with CDP in time", "ENGINE_UNAVAILABLE"),
            ("Breeze Blue sign-in form did not show", "OTHER"),
        ] {
            assert_eq!(error_code(text), Some(code), "{text}");
        }
        assert_eq!(error_code(""), None);
    }
}
