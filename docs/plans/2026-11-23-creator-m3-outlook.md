# Creator M3: Outlook mail Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Read Outlook mailboxes through Microsoft Graph (refresh-token rotation included), import Outlook combos, test mailboxes, and make Breeze Blue accounts on Outlook mailboxes, with the Emails page live for what this needs.

**Architecture:** `mail.rs` gains an `OutlookClient` beside `EleClient`. The two share one grace/cutoff helper and sit behind a two-variant `MailClient` enum that `wait_for_new_mail` and the runner's inbox take. The store gains Outlook import (set-based, one transaction) and the Emails list queries. `pages.rs` gains the Emails commands in the shapes `src/creator/types.ts` already defines (the approved mock contract). The runner learns a mailbox type: Ele makes a new inbox per account, as it does now; Outlook picks the free mailboxes for Breeze Blue, oldest first.

**Tech Stack:** Rust (reqwest 0.12, rusqlite 0.32, axum 0.7 for the in-process fake servers, all already in use), React + TS (Shard's UI kit; no new dependency).

**Spec:** `docs/plans/2026-11-22-creator-m0-m2.md` (roadmap M3, Global Constraints, D7, the "For M7" notes), `docs/creator/design/index.md` (Emails rows, bulk tests, error table), `docs/creator/working-process.md` §7, `docs/creator/box-workflow.md`. Python source of truth (read-only): `ghidorah_creator/providers/outlook.py` (whole file), `providers/ele.py` (`reuse_mailbox`, `extract_otp`), `api.py` (`list_emails`, `get_email`, `_mail_page`, `get_email_mail`, `add_emails`, `check_emails`), `engine/planner.py` (`_taken`, `_email_conds`), `engine/errors.py`, `ui/js/emails.js`, tests `tests/test_outlook.py`, `tests/test_ele.py` (`extract_otp` cases).

**Production facts:** 38,312 Outlook mailboxes were imported in Python. A Graph check found 23,759 gone (token expired or consent revoked) and 14,553 active. Most imported mailboxes are dead, and this drives D-M3-2.

---

## Decisions (approved by the owner, 2026-11-23)

The owner approved every recommendation below, with one change to D-M3-4 (its row holds the approved text).

| # | Decision | Recommendation | Why |
|---|---|---|---|
| D-M3-1 | How much of the Emails page goes live in M3 | **The parts M3's backend serves:** list (windows, Type/Status/Free for filters, search, order), Peek (details, accounts, mail with scroll-to-load, Gone state, Refresh), Add emails (Outlook paste with its report, new Ele mailboxes), Test (selection and "Test all mailboxes", pool of 5), "Make N Breeze Blue accounts" from Free for. **Stay mock until M7:** Export, Export all, Delete (it deletes Shard profiles under the engine lock, M5/M7). In the real build those three are not shown (one `MOCK &&` each), never shown as dead buttons | Importing and testing need a page. Everything listed reuses the approved mock components as they are |
| D-M3-2 | An Outlook mailbox found gone at run time | **Check before the browser opens:** a mailbox already `gone` fails `mailbox gone` (Python parity). An Outlook mailbox is then polled once, strictly, before `open`: a dead grant fails `mailbox gone` (MAILBOX_GONE, not retryable, no browser launched), an error fails `Outlook unavailable: …` (MAIL_API_DOWN, retryable). Python polls only inside the flow, so a dead grant there waits 180 s and ends NO_MAIL | One token call (about 1 s) per account. It saves a browser launch and 3 minutes on every dead mailbox, and 62 % of the imported ones are dead. It is also a free "seen" pass: mail already in the box is never taken as the verify mail |
| D-M3-3 | Where M3's Outlook mailboxes come from | **Pasting only** (the Add emails dialog, no line cap). The 38k production rows come with the M4 copy of `ghidorah.sqlite` (D7). Nothing reads Python's mailboxes in M3 | No second import path. Rows made in M3 on the box are test data: M4 replaces the DB file |
| D-M3-4 | Live gate mailboxes (**the run itself needs the owner's go**) | **Approved with a change: no owner-supplied combos.** The gate uses **25 active Outlook mailboxes that no account has ever used**, selected from the backup copy of the Python database on the Windows test box (never the live file): 20 for the live Test, 5 for Breeze. Their refresh tokens rotate in our sandbox store only, so they differ from the Python copy's until M4 brings the database over (accepted by the owner). Pass rule as D10, scaled: Rust Breeze-on-Outlook linked ≥ Python's linked on 5 other Outlook mailboxes the same day − 1, and no error code Python did not show | Both apps rotate refresh tokens. A mailbox used by both ends with two diverging tokens, and Python would see the Rust-made Breeze accounts' mailboxes as still free (a duplicate gives ACCOUNT_EXISTS). Never-used mailboxes keep the Python app's accounts out of it |
| D-M3-5 | Network path for login.live.com and Graph | **Direct, as Python** (not through the account's proxy) | Parity. Same exposure as today: one IP refreshes every token during Test all, and a throttled answer counts as failed, never gone |

## Global Constraints

The M0–M2 plan's Global Constraints apply unchanged. M3 adds:

- Reuse Shard as is, take the simplest path, add no mechanism the owner did not ask for (owner, binding). No new dependency, seam or upstream file. `check-seams` shows no new file.
- Secrets never leave the backend and are never logged: the refresh token (`mailboxes.token`), `client_id`, the Ele mailbox token, the access token, mail codes and links in any log line. No command's answer carries `token` or `client_id`. `Mailbox`'s `Debug` prints them as `***`. `EleError` texts carry no URL, token or body; reqwest errors are reduced to their kind (`class()`), as now.
- Data scale (500k mailboxes target): lists load in windows (`LIMIT/OFFSET`, 100 at most per call), counts are `GROUP BY` over indexed columns, and per-row extras (mail count, accounts) are read only for the window's ids. Imports are one transaction with a temp table for the duplicate check. No N+1, no whole-table load into memory.
- Outlook values are copied from Python: `TOKEN_URL = "https://login.live.com/oauth20_token.srf"`; `GRAPH_URL = "https://graph.microsoft.com/v1.0/me/messages?$top=20&$select=id,subject,from,body,bodyPreview,receivedDateTime&$orderby=receivedDateTime desc"`; token form `client_id`, `grant_type=refresh_token`, `refresh_token`, `scope="https://graph.microsoft.com/Mail.Read offline_access"`; HTTP timeout 15 s; `UNAVAILABLE_AFTER` 30 s; error prefix `"Outlook unavailable: inbox "`. The combo line is `email|password|refresh_token|client_id|recovery`, and its invalid reason is `"not email|password|refresh_token|client_id|recovery"`. `MAIL_PAGE = 50`, `ADD_EMAILS_MAX = 100`.
- Code is edited on the Mac. Build, every test and every live run happen on the Windows test box under `box-env.ps1` (`box-workflow.md`). The bar is no new failures against `main`'s baseline. Live runs need the owner's approval.

## Review Focus

1. **The refresh token rotates and the next poll uses the new one.** The Outlook poll reads the token from the row, not from the `Mailbox` it was given (Rust does not mutate it the way Python does). Losing the rotated token kills the mailbox. Pinned in Task 1 (`second_poll_uses_the_rotated_token`, `rotated_token_is_saved_even_when_graph_fails`).
2. **Only `invalid_grant` is gone.** Other 400/401 answers (`temporarily_unavailable`, `invalid_request`, an empty body), a 429 or 5xx, and a Graph failure all stay active and count as transient. Pinned in Task 1. At 38k mailboxes a wrong rule would mark thousands of live mailboxes gone in one Test all.
3. **A secret in an answer or a log:** a list, Peek, Test or import answer that carries `token`/`client_id`, or an error text holding a URL or token. Pinned in Task 1 (`outlook_errors_hold_no_token`) and Task 4 (`no_command_answer_holds_a_token_or_client_id`).
4. **Test reports the wrong state:** a transient failure on an already-gone mailbox reported as "gone", or an Ele 404 reported as "failed". Pinned in Task 4 (`test_mailbox_states`).
5. **A big paste or a big list:** 38k pasted lines with duplicates inside the paste and against the store, and a 500k-row list with counts. Pinned in Task 2 (`import_dedupes_in_paste_and_against_store_at_40k`) and Task 4 (ignored perf test, run on the box).

---

## File map

```
src-tauri/src/creator/
  mail.rs        + Grace (shared blip/cutoff), OutlookClient, OutlookUrls, MailClient,
                   extract_otp; EleClient uses Grace; wait_for_new_mail takes MailClient   (T1)
  store/mod.rs   + Mailbox.client_id, manual Debug, mailbox_set_token, import_outlook,
                   parse_outlook, email_list, email_mail_page, breeze_free_outlook   (T1, T2, T3, T4)
  errors.rs      + "Outlook unavailable" -> MAIL_API_DOWN, NOT_RETRYABLE           (T3)
  runner.rs      Ctx { ele: Option<EleClient>, outlook: OutlookClient }, MailInbox,
                 pre-flight (D-M3-2), make_breeze(.., mailbox_type, ..)             (T3)
  runner/tests.rs, testsite/mod.rs   fake Outlook routes on the fake site            (T3)
  pages.rs       emails, email, email_mail, email_ids, test_mailbox, parse_outlook,
                 add_outlook, add_ele; make_options/make_check/breeze_run route,
                 platforms free                                                      (T3, T4)
src/creator/
  index.tsx      c-emails shown in the real build                                    (T5)
  pages/EmailsPage.tsx, widgets/{EmailPeek,AddEmailsDialog,MakeDialog}.tsx           (T5)
  i18n/{en,ru,zh}.json   import report keys                                          (T5)
  mock.ts        email_mail by before_id, add_outlook/add_ele/email_ids answers      (T5)
```

---

### Task 1: Outlook Graph client

**Files:**
- Modify: `src-tauri/src/creator/mail.rs`, `src-tauri/src/creator/store/mod.rs`, `src-tauri/src/creator/runner.rs` (only the call sites that the `wait_for_new_mail` signature change forces)
- Test: `mail.rs` tests module, with a fake login and Graph server (axum through `FixtureServer::start_with`, like the Ele fake)

**Interfaces:**
- Produces (store): `Mailbox { …, client_id: Option<String> }` (column already in v24; `MAILBOX` select list grows). `impl Debug for Mailbox` by hand, with `token` and `client_id` printed as `***` (the `derive` goes). `Db::mailbox_set_token(id: i64, token: &str) -> Result<()>`.
- Produces (mail.rs):
  - `struct Grace { last_ok: Mutex<HashMap<i64, Instant>>, cutoff: Mutex<HashMap<i64, i64>>, pub unavailable_after: Duration }`, holding the code moved out of `EleClient` (`ok_now`, `failed(mb_id, prefix, reason, strict)`, `cutoff_of`, `set_cutoff`). `EleClient` keeps its public API and its tests pass unchanged, apart from `client.unavailable_after` becoming `client.grace.unavailable_after`.
  - `pub struct OutlookUrls { pub token: String, pub graph: String }` with `OutlookUrls::live()` set to the Global Constraints values.
  - `pub struct OutlookClient { http, db: Arc<Db>, urls: OutlookUrls, pub grace: Grace }`; `OutlookClient::new(db: Arc<Db>, urls: OutlookUrls) -> Result<Self, EleError>` (reqwest, 15 s timeout).
  - `OutlookClient::poll_inbox(&self, mb: &Mailbox) -> Result<Vec<MailMessage>, EleError>` ports Python `poll_inbox`. Order: (1) read the **current token from the row** (`mailbox_get`). (2) POST the token form. Network error → `failed(class)`. 400/401 with JSON `error == "invalid_grant"` → `mailbox_set_status(gone)` and `Ok(vec![])`. Other 400/401 → `failed("token HTTP {n}")`. Any other status that is not 200 → `failed("token HTTP {n}")`. No `access_token` → `failed("unreadable token response")`. (3) A new `refresh_token` that differs from the stored one → `mailbox_set_token` **before anything else**. (4) GET Graph with `Bearer`. Not 200 → `failed("HTTP {n}")`. Unreadable → `failed("unreadable response")`. (5) Set status active, `ok_now`. (6) Store items with an `id` not yet in `mail_list` (by `ele_message_id`): `sender = from.emailAddress.address`, `body = html_text(body.content)`, `html = content` only when `contentType == "html"`, `received_at = receivedDateTime`. `failed` here uses the prefix `"Outlook unavailable: inbox "`.
  - `OutlookClient::reuse_mailbox(&self, mb) -> Result<(), EleError>`: the same body as Ele's (strict poll, stale marking, cutoff), sharing one private fn `mark_stale_and_cutoff(db, grace, mb_id)` with Ele.
  - `pub enum MailClient<'a> { Ele(&'a EleClient), Outlook(&'a OutlookClient) }` with `poll_inbox`, `reuse_mailbox`, `cutoff_of(mb_id)` and `check(mb) -> Result<(), EleError>` (one strict poll: Python's `_last_ok = -inf`).
  - `wait_for_new_mail(client: MailClient<'_>, …)`. Same body, only the client type changes.
  - `pub fn extract_otp(text: &str) -> Option<String>`, ported verbatim from `ele.py` (all five patterns plus the fallback). The Peek's "code" needs it.
- Later (M6, Qwen): `six_digit_code` and the `otp_extracted` prefill from `bodyPreview`. M3 stores no code at poll time.

- [ ] **Step 1: Failing tests.** Port `tests/test_outlook.py` poll cases against the fake. The fake asserts the form fields and scope, `Authorization: Bearer AT` and the Graph query string:
  - `poll_maps_mail`: sender, subject, body `"Hi there"`, `received_at` = `"2026-11-17T10:00:00Z"`.
  - `poll_persists_rotated_token`.
  - `second_poll_uses_the_rotated_token`: the fake records `refresh_token` per call: `["OLD", "NEW"]`.
  - `rotated_token_is_saved_even_when_graph_fails`: token 200 with `NEW`, Graph 503. The row holds `NEW` and the mailbox stays active.
  - `poll_dedup`.
  - `poll_invalid_grant_marks_gone`: `Ok([])`, row gone.
  - `poll_other_400_not_gone`, three bodies: `{"error":"temporarily_unavailable"}`, `{"error":"invalid_request","error_codes":[90033]}`, `{}`.
  - `poll_429_and_5xx_are_transient`: token 429, then Graph 503. Both `Ok([])` inside the grace; with `unavailable_after = 0` both are `Err` starting `"Outlook unavailable: inbox "`.
  - `poll_stores_html_only_for_html_content`.
  - `reuse_mailbox_then_poll_returns_only_new`: as Python, including `NEWER` and token 500 → `Err` on reuse.
  - `wait_for_new_mail_on_outlook_drops_mail_at_or_before_the_cutoff`.
  - `outlook_errors_hold_no_token`: no error text contains `OLD`, `NEW`, `CID`, `AT` (as a whole token), `login.live.com` or `graph.microsoft.com`.
  - `mailbox_debug_masks_token_and_client_id`.
  - `extract_otp_cases`: the `test_ele.py` `extract_otp` cases, with the same inputs and outputs.
  - The existing Ele tests unchanged.
- [ ] **Step 2:** Box: `cargo test --manifest-path src-tauri/Cargo.toml creator::mail` → the new tests fail to compile or fail.
- [ ] **Step 3:** Implement as the Interfaces block says.
- [ ] **Step 4:** Box: `cargo test … creator::` PASS (Ele and runner tests included). `rustfmt --edition 2021 --check` clean.
- [ ] **Step 5:** Commit `Creator: Outlook Graph mail client with refresh-token rotation`.

### Task 2: Outlook import

**Files:**
- Modify: `src-tauri/src/creator/store/mod.rs`
- Test: store tests module

**Interfaces:**
- `pub struct OutlookLine { pub email: String, pub token: String, pub client_id: String, pub domain: String }`.
- `pub fn parse_outlook(text: &str) -> (Vec<(usize, OutlookLine)>, Vec<(usize, &'static str)>)`, a port of `_parse`. Line numbers are 1-based and count blank lines. Each part is trimmed. Exactly 5 parts are needed, plus `@` with a non-empty domain and non-empty token and client id. The password (part 2) and recovery (part 5) are dropped and never stored. Python keeps the domain's case, and so does this port.
- `pub struct OutlookImport { pub added: usize, pub duplicates: Vec<usize>, pub invalid: Vec<(usize, &'static str)> }`.
- `Db::import_outlook(&self, text: &str) -> Result<OutlookImport>`, in **one transaction**: create a temp table `_imp(email)`, insert the parsed emails, select the known ones through `idx_mailboxes_email` (`SELECT email FROM mailboxes WHERE email IN (SELECT email FROM temp._imp)`), drop the temp table (also on error). In line order: a known email or an email seen earlier in the paste goes to `duplicates`; otherwise a prepared `INSERT INTO mailboxes (provider,email,domain,token,status,client_id,created_at) VALUES ('outlook',?,?,?,'active',?,now)`. Email match is exact, as Python's.

- [ ] **Step 1: Failing tests** (Python names):
  - `parse_valid`.
  - `parse_invalid`: lines `[1,3,4,5]` from the Python text.
  - `import_creates_rows`: provider, status, token, client_id, domain.
  - `import_dedup`.
  - `import_dedupes_in_paste_and_against_store_at_40k`: 40,000 lines, of which 3,000 are already stored and 500 repeat inside the paste. `added`, `duplicates` and `invalid` counts are exact, and it takes under 5 s on the box.
  - `import_leaves_no_temp_table`.
  - `password_and_recovery_never_stored`: no stored column holds them.
- [ ] **Step 2:** Box: `cargo test … creator::store` → FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Box: PASS.
- [ ] **Step 5:** Commit `Creator: Outlook combo import`.

### Task 3: Breeze Blue on Outlook mailboxes

**Files:**
- Modify: `src-tauri/src/creator/runner.rs`, `runner/tests.rs`, `runner/live.rs` (`CREATOR_LIVE_MAILBOX=outlook|ele`, Ele by default), `errors.rs`, `store/mod.rs`, `pages.rs` (`make_options`, `make_check`, `breeze_run`, `platforms`), `testsite/mod.rs`
- Test: `runner/tests.rs`, `errors.rs`, `pages.rs` tests

**Interfaces:**
- `errors.rs`: a rule `Contains("Outlook unavailable") → MAIL_API_DOWN`, placed right after the Ele one (Python order). `pub const NOT_RETRYABLE: &[&str]` holds every code whose `fail_info(..).1` is false, pinned by a test that walks it.
- `Db::breeze_free_outlook_where() -> (String, Vec<Value>)`, a port of `_email_conds` + `_taken` for the email route of one platform with provider fixed to `outlook`, on alias `m`: `m.provider='outlook' AND m.status='active' AND lower(m.domain) NOT IN (SELECT domain FROM blocked_domains) AND m.token<>'' AND COALESCE(m.client_id,'')<>'' AND m.id NOT IN (SELECT mailbox_id FROM accounts WHERE platform=?1 AND ((status<>'failed' OR frozen=1) OR (status='failed' AND error_code IN NOT_RETRYABLE)))`. `Db::breeze_free_outlook(limit: i64) -> Result<Vec<i64>>` returns the oldest first. `Db::breeze_free_outlook_count() -> Result<i64>`.
  - ponytail: no count cache (Python caches by table revisions); measure in Task 4's perf test first.
- `runner.rs`:
  - `Ctx { db, ele: Option<EleClient>, outlook: OutlookClient, browsers, urls, tm, screens }`. `Ctx::mail_for(&self, mb: &Mailbox) -> Result<MailClient<'_>>`: outlook → `Outlook(&self.outlook)`; ele → `Ele(self.ele.as_ref())`, or `"Ele API unavailable: ele_api_url / ele_api_key not set"`.
  - `EleInbox` becomes `MailInbox { client: MailClient<'a>, mailbox, scrub }`, with the same `Inbox` impl.
  - In `attempt`, before `browsers.open`: add `client_id` to `scrub`. `mb.status == "gone"` → `bail!("mailbox gone")`. For Outlook, `client.check(&mb)`: `Err(e)` → bail with `e.0`; the row now gone → `bail!("mailbox gone")` (D-M3-2). Nothing has launched yet, so a `?` is fine here.
  - `make_breeze(cx, count, proxy_mode, mailbox_type: &str /* "ele"|"outlook" */, run)`. Outlook: `breeze_free_outlook(count)` once at the start, then one account per id: `pick_proxy` → `browser_of_mailbox` or `browser_create` → `account_create(route "email")` → `run_account`. Fewer ids than `count` → it makes what there is. Ele stays as it is.
  - `start(count, proxy_mode, route)`: `route` is `"email:ele"` or `"email:outlook"`; anything else → `unknown route`. Outlook with `count > breeze_free_outlook_count()` → `Only {n} free emails on this route`. `ele` is built only for the Ele route.
- `pages.rs`:
  - `make_options` → `{"email:ele": null, "email:outlook": <free count>}`.
  - `make_check {route}` adds `NO_ELE_KEY` only for `email:ele`.
  - `breeze_run {count, proxy_mode, route}`, where a missing route means `email:ele` (today's callers).
  - The `platforms` Breeze card's `free` is the Outlook free count.
- `testsite`: fake Outlook routes `/ms/oauth20_token.srf` and `/ms/v1.0/me/messages` serve the verify mail the site sends to an `@outlook.test` address. Knob `outlook_token: Ok | InvalidGrant | Http(u16)`. `Site::outlook_urls() -> OutlookUrls`.

- [ ] **Step 1: Failing tests:**
  - `outlook_breeze_happy_path_persists_linked`: the account is linked and its token rotated in the row.
  - `gone_mailbox_fails_before_the_browser_opens`: the row is already gone, `TestBrowsers` saw no open, the code is `MAILBOX_GONE`.
  - `dead_grant_at_preflight_is_mailbox_gone_without_a_browser`.
  - `outlook_down_at_preflight_is_mail_api_down_and_retryable`.
  - `outlook_make_takes_free_mailboxes_oldest_first_and_skips_taken`: the skipped cases are a linked Breeze account, a running one, a frozen failed one, a failed one with a non-retryable code, a gone mailbox, a blocked domain and an empty client id. A retryably failed one is taken again.
  - `outlook_route_refuses_more_than_free`.
  - `errors::outlook_unavailable_is_mail_api_down`.
  - `pages::make_options_counts_free_outlook`.
  - `no_secret_in_events_or_errors` extended with the refresh token and the client id.
- [ ] **Step 2:** Box: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Box: `cargo test … creator::` PASS. The browser-backed runner tests run with `CREATOR_REQUIRE_BROWSER=1` (set by `box-env.ps1`).
- [ ] **Step 5:** Commit `Creator: Breeze Blue on Outlook mailboxes`.

### Task 4: Emails commands

**Files:**
- Modify: `src-tauri/src/creator/pages.rs`, `store/mod.rs`, `runner.rs` (`pub fn outlook() -> Result<&'static OutlookClient>`, built once on `db()` with `OutlookUrls::live()`, so a Test all reuses one connection pool)
- Test: `pages.rs`, `store` tests

**Interfaces** (answers are the `types.ts` shapes; none carries `token` or `client_id`):
- `emails {kind, status, free_for, q, order, offset, limit}` → `EmailList`. `kind` is `outlook|ele`; `status` is `active|gone`; `free_for` is only `breezeblue` in M3, through `breeze_free_outlook_where`, and any other key → an empty result. `q` is `LIKE` on email (escaped as in `account_list`). `order` is `newest` (`m.id DESC`), `oldest` or `email`. `limit` is 1..100 (default 100). Counts follow Python `_email_counts`: per type with every filter but type, and the store's two `GROUP BY` queries. `mails` and `accounts` are read only for the window's ids (`IN (...)`).
  - `Db::email_list(&EmailQuery) -> Result<Value>`, `EmailQuery { kind, status, free_for, q, order, offset, limit }`, beside `account_list`.
- `email {id}` → `EmailDetail`, a port of `get_email`. The mailbox is not polled when it is gone or has an account in `running`. Otherwise an Ele mailbox needs Ele set, and the poll is one non-strict `poll_inbox` with a 60 s bound. A failure is ignored (stored mail is shown) and its text goes nowhere. Then come the first `MAIL_PAGE` mails, newest first (`COALESCE(received_at,'') DESC, id DESC`), with `code = extract_otp(subject + body)` and `link = null` (ElevenLabs' verify link is M6). `technical {browser_id, profile, proxy}` comes from `browser_of_mailbox` (proxy `host:port`, never credentials). `accounts` is `[{platform, status, verify_status}]`.
  - ponytail: Python's naive-Ele-clock-to-UTC calibration (`_mail_page.utc`) is left out; Ele times may show hours off. Port it if the owner sees it.
- `email_mail {id, before_id}` → `{items, more}`, the next 50 older than `before_id` by the same key (Python's cursor, so new mail landing at the top never shifts or duplicates rows).
- `email_ids {kind, status, free_for, q}` → `number[]`: every id matching the filters, for "Test all mailboxes" and select-all (one `SELECT m.id`, ids only).
- `test_mailbox {id}` → `"active" | "gone" | "failed"` (`check_emails` for one mailbox):
  - an account `running` on it → its stored status, no poll;
  - Ele without Ele settings → `failed`;
  - otherwise `MailClient::check`: `Ok` → the row's status after it; `Err` whose text ends `"inbox HTTP 404"` → `gone` (Ele deleted it; the row is set gone already); any other `Err` → `failed`. Bounded at 60 s, and a timeout is `failed`.
- `parse_outlook {text}` → `{valid, invalid}` (counts).
- `add_outlook {text}` → `{added, duplicates: number[], invalid: [line, reason][]}`.
- `add_ele {count}` → `{created, error?}`. Count is 1..`ADD_EMAILS_MAX`. It makes them one after another with `create_mailbox` and keeps what was made if one fails; the error text is `EleError`'s.
- `nav_counts` unchanged.

- [ ] **Step 1: Failing tests:**
  - `email_rows_match_types`: the keys of `emails`, `email` and `email_mail` equal `EmailList`, `EmailRow`, `EmailDetail` and `MailMessage` in `types.ts`, through the existing `fields()` helper.
  - `emails_filter_count_and_window`.
  - `free_for_breeze_lists_only_free_outlook`.
  - `email_peek_skips_poll_when_gone_or_running`.
  - `email_mail_pages_by_cursor_without_shift`: a new mail stored between calls is not returned twice.
  - `test_mailbox_states`: active, invalid_grant → gone, a 503 on a row already gone → `failed` (not `gone`), Ele 404 → `gone`, running → stored status with no request (the fake counts calls), Ele unset → `failed`.
  - `no_command_answer_holds_a_token_or_client_id`: serialise every answer above and assert neither value appears.
  - `add_outlook_reports_duplicates_and_invalid_lines`.
  - Ignored perf test `emails_list_at_500k`: 500,000 mailboxes made with one `WITH RECURSIVE` insert, 1,000 accounts. The first window plus counts, unfiltered and with each filter, each under 300 ms on the box. Record the numbers in the task report.
- [ ] **Step 2:** Box: FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Box: PASS, then `cargo test --locked --lib creator::pages::tests::emails_list_at_500k -- --ignored --nocapture`.
- [ ] **Step 5:** Commit `Creator: Emails commands`.

### Task 5: Wire the Emails page

**Files:**
- Modify: `src/creator/index.tsx` (the `PAGES` filter lets `c-emails` through in the real build), `pages/EmailsPage.tsx`, `widgets/EmailPeek.tsx`, `widgets/AddEmailsDialog.tsx`, `widgets/MakeDialog.tsx` (pass `route` to `breeze_run`), `mock.ts` (answers for `email_ids`, `add_outlook`, `add_ele`, and `email_mail` by `before_id`), `i18n/{en,ru,zh}.json`
- Test: `npm run check` (incl. `i18n.test.ts` copy rules), `e2e/creator-mockups.spec.ts` in `dev:mock`

**Interfaces** (the Task 4 commands):
- Add emails confirm → `add_outlook {text}` or `add_ele {count}`. The dialog stays open and shows the report on the "Ready to add" line: `creator.addEmails.added` / `addedOne`, plus new keys `creator.addEmails.duplicates` (`"{n} already added"`, `duplicatesOne`) and the existing `invalid` / `invalidOne` with their line numbers (the first 20, then "and {n} more" with Shard's wording if it has one). Then the list reloads. Ele's "How many" max is 100, the backend's limit. The sample text in the textarea is the mock's only: the real build starts empty.
- Test (bulk bar) uses the selected ids. "Test all mailboxes" uses `email_ids` with the current filters, never `list.items`. Both use the existing `runPool(…, 5, test_mailbox)` and the existing toasts. The list reloads after the summary.
- Peek's Refresh reloads the `email` call (a key bump) instead of the mock toast. `MailList` asks `email_mail {id, before_id: items.at(-1).id}`.
- Export, Export all and Delete render only when `MOCK` (D-M3-1).
- Free for: the Breeze Blue chip and "Make N Breeze Blue accounts" open Make with route `email:outlook` preselected.

- [ ] **Step 1:** Box: `npm run check` and the mockup spec pass on `main` first (baseline).
- [ ] **Step 2:** Wire as above. `npm run check` PASS. `E2E_PORT=4794 npx playwright test e2e/creator-mockups.spec.ts --project=dev` passes (screens unchanged except where the mock now answers new commands).
- [ ] **Step 3: Dev run on the box (sandboxed, fake data only).** Paste 30 made-up combos (3 duplicates, 2 bad lines) and check the report. Open Peek, check the Gone state on a row set gone by hand, and scroll mail on a mailbox seeded with 120 mails. Run Test on made-up Outlook combos: all `failed` or `gone` (Microsoft refuses fake tokens with `invalid_grant` or 400). This sends fake tokens to login.live.com, never a real mailbox. Screenshots light/dark for the report, emails blurred.
- [ ] **Step 4:** Commit `Creator: Emails page on real data (list, Peek, add, test)`.

### Task 6: Box gate and live gate

- [ ] **Step 1: Full box gate** on the branch (`box-workflow.md` flow), compared with `main`'s baseline: `npm run check`; `cargo test --locked` (in `src-tauri`, full); rustfmt check; `node --test scripts/creator/*.test.mjs`; `node scripts/creator/check-seams.mjs` (no new file); the e2e lane from §7.
- [ ] **Step 2: Whole-branch review** by a fresh reviewer (opus or fable) before any live run.
- [ ] **Step 3: Live gate, ONLY after the owner's explicit approval (D-M3-4).** In the sandbox, back up our sandbox DB first. Then:
  1. Select the 25 mailboxes (D-M3-4) from the backup copy of the Python database into a combo file in the sandbox, never printed, then paste them into our sandbox store (5 for Breeze first, so Make takes them oldest first; then the 20 for the Test).
  2. Test them (pool of 5) and report active/gone/failed. The harness tests every mailbox the paste added, so all 25 (a superset of the 20). Where Python's status is known, compare it per mailbox. Rotated tokens stay only in our sandbox DB.
  3. One Breeze run on Outlook through `live_breeze` with `CREATOR_LIVE_MAILBOX=outlook CREATOR_LIVE_COUNT=1` (scheduled task in the desktop session).
  4. Then 4 more, interleaved with 5 Python Breeze-on-Outlook runs the same day on other mailboxes.
  5. The report table: linked count, error codes per app, median seconds.
  6. The pass rule is D-M3-4's. Results go in the report, not the repo.
- [ ] **Step 4:** Report to the owner (result, test count, screenshots, what is unverified). Merge only on "merge".

---

## Execution notes

- Order: T1 → T2 and T3 (T2 and T3 both touch `store/mod.rs`, so run them one after the other: T2, then T3) → T4 → T5 → T6. T5 can start once T4's command shapes are committed.
- Models:
  - T1: opus, reviewer fable or opus. Rotation and gone rules are the core risk.
  - T2: sonnet, reviewer opus.
  - T3: opus.
  - T4: sonnet, reviewer opus (secrets in answers).
  - T5: sonnet.
- Each task: implementer → fresh reviewer → fix rounds. Push the task branch to `origin` only on the owner's word, because the box fetches from GitHub (`box-workflow.md`). Until then, a task can be reviewed on the Mac, but its gates wait.

## Risks

- **Concurrent rotation:** a Peek refresh, a Test and a run polling the same mailbox at once each refresh with the stored token, and the last writer wins. Microsoft keeps an older refresh token valid after it issues a new one, so this is safe in practice. Python has the same race. Test skips mailboxes with a running account, and Peek does not poll them.
- **Throttling at scale:** Test all on 14k+ live mailboxes from one IP may get AADSTS throttling (429 or 400 `temporarily_unavailable`). By design those count as failed, never gone, so a rerun later fixes them.
- **Two apps, one mailbox:** until M4 retires the Python DB, a mailbox used by both apps forks its refresh token. The live gate uses disjoint mailboxes (D-M3-4). M4's file copy brings Python's tokens back, and any token rotated only in our M3 sandbox is lost then. That is harmless while old tokens stay valid, and gone if Microsoft ever revokes them.
- **Pre-flight changes behaviour vs Python** (D-M3-2): accounts on dead mailboxes end MAILBOX_GONE in about 1 s instead of NO_MAIL after 180 s. The fail-streak rules (M5) treat MAILBOX_GONE as `streak=false`, which is correct.
- **A permanent token error at the head of the queue:** an Outlook mailbox whose token call keeps failing with something other than `invalid_grant` (a 400 such as `invalid_request`, or a 5xx) fails pre-flight as MAIL_API_DOWN, which is retryable, so it stays free and, being oldest, is taken first by every Make. Watch for it in the live gate; if it shows, count such failures per mailbox or skip a mailbox that failed this way recently.
- **Ele times in Peek** may show hours off until the calibration is ported (Task 4 ponytail note).
- **Free count without a cache:** one `COUNT` with a `NOT IN` subquery per Make dialog open and per `platforms` call. Fine at 38k. Task 4's perf test tells whether 500k needs Python's revision cache.
