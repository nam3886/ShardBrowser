//! Python `tests/test_breezeblue.py`, case by case, against the fake site in a
//! real browser (`testsite`); the mailbox is a fake `Inbox`.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures_util::future::BoxFuture;

use super::*;
use crate::creator::providers::tts::{Inbox, KeyState, TtsRun};
use crate::creator::testsite::{Knobs, Noop, Recorder, Site, Survey, Turnstile};
use crate::creator::web::testkit::TestBrowser;
use crate::creator::web::Page;

const EMAIL: &str = "bo@ex.io";
const NEW_PW: &str = "Gq7!rTz2";

struct FakeInbox {
    page: Page,
    rec: Recorder,
    link: Option<String>,
    reuse_err: Option<String>,
    /// The page re-renders (inputs emptied) while the mailbox is reused.
    wipe: bool,
    /// The form's own state resets while the inputs still show the values.
    reset_store: bool,
    waited: Mutex<Vec<(String, Vec<String>)>>,
    filled_at_reuse: Mutex<Option<bool>>,
}

impl Inbox for FakeInbox {
    fn reuse(&self) -> BoxFuture<'_, Result<(), String>> {
        Box::pin(async move {
            if let Some(e) = &self.reuse_err {
                return Err(e.clone());
            }
            let js = "() => [...document.querySelectorAll('input[type=email], input[type=password]')].every(i => i.value)";
            let filled = self.page.eval::<bool>(js, World::Isolated).await.ok();
            *self.filled_at_reuse.lock().unwrap() = filled;
            if self.wipe {
                let js =
                    "() => { for (const i of document.querySelectorAll('input')) i.value = ''; }";
                self.page.eval::<()>(js, World::Isolated).await.unwrap();
            }
            if self.reset_store {
                let js = "() => { store.email = ''; store.password = ''; }";
                self.page.eval::<()>(js, World::Main).await.unwrap();
            }
            self.rec.event("reuse");
            Ok(())
        })
    }

    fn wait_link<'b>(
        &'b self,
        sender_has: &'b str,
        accept: &'b [&'b str],
        _timeout: Duration,
        _stop: &'b (dyn Fn() -> bool + Send + Sync),
    ) -> BoxFuture<'b, Result<Option<String>, String>> {
        Box::pin(async move {
            let accept = accept.iter().map(|s| s.to_string()).collect();
            self.waited
                .lock()
                .unwrap()
                .push((sender_has.to_string(), accept));
            Ok(self.link.clone())
        })
    }
}

struct Env {
    _b: TestBrowser,
    page: Page,
    site: Site,
    inbox: FakeInbox,
}

impl Env {
    async fn new(k: Knobs) -> Option<Env> {
        let b = TestBrowser::launch().await?;
        let page = b.page().await;
        let site = Site::start(k).await;
        let inbox = FakeInbox {
            page: page.clone(),
            rec: site.recorder(),
            link: Some(site.link.clone()),
            reuse_err: None,
            wipe: false,
            reset_store: false,
            waited: Mutex::default(),
            filled_at_reuse: Mutex::default(),
        };
        Some(Env {
            _b: b,
            page,
            site,
            inbox,
        })
    }

    fn mailed(&self) -> bool {
        !self.inbox.waited.lock().unwrap().is_empty()
    }

    fn key(&self) -> Option<String> {
        self.site.seen(|s| s.key.clone())
    }
}

struct Opts {
    password: &'static str,
    reused: bool,
    tm: Timings,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            password: NEW_PW,
            reused: false,
            tm: Timings::fast(),
        }
    }
}

#[derive(Debug)]
struct Out {
    res: Option<&'static str>,
    error: Option<String>,
    api_key: Option<String>,
    untested: Option<String>,
    logs: Vec<String>,
    steps: Vec<String>,
}

async fn signup(e: &Env, o: Opts) -> Out {
    let logs = Arc::new(Mutex::new(vec![]));
    let steps = Arc::new(Mutex::new(vec![]));
    let mut t = TtsRun::new(&e.page, KEY, EMAIL.into());
    t.password = Some(o.password.into());
    t.password_reused = o.reused;
    t.mail = Some(&e.inbox);
    let l = logs.clone();
    t.log = Box::new(move |_, m| l.lock().unwrap().push(m.to_string()));
    let s = steps.clone();
    t.on_step = Box::new(move |n| s.lock().unwrap().push(n.to_string()));
    let res = run(&mut t, &e.site.urls, &o.tm, None).await;
    let out = Out {
        res,
        error: t.error.clone(),
        api_key: t.api_key.clone(),
        untested: t.untested_key.clone(),
        logs: logs.lock().unwrap().clone(),
        steps: steps.lock().unwrap().clone(),
    };
    out
}

macro_rules! env {
    ($k:expr) => {
        match Env::new($k).await {
            Some(e) => e,
            None => return,
        }
    };
}

fn linked(o: &Out) {
    assert_eq!(o.res, Some("linked"), "{:?} {:?}", o.error, o.logs);
}

#[tokio::test]
async fn email_happy_path() {
    let e = env!(Knobs::default());
    let o = signup(&e, Opts::default()).await;
    linked(&o);
    let ev = e.site.seen(|s| s.events.clone());
    let at = |x: &str| ev.iter().position(|e| e == x).unwrap();
    assert_eq!(
        ev.first().map(String::as_str),
        Some("visit login"),
        "{ev:?}"
    );
    // Filled before the mailbox reuse, reused once, before Continue.
    assert_eq!(*e.inbox.filled_at_reuse.lock().unwrap(), Some(true));
    assert!(at("reuse") < at("click Continue"), "{ev:?}");
    assert_eq!(e.site.count("reuse"), 1);
    assert_eq!(
        e.site.seen(|s| s.typed_password.clone()).as_deref(),
        Some(NEW_PW)
    );
    // The verify mail: Breeze Blue's sender, a link with "verif".
    assert_eq!(
        *e.inbox.waited.lock().unwrap(),
        vec![("breezeblue".to_string(), vec!["verif".to_string()])]
    );
    assert_eq!(e.site.count("click Skip question"), 2);
    assert_eq!(e.site.count("click Start creating"), 1);
    assert_eq!(
        e.site.seen(|s| s.key_name.clone()).as_deref(),
        Some("default")
    );
    let key = e.key().unwrap();
    assert_eq!(e.site.seen(|s| s.balance_keys.clone()), vec![key.clone()]);
    assert_eq!(o.api_key, Some(key));
    assert_eq!(e.page.url().await, "about:blank");
    assert_eq!(e.site.count("click Done"), 1);
    assert_eq!(
        o.steps[o.steps.len() - 4..],
        ["signed in", "api key", "key test", "linked"]
    );
}

#[tokio::test]
async fn wrong_password_with_new_password_is_fatal() {
    let e = env!(Knobs {
        existing_password: Some("someone-else".into()),
        ..Knobs::default()
    });
    let o = signup(&e, Opts::default()).await;
    assert_eq!(o.res, None);
    assert_eq!(o.error.as_deref(), Some(EMAIL_EXISTS));
    assert!(!e.mailed());
}

#[tokio::test]
async fn wrong_password_with_reused_password_stays_retryable() {
    let e = env!(Knobs {
        existing_password: Some("someone-else".into()),
        ..Knobs::default()
    });
    let o = signup(
        &e,
        Opts {
            password: "saved-pw-1",
            reused: true,
            ..Opts::default()
        },
    )
    .await;
    assert_eq!(o.res, None);
    assert_eq!(o.error.as_deref(), Some(NO_SIGNIN));
}

#[tokio::test]
async fn existing_account_with_our_password_skips_mail() {
    let e = env!(Knobs {
        existing_password: Some("saved-pw-1".into()),
        ..Knobs::default()
    });
    let o = signup(
        &e,
        Opts {
            password: "saved-pw-1",
            reused: true,
            ..Opts::default()
        },
    )
    .await;
    linked(&o);
    assert!(!e.mailed());
    assert_eq!(o.api_key, e.key());
}

#[tokio::test]
async fn no_verify_mail() {
    let mut e = env!(Knobs::default());
    e.inbox.link = None;
    let o = signup(&e, Opts::default()).await;
    assert_eq!(o.res, None);
    assert_eq!(o.error.as_deref(), Some(NO_VERIFY_MAIL));
}

#[tokio::test]
async fn turnstile_try_again_once_then_passes() {
    let e = env!(Knobs {
        turnstile: Turnstile::FailOnce,
        ..Knobs::default()
    });
    linked(&signup(&e, Opts::default()).await);
    assert_eq!(e.site.count("click Try again"), 1);
}

#[tokio::test]
async fn turnstile_old_failure_text_lingering_after_try_again_is_not_a_second_failure() {
    let e = env!(Knobs {
        turnstile: Turnstile::Linger(2000),
        ..Knobs::default()
    });
    let mut tm = Timings::fast();
    tm.submit_wait = Duration::from_secs(5);
    linked(
        &signup(
            &e,
            Opts {
                tm,
                ..Opts::default()
            },
        )
        .await,
    );
    assert_eq!(e.site.count("click Try again"), 1);
}

#[tokio::test]
async fn turnstile_failing_twice_is_captcha_unsolved() {
    let e = env!(Knobs {
        turnstile: Turnstile::FailAlways,
        ..Knobs::default()
    });
    let o = signup(&e, Opts::default()).await;
    assert_eq!(o.error.as_deref(), Some(CAPTCHA_UNSOLVED));
    assert_eq!(e.site.count("click Try again"), 1);
    assert!(!e.mailed());
}

#[tokio::test]
async fn turnstile_that_never_clears_is_captcha_unsolved() {
    let e = env!(Knobs {
        turnstile: Turnstile::Never,
        ..Knobs::default()
    });
    let o = signup(&e, Opts::default()).await;
    assert_eq!(o.error.as_deref(), Some(CAPTCHA_UNSOLVED));
}

#[tokio::test]
async fn turnstile_waiting_for_a_click_is_ticked_in_page() {
    let e = env!(Knobs {
        turnstile: Turnstile::NeedsClick {
            presses: 1,
            delay_ms: 0
        },
        ..Knobs::default()
    });
    let mut tm = Timings::fast();
    tm.tick_grace = Duration::ZERO;
    linked(
        &signup(
            &e,
            Opts {
                tm,
                ..Opts::default()
            },
        )
        .await,
    );
    assert_eq!(e.site.count("cf press"), 1);
}

/// Live 2026-11-21: the 2nd tick landed ~3 s before the deadline and the widget
/// needed ~5 s more.
#[tokio::test]
async fn turnstile_ticked_near_the_deadline_gets_time_to_pass() {
    let e = env!(Knobs {
        turnstile: Turnstile::NeedsClick {
            presses: 2,
            delay_ms: 600
        },
        ..Knobs::default()
    });
    let mut tm = Timings::fast();
    // Past by the time the first tick's own wait ends.
    tm.submit_wait = Duration::from_secs(1);
    tm.tick_grace = Duration::ZERO;
    tm.tick_settle = Duration::from_secs(3);
    linked(
        &signup(
            &e,
            Opts {
                tm,
                ..Opts::default()
            },
        )
        .await,
    );
    assert_eq!(e.site.count("cf press"), 2);
}

#[tokio::test]
async fn turnstile_ticks_stop_at_ticks() {
    let e = env!(Knobs {
        turnstile: Turnstile::NeedsClick {
            presses: 3,
            delay_ms: 0
        },
        ..Knobs::default()
    });
    let mut tm = Timings::fast();
    tm.submit_wait = Duration::from_secs(1);
    tm.tick_grace = Duration::ZERO;
    let o = signup(
        &e,
        Opts {
            tm,
            ..Opts::default()
        },
    )
    .await;
    assert_eq!(o.error.as_deref(), Some(CAPTCHA_UNSOLVED));
    assert_eq!(e.site.count("cf press"), 2);
}

/// A passed widget stays drawn: never press it (live x.ai 2026-11-19).
#[tokio::test]
async fn turnstile_widget_that_passed_is_never_pressed() {
    let e = env!(Knobs {
        turnstile: Turnstile::Passed { delay_ms: 2500 },
        ..Knobs::default()
    });
    let mut tm = Timings::fast();
    tm.tick_grace = Duration::ZERO;
    linked(
        &signup(
            &e,
            Opts {
                tm,
                ..Opts::default()
            },
        )
        .await,
    );
    assert_eq!(e.site.count("cf press"), 0);
}

#[tokio::test]
async fn turnstile_that_passes_by_itself_is_never_pressed() {
    let e = env!(Knobs::default());
    let mut tm = Timings::fast();
    tm.tick_grace = Duration::ZERO;
    linked(
        &signup(
            &e,
            Opts {
                tm,
                ..Opts::default()
            },
        )
        .await,
    );
    assert_eq!(e.site.count("cf press"), 0);
}

#[tokio::test]
async fn continue_that_changes_nothing_fails() {
    let e = env!(Knobs {
        continue_noop: Noop::Always,
        ..Knobs::default()
    });
    let o = signup(&e, Opts::default()).await;
    assert_eq!(
        o.error.as_deref(),
        Some("Breeze Blue sign-up did not go through")
    );
    assert!(!e.mailed());
    // One more try on a fresh form first.
    assert_eq!(e.site.count("click Continue"), 2);
}

#[tokio::test]
async fn continue_that_resets_the_form_once_goes_again() {
    let e = env!(Knobs {
        continue_noop: Noop::Once,
        ..Knobs::default()
    });
    linked(&signup(&e, Opts::default()).await);
}

#[tokio::test]
async fn continue_never_enabled_fails_early() {
    let e = env!(Knobs {
        continue_disabled: true,
        ..Knobs::default()
    });
    let o = signup(&e, Opts::default()).await;
    assert_eq!(
        o.error.as_deref(),
        Some("Breeze Blue Continue button did not enable")
    );
}

#[tokio::test]
async fn mail_reuse_error_fails_before_continue() {
    let mut e = env!(Knobs::default());
    e.inbox.reuse_err = Some("mailbox gone".into());
    let o = signup(&e, Opts::default()).await;
    assert_eq!(o.error.as_deref(), Some("mail: mailbox gone"));
    assert_eq!(e.site.count("click Continue"), 0);
}

#[tokio::test]
async fn signs_in_after_verify_when_signed_out() {
    let e = env!(Knobs {
        logged_in_after_verify: false,
        ..Knobs::default()
    });
    let o = signup(&e, Opts::default()).await;
    linked(&o);
    assert_eq!(e.site.count("click Continue"), 2);
    assert!(o.steps.iter().any(|s| s == "sign in"));
}

#[tokio::test]
async fn key_page_on_login_signs_in_once_and_retries() {
    let e = env!(Knobs {
        keys_signed_out_once: true,
        ..Knobs::default()
    });
    let o = signup(&e, Opts::default()).await;
    linked(&o);
    assert_eq!(e.site.seen(|s| s.keys_visits), 2);
    assert_eq!(e.site.count("click Continue"), 2);
    let at = |x: &str| o.steps.iter().position(|s| s == x).unwrap();
    assert!(at("api key") < at("sign in"));
}

#[tokio::test]
async fn never_logs_secrets() {
    let mut text = String::new();
    let mut secrets = vec![EMAIL.to_string(), NEW_PW.into(), "saved-pw-1".into()];
    let cases = [
        (
            Knobs {
                turnstile: Turnstile::FailOnce,
                ..Knobs::default()
            },
            Opts::default(),
            true,
        ),
        (
            Knobs {
                balance_status: 401,
                ..Knobs::default()
            },
            Opts::default(),
            false,
        ),
        (
            Knobs {
                existing_password: Some("other".into()),
                ..Knobs::default()
            },
            Opts {
                password: "saved-pw-1",
                reused: true,
                ..Opts::default()
            },
            false,
        ),
    ];
    for (k, opts, ok) in cases {
        let e = env!(k);
        let o = signup(&e, opts).await;
        assert_eq!(o.res.is_some(), ok, "{:?}", o.error);
        text += &[o.logs, o.steps, vec![o.error.unwrap_or_default()]]
            .concat()
            .join(" ");
        secrets.push(e.site.link.clone());
        secrets.extend(e.key());
    }
    let text = text.to_lowercase();
    secrets.push("token123".into());
    for s in secrets {
        assert!(!text.contains(&s.to_lowercase()), "a secret in: {text}");
    }
}

#[tokio::test]
async fn key_headers_and_check_key() {
    assert_eq!(key_headers("k"), vec![("xi-api-key", "k".to_string())]);
    for (status, want) in [
        (200, KeyState::Active),
        (403, KeyState::SignedOut),
        (500, KeyState::Unknown),
    ] {
        let site = Site::start(Knobs {
            balance_status: status,
            ..Knobs::default()
        })
        .await;
        let mint = site.urls.key_test.replace("/v1/balance", "/api/keys");
        let r: serde_json::Value = reqwest::Client::new()
            .post(mint)
            .body("default")
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let key = r["key"].as_str().unwrap();
        assert_eq!(check_key(&site.urls, key, None).await, want);
        assert_eq!(
            check_key(&site.urls, "brz_wrong", None).await,
            KeyState::SignedOut
        );
    }
    let gone = Urls {
        key_test: "http://127.0.0.1:9/v1/balance".into(),
        ..Urls::live()
    };
    assert_eq!(check_key(&gone, "k", None).await, KeyState::Unknown);
}

#[tokio::test]
async fn key_test_401_drops_key_500_keeps_untested() {
    for (status, untested) in [(401, false), (500, true)] {
        let e = env!(Knobs {
            balance_status: status,
            ..Knobs::default()
        });
        let o = signup(&e, Opts::default()).await;
        assert_eq!(o.res, None);
        assert_eq!(o.error, Some(format!("API key test failed: HTTP {status}")));
        assert_eq!(o.api_key, None);
        assert_eq!(o.untested.is_some(), untested);
        if untested {
            assert_eq!(o.untested, e.key());
        }
    }
}

#[tokio::test]
async fn survey_that_shows_at_the_key_page_is_skipped() {
    let e = env!(Knobs {
        survey: Survey::AtKeyPage,
        ..Knobs::default()
    });
    linked(&signup(&e, Opts::default()).await);
    assert_eq!(e.site.count("click Start creating"), 1);
}

#[tokio::test]
async fn form_wiped_during_the_mailbox_reuse_is_filled_again() {
    let mut e = env!(Knobs::default());
    e.inbox.wipe = true;
    linked(&signup(&e, Opts::default()).await);
}

/// The refill after the mailbox reuse types again even when the inputs still
/// show the values: the page's own form state may have been reset.
#[tokio::test]
async fn form_state_reset_during_the_mailbox_reuse_is_typed_again() {
    let mut e = env!(Knobs::default());
    e.inbox.reset_store = true;
    linked(&signup(&e, Opts::default()).await);
    assert_eq!(e.site.count("click Continue"), 1);
    assert_eq!(
        e.site.seen(|s| s.typed_password.clone()).as_deref(),
        Some(NEW_PW)
    );
}

#[tokio::test]
async fn survey_last_skip_saves_slowly_then_start_creating() {
    let e = env!(Knobs {
        survey_busy_ms: 1500,
        ..Knobs::default()
    });
    linked(&signup(&e, Opts::default()).await);
    assert_eq!(e.site.count("click Start creating"), 1);
}

#[test]
fn key_re_takes_a_hyphen() {
    assert!(KEY_RE.is_match(&format!("brz_{}-{}abc", "Ab3x".repeat(3), "Zz9_".repeat(4))));
    // The list shows a masked prefix.
    assert!(!KEY_RE.is_match(&format!("brz_{}{}", "Ab3x".repeat(2), "•".repeat(20))));
}

#[tokio::test]
async fn survey_redirect_after_the_key_page_loads_is_skipped() {
    let e = env!(Knobs {
        survey: Survey::AfterKeyPageLoads,
        ..Knobs::default()
    });
    linked(&signup(&e, Opts::default()).await);
    assert_eq!(e.site.count("click Start creating"), 1);
}

#[tokio::test]
async fn survey_is_opened_before_the_key_page() {
    let e = env!(Knobs {
        survey: Survey::AfterKeyPageLoads,
        ..Knobs::default()
    });
    linked(&signup(&e, Opts::default()).await);
    let ev = e.site.seen(|s| s.events.clone());
    let at = |x: &str| ev.iter().position(|e| e == x).unwrap();
    assert!(at("visit welcome") < at("visit keys"), "{ev:?}");
}

#[tokio::test]
async fn waits_for_the_sign_in_configuration_before_continue() {
    let e = env!(Knobs {
        config_load_ms: 1500,
        ..Knobs::default()
    });
    linked(&signup(&e, Opts::default()).await);
    assert_eq!(e.site.count("click Continue"), 1);
}
