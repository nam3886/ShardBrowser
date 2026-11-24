# Creator M5: engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make accounts run as a queue: Make queues them, a fixed number run at once, a failure streak stops a platform's queue until the next Make or Retry, Stop takes a job's waiting accounts out, Retry queues a failed account again, a start of the app fails what a crash or close left half-done, a browser closed mid-run ends its account at once, and a proxy a run failed on stops being picked. The Python-database import leaves the app (the owner copies the file by hand). Still Breeze Blue only.

**Architecture:** The queue is the `accounts` table, as in Python: a row with `status='queued'` waits, and one function (`Engine::schedule`) is the only place a run starts. `engine.rs` (new) holds the scheduler and its in-memory state (halted platforms, streak counts, running ids); it knows nothing about browsers: it gets "run account `id`" as a function and reads the row when that ends, so its tests need no browser. `runner.rs` keeps the one-account run (`run_account`) and gains Make-as-queue, the browser row made at run start, and the closed-browser signal. `proxy.rs` makes pick+touch (and pick+browser row) atomic and holds the one-shot Retest. `pages.rs` exposes the commands the existing pages call. No new dependency, seam or upstream file; schema stays v24.

**Tech Stack:** Rust (tokio, rusqlite 0.32, already in use; Shard's own `space::proxy::geo_check` for Retest), React + TS (Shard's UI kit).

**Spec:** `docs/plans/2026-11-22-creator-m0-m2.md` (roadmap M5, Global Constraints, D16, D17), `docs/plans/2026-11-23-creator-m3-outlook.md` (pre-flight, platform-mail rule), `docs/plans/2026-11-24-creator-m4-data.md` (no startup sweep yet), `docs/creator/design/index.md` (job card, halted queue card, the error table's streak column), `docs/creator/working-process.md` §7, `docs/creator/box-workflow.md`, `docs/creator/windows-build.md`. Python source of truth (read-only): `ghidorah_creator/engine/engine.py` (`_schedule`, `_done`, `_tally`, `_run_svc`, `resume_queue`, `mark_interrupted`, `_retry_service`, `_requeue`, `_cap`, `shutdown`), `engine/planner.py` (`make`, `_queue`, `_taken`), `engine/account.py` (`create_browser`), `engine/account_service.py` (`run_service_account`: the closed-window checks), `engine/errors.py` (`Code.streak`, `Code.retryable`), `proxy/assign.py` (`pick_proxy`, `usable`), `store/models.py` (`DEFAULTS`, `record_failure`: the `failing` mark; `proxy/checker.py`'s loop is not ported), `api.py` (`get_state`, `resume_queue`, `retry_accounts`, `make_accounts`, the `max_chrome` range), tests `tests/test_engine.py`, `test_fail_streak.py`, `test_scheduler.py`, `test_max_chrome.py`, `test_busy.py`, `test_proxy_assign.py`.

---

## Facts and constraints (verified while planning)

- **Today's run is one at a time.** `runner.rs` has one global `RUN: RunState` (`begin` refuses a second run), `begin_job` + `make_breeze` make accounts one after another, and "Stop" is a flag read between accounts. Its notes name what M5 owes: no close watcher, no startup sweep (`run_account`), pick and touch are two statements (`proxy::pick_proxy`), a rerun after a failed pick reads "proxy deleted" (`make_rows_and_run`).
- **Python's queue is the accounts table.** Make inserts the job and its `queued` rows in one transaction; `_schedule` starts queued rows (oldest id first) while fewer than `max_chrome` run; a run's end calls `_tally` then `_schedule`. Nothing else starts a run. The halted state is memory only and is gone after a restart.
- **Python values, copied:** `max_chrome` default 3, range 1..20; `fail_streak_stop` default 3, range 0..50, 0 = never stops. `LIVE = queued, running, waiting`.
- **Retry is by hand in Python.** No account is ever retried by itself; `retry_count` only counts the person's retries. M5 adds no automatic retry.
- **A restart empties the queue in Python.** `mark_interrupted` fails every unfrozen row left `queued`, `running` or `waiting` as `Interrupted` (retryable, never counted toward a streak). Queued rows do not resume by themselves: no browser opens at app start without the person asking. The person retries them.
- **Streak rule exists already:** `errors::counts_toward_streak` (marked `dead_code` until now) and `NOT_RETRYABLE` match Python's `Code.streak` / `Code.retryable`. Today, as in Python, it still counts four proxy codes (`PROXY_REFUSED`, `PAGE_LOAD_FAILED`, `GEO_BLOCKED`, `PROXY_CHECK_FAILED`); the owner's D-M5-3 changes that (Task 2).
- **A dead app leaves no browser.** Shard ties every browser to the launcher process (`process/tie.rs`: a job object with kill-on-close on Windows), and `shard::open_session` already stops a profile Shard still tracks before a run. So Python's `stop_leftovers` has nothing to do here and is not ported.
- **Closing is Shard's, and the signal is the CDP connection.** Our profiles are ordinary Shard profiles (D17): Close all, the quit prompt and a crash are Shard's own code. When the browser goes, its WebSocket closes: vendored chromiumoxide's `Connection` stream ends (`creator-vendor/chromiumoxide/src/conn.rs`: `WsMessage::Close` and the stream's `None` both give `Poll::Ready(None)`), the `Handler` stream stops yielding, and our pump task in `web/mod.rs` `Session::connect` (`while handler.next().await.is_some() {}`) falls out of its loop. That end is the signal; no poll of Shard's tracker is needed. Today nothing reads it: a page call after the close fails, but a wait that makes no page call (the verify-mail wait, up to 180 s) runs to its timeout before the run reads `Interrupted`. `TtsRun.closed` ("the runner sets it") is the flag every wait already ends on.
- **App quit mid-run needs no shutdown code of ours:** the next start's sweep fails the rows (Python relies on the same sweep after a capped shutdown).
- **Proxies, the Shard way (D-M5-3).** Shard checks a proxy when it launches a profile on it and launches anyway; `shard::open_session` already fails a run whose page is on UTC behind a proxy (`proxy location lookup failed`, `PROXY_CHECK_FAILED`), and a page that does not load is `PAGE_LOAD_FAILED`. No checker of ours runs in the background. v24 already has what marks a proxy: `proxies.status` (`failing`), `reason`, `fail_streak`, `checked_at`; Python's `record_failure` sets `status='failing'` on one failure, and `proxy::usable` already skips `failing`. `crate::space::proxy::geo_check(&ProxyEntry, None)` is public, goes through the proxy and writes nothing; `space::proxy::full_test` is NOT used (it keeps a history in Shard's own proxy store).
- **No schema change is needed or made.** `jobs` has `choice`, `proxy_mode`, `proxy_kind`; `accounts` has `retry_count`; settings are key/value rows. **For M7 (owner):** the Proxy pool page shows only what v24 holds (host, port, user, on/off, status, reason, country, exit IP, static or rotating, sign-up counts, resting). The mock's name, notes, protocol, IP type and IP history are dropped from M7's scope; no column is added for them.
- **What the pages already have:** the Accounts job card with Stop, the halted-queue card (its Resume is a toast only today), the Peek's Retry (mock only), and the Settings rows "browsers at once" and "stop a queue after N failures" (mock only). `RunState` (`types.ts`) already carries `jobs[]` and `halted[]`.
- **The import is in the app today (M4):** `store::import_python`, `open_or_import`, `python_db_path`, `ImportReport`, the `creator-import.json` report, the `Importing` / `Failed` states of `runner::db()`, the ignored `live_import` test and their tests. The owner now does the cutover by hand (D-M5-4), so all of it goes. The store is Python's schema v24 verbatim and `Db::open` adds `idx_mail_sender` itself, so a copied `ghidorah.sqlite` opens as our store. `import_basics` (M2: proxies, blocked domains and Ele settings from a copy; the Settings button and `live_breeze` use it) is not part of this and stays.
- **Public repo:** no production counts, hosts, user names or addresses in this plan, the code, tests or commit messages. Live results go to the owner report only.

## Decisions (answered by the owner, 2026-11-24)

| # | Decision | Answer | In plain words |
|---|---|---|---|
| D-M5-1 | What **Stop** on a running job does, now that several accounts run at once | **Approved as recommended:** the job's waiting accounts are taken out of the queue (Failed "cancelled", shown as stopped, Retry works on them, never counted toward a failure streak). Accounts already in a browser finish. Other jobs go on | Stop means "start nothing more of this job"; nothing is deleted |
| D-M5-2 | A free Outlook mailbox that turns out to hold Breeze Blue mail when it is first read | **Approved as recommended:** its account fails "already exists" within about a second through the existing pre-flight, no browser opens, and the job makes one fewer | A clear failed row and a second Make, no swapping mid-job |
| D-M5-3 | Proxy checking | **Changed by the owner: Shard's mechanism, no checker of ours.** No background loop and no check when a proxy is picked. A dead proxy fails the run with its proxy error code. Proxy errors never count toward a failure streak. A run that fails with a proxy error marks its proxy failing at once (the existing `status` column, as Python's `record_failure`); pick skips it; Retest sets it back when it passes. Nothing is moved, no schema change | The run itself is the test. Retest is one check, on demand |
| D-M5-4 | Bringing the Python data over | **Owner: by hand, no import code in the app.** Close the Python app, copy `ghidorah.sqlite` to our store's place, start our app. The M4 import code and its tests are deleted (Task 0) and `docs/creator/cutover.md` is rewritten | One file copy instead of code that runs once |

**Manager simplifications (2026-11-24), applied:**

1. No 1 s close poll: the closed CDP connection is the signal (Facts).
2. **Pause all / Resume all and a Resume command are not built.** The approved mockup's header item "Pause all queues" stays mock only and is not wired in M5 or planned for M7; per-job Stop plus Retry cover it. **A halted platform clears its halt when the person starts a new Make or a Retry for that platform** (owner): the halt and its streak count are dropped, and what was still queued for it starts again with the new work. Without that rule a halted platform's queued accounts could only be started by an app restart, which fails them.
3. The fail streak is Python's (per platform), and its threshold is Python's setting `fail_streak_stop` (owner: the approved Settings mock has the row and the owner uses it): default 3, range 0..50, 0 = off, read from the store, so a copied store keeps its value. The Settings page shows it beside "browsers at once".
4. The M7 Proxy pool page shows only v24's columns (Facts).
5. Prune and key/verify checks stay out of M5.

## Not in M5

- The roadmap's "key/verify checks" (they need each provider's check: M6) and "prune" (old events, screenshots, mail bodies: with M7/M8).
- Pause all / Resume all and Resume (not built, above). Python's desktop notification when a queue stops (the halted card shows it), bulk and row Retry, Open browser, Close all browsers of ours (Shard's Close all is there), Export, Delete: M7.
- The Retest button and the failing-proxy Problems card: they live on the Proxy pool page (M7). M5 ships the marking and the `test_proxy` command they will call.
- "Free Ele mailboxes first" (Python's planner): the Ele route makes new inboxes, as today. Make stays 1-20 accounts per job (Python allows 500); several jobs may queue.
- Google's sweep extras ("Quit after OTP", deleting the failed profile): with Google in M6. Until then every swept row reads `Interrupted`.

## Global Constraints

The M0-M2, M3 and M4 Global Constraints apply unchanged. M5 adds:

- **One place starts a run:** `Engine::schedule`. It checks and registers under one lock with no await in between. Nothing else calls `run_account` in the app.
- **At most `max_chrome` runs at once, one per mailbox.** A slot is freed on every end of a run: success, failure, error, panic.
- **The app has no import path:** `db()` opens the store or makes a fresh v24 one, nothing else.
- **Queue state the person sees is the row:** `queued`, `running`, `failed`. Halted platforms and streak counts are memory only (Python): a restart clears them and the sweep fails what was live.
- **Atomic picks:** a proxy pick and its `last_used_at` stamp, and a pick plus the new browser row, happen under one process-wide lock; a job and all its queued rows, including the choice of free mailboxes, are one transaction. The store has one writer process (ours).
- **Nothing names a proxy:** no host, user, password or URL in an event, a reason or an error. A proxy is named by its row id; reasons are fixed texts or error codes.
- **Tests never reach the network:** Retest takes its probe as a function.
- Code is edited on the Mac; build, every test and every live run happen on the Windows test box under `box-env.ps1`. The bar is no new failures against `main`'s baseline. Live runs need the owner's approval.

## Review Focus

1. **Two runs on one mailbox, or more than the cap.** Check-and-register must not be split by an await or a second lock. Pinned in Task 2 (`concurrency_respected`, `one_running_account_per_email`, `retry_respects_cap`).
2. **A slot that never comes back.** A run that panics, errors before its row is written, or leaves its row `running` must free its slot and fail the row. Pinned in Task 2 (`slot_released_when_the_run_panics`, `a_run_that_leaves_its_row_live_is_failed`).
3. **Two slots take the same proxy or mailbox.** Pinned in Task 1 (`concurrent_makes_never_share_a_mailbox`) and Task 3 (`concurrent_picks_never_share_a_proxy`, `concurrent_new_browsers_spread_in_least_mode`).
4. **The streak counts the wrong failures, or stops twice.** Owner stops, every proxy code, mailbox causes and refusals before a browser never count; runs already going when the queue stops still count but stop it once. Pinned in Task 2.
5. **The sweep fails a healthy row, or runs late.** Only unfrozen live rows; before the first schedule; once per opened store. Pinned in Task 1 (`sweep_fails_only_unfrozen_live_rows`) and Task 4 (`the_store_is_swept_when_it_opens`).
6. **A good proxy marked failing, or a dead one picked forever.** Only the two clear proxy errors (`PROXY_CHECK_FAILED`, `PROXY_REFUSED`) mark it; a page that did not load and any other failure (captcha, mail, the site refusing) never do, so a site outage cannot drain the pool; a marked proxy is skipped by `pick_proxy` until Retest passes. Pinned in Task 4 (`a_proxy_failure_takes_the_proxy_out_of_picks`, `other_failures_never_mark_the_proxy`) and Task 3.
7. **A halt nobody can clear.** A new Make and a Retry of that platform must both clear it and start what was queued. Pinned in Task 2 (`a_retry_clears_the_halt_and_the_queue_goes_on`) and Task 4 (`a_new_make_clears_the_halt`).
8. **The import removal takes too much.** `idx_mail_sender`, its tests, `import_basics`, `live_perf`, `live_profiles_check` and `live_open_imported` stay. Pinned in Task 0.

---

## File map

```
src-tauri/src/creator/
  store/mod.rs     - import_python*, open_or_import, python_db_path, ImportReport, their tests       (T0)
                   + Job, job_get, queue_job, accounts_queued, account_requeue, job_cancel_queued,
                     sweep_live, jobs_live, setting_int, breeze_free(provider),
                     proxy_fault, proxy_retested                                                (T1)
  engine.rs        new: Engine (schedule, slots, halt, streak, retry, stop job), sweep          (T2)
  engine/tests.rs  new: the Python engine tests, no browser                                     (T2)
  errors.rs        counts_toward_streak: no proxy code counts; PROXY_FAULTS                     (T2)
  mod.rs           pub mod engine                                                               (T2)
  proxy.rs         + PICK lock, new_browser, retest, live probe                                 (T3)
  profile.rs       entry() visible to proxy.rs                                                  (T3)
  web/mod.rs       Session::closed (the pump's end)                                             (T4)
  runner.rs        db() plainly opens the store; the import states go                           (T0)
                   make (queue), browser row at run start, closed signal, proxy fault,
                   engine(), start; RunState, RUN, begin_job, make_breeze, new_account,
                   used_elsewhere go                                                            (T4)
  runner/tests.rs  db_reports_importing_then_the_same_failure goes                              (T0)
                   the make / closed / proxy-fault tests; the one-at-a-time tests go            (T4)
  runner/live.rs   live_import goes                                                             (T0)
                   live_breeze kept compiling on make + engine (T4); live_engine, live_sweep    (T7)
  pages.rs         run_state, job_stop, retry_accounts, test_proxy,
                   settings max_browsers, fail_streak_stop                                      (T5)
src/creator/
  types.ts (comment only), mock.ts, i18n/{en,ru,zh}.json (one sentence), pages/AccountsPage.tsx, widgets/AccountPeek.tsx,
  settings/CreatorSettingsSection.tsx                                                           (T6)
docs/creator/
  design/index.md    the streak sentence under the error table                                  (T2)
  cutover.md         the manual cutover                                                         (T0)
  windows-build.md   live_import's paragraph goes (T0); the new live tests (T7)
  working-process.md §7 risky areas: + creator/engine.rs                                        (T7)
```

---

### Task 0: Remove the Python import; the cutover is a file copy

**Files:**
- Modify: `src-tauri/src/creator/store/mod.rs`, `runner.rs` (`db()` and its static only), `runner/tests.rs`, `runner/live.rs`, `docs/creator/cutover.md`, `docs/creator/windows-build.md` ("Import rehearsal": the `live_import` line goes, the heading becomes "Store checks")

**Interfaces:**
- Deleted from `store/mod.rs`: `python_db_path`, `ImportReport`, `import_python`, `import_python_inner`, `open_or_import`, the `creator-import.json` write, and every test of them (the fixture helper too if nothing else uses it).
- `runner::db()`: `DbState`, `IMPORTING` and the import thread go. It is again a locked static `Option<Arc<Db>>` and `Db::open(&store::db_path()?)` on the first call (a missing file becomes a fresh v24 store). An open error is returned and the next call tries again. `reset_db_for_test` stays.
- `runner/tests.rs`: `db_reports_importing_then_the_same_failure` goes. `runner/live.rs`: `live_import` and its header lines go; `live_perf`, `live_profiles_check` and `live_open_imported` stay as they are (they open the store at `CREATOR_LIVE_OUT`, never the import).
- Kept untouched: `idx_mail_sender` in `Db::open` with `open_adds_the_sender_index_and_stays_v24` and `free_count_scans_the_covering_index`; `import_basics` and its command.
- `docs/creator/cutover.md`, rewritten to the manual steps (no hosts or user names):
  1. Preconditions as today (every provider ported, the stock installer kept, free disk, the owner present).
  2. Stop every job in the Python app, wait until nothing is making, close it normally. Check that no `ghidorah.sqlite-wal` is left beside the file, or that it is 0 bytes: a clean close leaves none. If one stays, open and close the Python app again; only if that fails, copy the `-wal` along (never the `-shm`).
  3. Close stock ShardBrowser normally.
  4. Back up the three folders as today, with the count and size check.
  5. Install ours as today (never "delete app data").
  6. Before the first start, with our app not running: move any existing `%LOCALAPPDATA%\shardx-launcher\creator.sqlite` aside, then copy `%LOCALAPPDATA%\ghidorah-creator\ghidorah.sqlite` to `%LOCALAPPDATA%\shardx-launcher\creator.sqlite`.
  7. Start our app. The first open adds the sender index (seconds on a large mail table) and the startup sweep fails rows Python left mid-run (0 expected after step 2). Check the Accounts, Emails and proxy counts against the Python app's last screen, and Shard's profile count against the pre-cutover one.
  8. Verify live as today. 9. Retire Python as today. 10. Rollback as today, plus: delete our `creator.sqlite`.
  The lines about the background import, "Importing the Python database…" and `creator-import.json` go.

- [ ] **Step 1: Failing tests** (runner, under `test_root`, `migrate::serial()`): `db_opens_a_copied_python_store_as_it_is`: a v24 file with rows placed at `store::db_path()`, and a v24 file also sitting at Python's old default place -> `db()` is `Ok` at the first call, the rows are the placed file's, `idx_mail_sender` exists, `user_version` is 24, and nothing but the store and its `-wal`/`-shm` was written beside it. `db_makes_a_fresh_store_when_none_is_there`: no store, a Python file at its old place -> `db()` is `Ok` at once with an empty store (today it answers "Importing…").
- [ ] **Step 2:** Delete as listed; rewrite `cutover.md`.
- [ ] **Step 3:** Box: `cargo test … creator::` PASS (the removed tests are gone, the index tests still run); `cargo test --lib creator::runner::live -- --list` no longer shows `live_import`; a search of `src-tauri/src`, `src` and `docs/creator` for `open_or_import`, `import_python`, `python_db_path`, `creator-import` and `IMPORTING` finds nothing; rustfmt clean.
- [ ] **Step 4:** Commit `Creator: no Python import in the app; the cutover copies the store by hand`.

### Task 1: Store: the queue's rows and the proxy marks

**Files:**
- Modify: `src-tauri/src/creator/store/mod.rs`
- Test: store tests module

**Interfaces** (all on `Db`; each write is one statement or one transaction):
- `pub struct Job { pub id: i64, pub platform: String, pub choice: String, pub count: i64, pub proxy_mode: String, pub proxy_kind: String }`; `job_get(id) -> Result<Option<Job>>`.
- `breeze_free_where(provider: &str)`: today's `breeze_free_outlook_where` with the provider as an argument (`outlook` keeps its token and client-id conditions; nothing else changes). `breeze_free_outlook*` stay as thin callers.
- `queue_job(platform, route, choice, proxy_mode, proxy_kind, mailboxes: Mailboxes) -> Result<(i64, Vec<i64>)>` with `enum Mailboxes<'a> { These(&'a [i64]), FreeOutlook(i64) }`. **One `BEGIN IMMEDIATE` transaction** (Python `planner.make`): for `FreeOutlook(n)` select the `n` oldest free ids inside it; fewer than `n` -> `Err("Only {k} free emails on this route")`, nothing inserted. Insert the `jobs` row (`choice`, `count`, `proxy_mode`, `proxy_kind`), then per mailbox Python's `_queue`: a `failed` row of the same mailbox, platform and dedupe key is queued again in place (today's `account_queue` body, `retry_count=0`), else a new `queued` row. Returns the job id and the account ids. `account_queue` stays for its callers until Task 4 removes them.
- `accounts_queued() -> Result<Vec<(i64 /*id*/, i64 /*mailbox*/, String /*platform*/)>>`: `status='queued' AND frozen=0 ORDER BY id` (`idx_accounts_status`).
- `account_requeue(id) -> Result<bool>` (Python `_requeue`): only when the row exists, is not frozen and is `failed`: `status='queued'`, `error`, `error_code`, `step`, `step_note`, `screenshot`, `started_at`, `finished_at` NULL, `retry_count+1`. False when nothing changed.
- `job_cancel_queued(job) -> Result<i64>` (D-M5-1): the job's `queued` rows -> `status='failed'`, `error='cancelled'`, `error_code='INTERRUPTED'`, `finished_at=now`. Returns how many.
- `sweep_live() -> Result<i64>` (Python `mark_interrupted`, the service half): `UPDATE accounts SET status='failed', error='Interrupted', error_code='INTERRUPTED', finished_at=?, updated_at=? WHERE frozen=0 AND status IN ('queued','running','waiting','cancelled','later')`.
- `jobs_live() -> Result<Vec<serde_json::Value>>`: for every job that has a `queued` or `running` account (ids found through `idx_accounts_status`, then one `GROUP BY job_id` over those jobs only): `job_id`, `platform`, `total` (its rows), `linked`, `making` (queued + running), `failed`, `step` (the newest running row's `step_note`). Never a scan of all jobs.
- `setting_int(key, default, lo, hi) -> Result<i64>`: unreadable -> default, then clamped (Python `settings.int`).
- Proxy marks (D-M5-3), on the existing columns only:
  - `proxy_fault(id, reason: &str) -> Result<()>` (Python `record_failure`): one statement, `status='failing'`, `reason`, `checked_at=now`, `fail_streak=fail_streak+1`.
  - `proxy_retested(id, ok: bool, reason: Option<&str>)`: ok -> `status='healthy'`, `fail_streak=0`, `reason=NULL`, `checked_at=now`; not ok -> `status='failing'`, `reason`, `checked_at=now`.

- [ ] **Step 1: Failing tests:**
  - `queue_job_takes_the_oldest_free_outlook_and_marks_them_taken`: 5 free, `FreeOutlook(3)` -> 3 queued rows on the 3 oldest; `breeze_free_outlook_count` is 2; the job row holds choice, mode and kind.
  - `queue_job_short_inserts_nothing`: 2 free, `FreeOutlook(3)` -> the error text above; no job row, no account row.
  - `queue_job_requeues_a_failed_row_in_place`: id, secrets and extra kept; `retry_count` 0.
  - `concurrent_makes_never_share_a_mailbox`: 10 free; two threads `FreeOutlook(5)` -> 10 distinct mailboxes. 8 free -> one succeeds, the other gets the short error with nothing inserted.
  - `accounts_queued_is_oldest_first_and_skips_frozen`.
  - `requeue_only_touches_a_failed_unfrozen_row`: linked, running, frozen and missing -> false, row unchanged; failed -> cleared fields, `retry_count+1`.
  - `cancel_queued_leaves_running_and_other_jobs_alone`; the cancelled rows' code is `INTERRUPTED` and `errors::fail_info` says retryable.
  - `sweep_fails_only_unfrozen_live_rows`: queued, running, waiting -> failed `Interrupted` with code and `finished_at`; linked, failed and a frozen running row unchanged; returns 3.
  - `jobs_live_lists_only_jobs_still_making`: counts per job; a finished job is not listed; `EXPLAIN QUERY PLAN` shows no scan of `jobs`.
  - `setting_int_defaults_and_clamps`.
  - `proxy_fault_marks_failing_with_its_reason` (status, reason, `checked_at`, count + 1); `proxy_fault_is_an_atomic_increment` (Python `test_record_failure_is_atomic_increment`: two threads, 50 each -> 100).
  - `retest_clears_or_sets_failing`.
- [ ] **Step 2:** Box: `cargo test --manifest-path src-tauri/Cargo.toml creator::store` -> the new tests fail.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Box: `cargo test … creator::` PASS; `rustfmt --edition 2021 --check` clean; `new_db_is_v24_and_matches_python_tables` still passes (no schema change).
- [ ] **Step 5:** Commit `Creator: store functions for the account queue and the proxy marks`.

### Task 2: The engine (parallel with Task 3)

**Files:**
- Create: `src-tauri/src/creator/engine.rs`, `src-tauri/src/creator/engine/tests.rs`
- Modify: `src-tauri/src/creator/mod.rs` (`pub mod engine;`), `errors.rs`, `docs/creator/design/index.md` (the sentence listing the codes that never count toward stopping a queue gains the four proxy codes; the halted-queue rows say the queue goes on with the next Make or Retry, and Pause all is marked not built)

**Interfaces:**
- `errors.rs`: `counts_toward_streak` loses its `dead_code` allow and answers false for every code whose `fail_info` group is `proxy` (adds `PROXY_REFUSED`, `PAGE_LOAD_FAILED`, `GEO_BLOCKED`, `PROXY_CHECK_FAILED`; owner, D-M5-3; Python counts these four). `pub const PROXY_FAULTS: &[&str] = &["PROXY_CHECK_FAILED", "PROXY_REFUSED"]`: the clear proxy errors, the only ones that mark a proxy failing (owner; Task 4 reads it). `PAGE_LOAD_FAILED` is not one: Shard does not sideline a proxy for a page that did not load, and a site outage must not drain the pool; it only stays out of the streak. `GEO_BLOCKED` (the site refuses the country) and `PROXY_UNUSABLE` / `NO_PROXY` (no run happened) are not faults of a proxy.
- `pub type RunFn = Arc<dyn Fn(i64) -> BoxFuture<'static, ()> + Send + Sync>`: runs account `id` and writes its row. `pub type ReadyFn = fn() -> bool`: false while Shard's engine is downloading, updating or swapping.
- `pub struct Engine { db: Arc<Db>, run: RunFn, ready: ReadyFn, st: Mutex<State> }`; `State { halted: HashMap<String, Halt>, streak: HashMap<String, u32>, running: HashMap<i64 /*account*/, i64 /*mailbox*/>, waiting_ready: bool }`; `#[derive(Serialize)] pub struct Halt { pub n: u32, pub error: String, pub code: String }`.
- `Engine::new(db, run, ready) -> Arc<Engine>`.
- `pub fn sweep(db: &Db) -> Result<i64>`: `db.sweep_live()`, and an `events` row `warn` / step `cleanup` / `"{n} interrupted by the last close"` when n > 0.
- `schedule(self: &Arc<Self>)` (Python `_schedule`), under the state lock, no await. Not `ready()` -> if queued rows exist and `waiting_ready` is false, set it and spawn one task that sleeps 5 s, clears it and calls `schedule`; return. `cap = db.setting_int("max_chrome", 3, 1, 20)`. For each `accounts_queued()` row: stop at `running.len() >= cap`; skip a halted platform, an id already running, a mailbox already in `running`; else insert into `running` and `tokio::spawn` the run.
- The spawned run: `tokio::spawn(run(id))` awaited through its `JoinHandle`, so a panic is an `Err`, then `done(id)`.
- `done(id)` (Python `_done` + `_tally`): remove from `running`. Read the row: still `queued`/`running`/`waiting` (the run panicked or returned without writing) -> fail it `"run: task ended"` (code `APP_ERROR`) through `account_update` with `finished_at`. Then tally by platform: `linked` -> streak 0. `failed` whose `error_code` is NULL or `counts_toward_streak(code)` -> streak + 1; with `limit = setting_int("fail_streak_stop", 3, 0, 50)`: when `limit > 0`, `n >= limit` and the platform is not halted yet -> `halted[platform] = Halt { n: limit, error, code (or "OTHER") }` and an `events` row `warn` / step = platform / `"paused after {limit} failures in a row"`. Anything else does not count. Then `schedule`.
- `clear_halt(platform)` (Python `resume_queue`'s body): drop that platform's halt and streak. Called by a new Make of that platform (Task 4's `start`) and by `retry`; there is no Resume command and no pause (owner; manager, 2). `kick()`: `schedule`.
- `retry(ids: &[i64]) -> (usize, Vec<String>)` (Python `_retry_service` without parents): per distinct id: missing, frozen, in `running`, or not `failed` -> skipped silently (the caller counts). `error == "mailbox gone"` -> refused `"{label} not retried: mailbox gone — its inbox was deleted, make a new account"`; a code in `NOT_RETRYABLE` -> refused `"{label} not retried: {error}"`; each refusal is also an `events` row `warn` / step `retry` on the mailbox. Else `account_requeue` and `clear_halt` of that row's platform. Then `kick`. `label` = `providers::label(platform)`. A retry that queued nothing clears nothing.
- `stop_job(job) -> Result<i64>`: `db.job_cancel_queued(job)`, then `kick` (D-M5-1).
- `halted() -> Vec<(String, Halt)>` (sorted by platform); `has_task(id) -> bool`.
- ponytail note in the file: scheduling reads every queued row per pass (Python does); page it if queues reach tens of thousands.

- [ ] **Step 1: Failing tests** (`engine/tests.rs`; `temp_db`, rows inserted directly, a fake `RunFn` that marks the row `running`, waits on a per-id gate, then writes the scripted `(status, error)` with its `error_code`; no browser, no network). Ports, Python names kept:
  - From `test_engine.py` / `test_max_chrome.py` / `test_scheduler.py` / `test_busy.py`: `concurrency_respected` (cap 2, 5 queued -> 2 running, 3 queued; one ends -> the next starts); `raising_cap_kicks_scheduler` (cap 1 -> 3 and `kick`); `one_running_account_per_email` (two platforms' rows on one mailbox run one after the other); `frozen_account_never_starts_until_unfrozen`; `schedule_tolerates_vanished_row`; `queued_rows_start_oldest_first`; `retry_only_failed`; `retry_skips_an_account_still_running`; `retry_respects_cap`; `retry_refuses_what_no_rerun_fixes` (ACCOUNT_EXISTS and mailbox gone: row untouched, the event text above).
  - Ours: `slot_released_when_the_run_panics`; `a_run_that_leaves_its_row_live_is_failed` (`run: task ended`, slot free, the next starts); `nothing_starts_while_the_engine_is_not_ready_then_it_goes_on` (ready flips to true; the 5 s wait is a field the test shortens); `stop_job_cancels_only_its_queued_rows` (running ones finish, the other job runs on, no streak change); `sweep_runs_first` (`sweep` then `schedule`: the rows a crash left are failed, not started).
  - From `test_fail_streak.py`: `queue_halts_after_n_failures` (setting 2, cap 1, 4 queued -> 2 failed, 2 still queued, `Halt { n: 2, error: "boom", code: "OTHER" }`, one event); `success_resets_streak`; `interrupted_not_counted`; `mailbox_gone_not_counted`; `unusable_browser_proxy_not_counted`; `refusals_before_any_browser_never_halt` (`No browser`, `Email has no address`, `No healthy proxy for a new browser`); `halt_is_per_queue`; `inflight_results_count_but_halt_once` (cap 3, setting 2, three fail together -> streak 3, halted once, one event); `zero_never_halts`; `default_pauses_after_three` (no setting row -> 3); `window_closed_still_counts`; `halt_records_code`.
  - The halt's only ways out (owner): `a_retry_clears_the_halt_and_the_queue_goes_on` (halted with 2 still queued; `retry` of one failed row of that platform -> no halt, streak 0, the retried row and the 2 queued ones run); `clear_halt_then_kick_starts_the_queued_rows` (what a new Make does); `a_retry_that_queues_nothing_keeps_the_halt` (a not-retryable row); `a_retry_on_another_platform_keeps_the_halt`.
  - D-M5-3: `proxy_failures_never_halt` (setting 1; `page load: net::ERR_PROXY_CONNECTION_FAILED`, `proxy location lookup failed` and a `PROXY_REFUSED` text, three rows each -> all failed, nothing halted, streak 0). In `errors.rs`: `no_proxy_code_counts_toward_the_streak` (every code whose group is `proxy`), and the design table's sentence lists the same codes.
- [ ] **Step 2:** Box: `cargo test … creator::engine` -> FAIL (does not compile).
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Box: `cargo test … creator::` PASS, run `creator::engine` 5 times in a row (ordering bugs show as flakes); rustfmt clean.
- [ ] **Step 5:** Commit `Creator: the account queue's scheduler (slots, fail streak, retry, stop, sweep)`.

### Task 3: Atomic picks and Retest (parallel with Task 2)

**Files:**
- Modify: `src-tauri/src/creator/proxy.rs`, `profile.rs` (`entry` becomes `pub(super)`, nothing else)
- Test: `proxy.rs` tests module

**Interfaces:**
- `static PICK: Mutex<()>`: held across list -> choose -> `proxy_touch` in `pick_proxy` (its ponytail note goes; a new one says: process-wide lock, a transaction if a second process ever writes the store).
- `pub fn new_browser(db: &Db, mailbox_id: i64, mode: &str, kind: &str) -> Result<i64>` (Python `create_browser`): under `PICK`, pick, then `browser_create` with the picked proxy; so `least` counts the row before the next pick. `NoHealthyProxy` -> the error, no row.
- **Retest, one proxy, on demand** (no loop, no check on pick): `pub type Probe = Arc<dyn Fn(Proxy) -> BoxFuture<'static, bool> + Send + Sync>`; `pub fn live_probe() -> Probe`: `profile::entry(&p)`, then `tokio::time::timeout(10 s, crate::space::proxy::geo_check(&entry, None))` is `Ok(Ok(_))`. `pub async fn retest(db: &Db, id: i64, probe: &Probe) -> Result<bool>`: a missing row -> `Err`; `proxy_retested(id, ok, (!ok).then_some("retest failed"))`; answers `ok`. The probe's own error text is never stored or logged.
- No background check and no mover (owner): `usable` already keeps a `failing` proxy out of new picks, and an account already on one fails `PROXY_UNUSABLE` until Retest passes or its proxy is changed (M7).

- [ ] **Step 1: Failing tests** (fake probes; no network):
  - `concurrent_picks_never_share_a_proxy`: 8 proxies, 8 threads, `rotate` -> 8 distinct.
  - `concurrent_new_browsers_spread_in_least_mode`: 4 proxies, 4 threads `new_browser(.., "least", "")` -> 4 distinct proxies, 4 browser rows.
  - `new_browser_without_a_usable_proxy_makes_no_row`; `new_browser_in_mode_none_has_no_proxy`.
  - `a_failing_proxy_is_picked_again_only_after_retest_passes`: one `proxy_fault` -> `pick_proxy` skips it; `retest` with a false probe -> still skipped; with a true probe -> picked again, count 0.
  - `retest_stores_no_probe_text`: the stored reason is `retest failed`; no event is written.
  - The existing `proxy.rs` tests unchanged.
- [ ] **Step 2:** Box: `cargo test … creator::proxy` -> FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Box: `cargo test … creator::` PASS; rustfmt clean; `check-seams` shows no new file (nothing in `space/` is touched or widened).
- [ ] **Step 5:** Commit `Creator: proxy picks that cannot collide, Retest on Shard's check`.

### Task 4: Runner on the queue: Make, the browser row, the closed browser, the proxy fault

**Files:**
- Modify: `src-tauri/src/creator/runner.rs`, `runner/tests.rs`, `web/mod.rs`, `runner/live.rs` (only what keeps `live_breeze` compiling and working)
- Test: `runner/tests.rs`, `web/mod.rs` tests

**Interfaces:**
- **Closed browser, no poll** (`web/mod.rs`): `Session` gains `closed: Arc<AtomicBool>`; the pump task sets it after its loop ends (`while handler.next().await.is_some() {}` then `closed.store(true)`); `pub fn closed(&self) -> Arc<AtomicBool>`. `Drop` still aborts the pump (an aborted pump sets nothing). In `attempt`: `t.closed = opened.session.closed()`. Every wait of the flow already ends on that flag; the result is `failure_text` -> `Interrupted`, no screenshot, as today. The `Browsers` trait is unchanged. The ponytail note on `run_account` goes. `web/` is a risky area: this adds one flag and changes no input or stealth code.
- **The browser row at run start** (Python `_run_svc`): in `attempt`, after the mailbox pre-flight (gone, Outlook check, platform mail) and before `browser_proxy`: no browser row for the mailbox -> `proxy::new_browser(db, mb.id, mode, kind)` with the account's job (`job_get(acc.job_id)`; no job: `rotate`, any kind); `NoHealthyProxy` -> the run fails `NO_PROXY` and no browser row is made. So a dead mailbox never uses up a proxy pick, and a Retry after `NO_PROXY` picks again (the old "proxy deleted" note goes).
- **The proxy fault** (D-M5-3), in `run_account` after the final error is known: the code is in `errors::PROXY_FAULTS` and the mailbox's browser row has a proxy -> `db.proxy_fault(proxy_id, code)` and an `events` row `warn` / step `proxy` / `"proxy {id} marked failing: {code}"` (the row id, never the host). A failure at `open` (`proxy location lookup failed`) counts the same way. Any other failure leaves the proxy as it is.
- **Make queues** (Python `planner.make` + `api.make_accounts`): `pub async fn make(db: &Arc<Db>, ele: Option<&EleClient>, count: u32, proxy_mode: &str, route: &str) -> Result<(i64, usize)>` under `static MAKE: tokio::sync::Mutex<()>` (Python `_make_lock`): `check_start` as today; `email:outlook` -> `queue_job(.., Mailboxes::FreeOutlook(count))`; `email:ele` -> `create_mailbox` `count` times, one after another, then `queue_job(.., These(&ids))`; an Ele error after some were made -> `Err("{n} new emails created, nothing queued: {why}")` (Python's text; `1 new email`). `jobs.choice` holds the route.
- `pub fn app_ctx(db: Arc<Db>) -> Result<Ctx<Shard>>`: today's `start` body that builds the `Ctx`; `ele` is `Some` when the Ele address and key are set.
- `pub fn engine() -> Result<Arc<Engine>>`: one engine per opened store (kept beside it; `reset_db_for_test` drops both). Made on first use: `engine::sweep(&db)` first, then `Engine::new(db, run, pages' engine-ready fn)` where `run` builds `app_ctx` and calls `run_account` (a `Ctx` error fails the row with `err_line`).
- `pub async fn start(count, proxy_mode, route) -> Result<(i64, usize)>`: `engine()?`, `make(..)`, then `engine.clear_halt(KEY)` (a new Make clears the platform's halt, owner) and `engine.kick()`. A Make that queued nothing clears nothing.
- **Removed:** `RunState`, `RUN`, `EndRun`, `begin_job`, `make_breeze`, `new_account`, `make_rows_and_run`, `used_elsewhere` (D-M5-2: the run's own pre-flight already fails such a mailbox `ACCOUNT_EXISTS` before a browser opens), and `Db::account_queue` if nothing else calls it.
- `runner/live.rs`: `live_breeze` calls `make` then an `Engine` built on its own `Ctx` and waits until no row of the job is live; `CREATOR_LIVE_MAILBOX_ID` queues exactly that mailbox with `Mailboxes::These`. Same variables, same printed report.

- [ ] **Step 1: Failing tests** (browser-backed ones as today, under `CREATOR_REQUIRE_BROWSER=1`):
  - `web`: `a_session_says_closed_when_its_browser_goes` (the test's own bare engine is killed -> the flag is true within 2 s; false while it runs; a dropped `Session` sets nothing).
  - `a_browser_closed_mid_run_ends_the_account_within_seconds`: the site holds the verify mail back (a 180 s wait); the test's own engine is killed 1 s in -> the run returns in under 5 s with `Interrupted`, code `INTERRUPTED`, no screenshot. This test gets its own engine process, never the shared one.
  - `make_queues_outlook_rows_oldest_first_and_starts_nothing`: rows `queued`, no browser row, no open; replaces `outlook_make_takes_free_mailboxes_oldest_first_and_skips_taken` (its skip cases stay, checked on what `make` queued).
  - `make_on_ele_creates_the_inboxes_then_queues_them`; `ele_down_part_way_queues_nothing_and_says_how_many_were_made` (replaces `ele_down_fails_without_rows…`'s Ele half).
  - `a_run_makes_its_browser_row_and_no_proxy_fails_it_without_one` (replaces the no-proxy half): `NO_PROXY`, no browser row; after a proxy is added, the same row run again links.
  - `a_gone_mailbox_uses_no_proxy_pick`: `last_used_at` of every proxy unchanged.
  - `a_free_mailbox_with_breeze_mail_fails_account_exists_without_a_browser` (replaces `a_mailbox_whose_fresh_inbox_has_breeze_mail_is_skipped_without_rows`, D-M5-2).
  - `two_makes_at_once_never_queue_one_mailbox_twice` (through `make`, 2 tasks).
  - `a_proxy_failure_takes_the_proxy_out_of_picks`: one proxy, `fail_open` giving `proxy location lookup failed`: the account fails `PROXY_CHECK_FAILED`, the proxy is `failing` with that code as its reason, the event names it by id only, the next new account fails `NO_PROXY`; after `proxy::retest` with a true probe it is picked again. `other_failures_never_mark_the_proxy` (`page load: net::ERR_PROXY_CONNECTION_FAILED`, a captcha failure, a mail timeout, `Interrupted`: status and `fail_streak` unchanged).
  - `a_new_make_clears_the_halt` (through `start`'s body on a test engine: halted with queued rows; a Make of the same platform -> no halt, old and new rows run); `a_make_that_queues_nothing_keeps_the_halt` (not enough free emails).
  - `the_store_is_swept_when_it_opens` (under `test_root`, `migrate::serial()`): a store file with a `running` and a `queued` row -> after `engine()`, both failed `Interrupted`; a second `engine()` call sweeps nothing new (a row set `queued` in between stays queued and is run).
  - `the_job_is_seen_from_the_start_and_its_accounts_join_it` rewritten on `make` + `jobs_live`.
  - Removed with their code: `one_run_at_a_time`, `stop_is_checked_between_accounts`.
  - Unchanged and still passing: the happy paths, the failure and secret tests, the pre-flight tests, `a_browser_closed_mid_run_is_interrupted_without_a_screenshot`.
- [ ] **Step 2:** Box: `cargo test … creator::runner creator::web` -> FAIL.
- [ ] **Step 3:** Implement. `pages.rs` is touched only as far as it must to compile (`breeze_run` awaits `start`; `breeze_stop` and `run_state`'s job part become Task 5's); say so in the commit.
- [ ] **Step 4:** Box: `cargo test … creator::` PASS with `CREATOR_REQUIRE_BROWSER=1`; rustfmt clean.
- [ ] **Step 5:** Commit `Creator: Make queues accounts, the run makes its browser row and ends when its browser closes`.

### Task 5: Commands (parallel with Task 6 and Task 7)

**Files:**
- Modify: `src-tauri/src/creator/pages.rs`
- Test: `pages.rs` tests module

**Interfaces** (answers in the `types.ts` shapes; no secret in any):
- `breeze_run {count, proxy_mode, route}` -> `{ job_id, queued }`.
- `job_stop {job_id}` -> the number cancelled. `breeze_stop` goes.
- `retry_accounts {ids}` -> `{ retried, skipped, error? }`: `error` is the first refusal text (`Nothing to retry` for no ids).
- `run_state` -> `{ jobs: db.jobs_live(), halted: [{ platform, n, error, code }], setup }` (the `RunState` type as it is). The halted `error` is the stored (already masked) text.
- `test_proxy {id}` -> the answer of `proxy::retest(.., &live_probe())` (M7's Retest calls it; no page calls it in M5).
- `settings` adds `max_browsers` (`max_chrome`) and `fail_streak_stop`; `settings_set` takes both in `patch`: out of range -> `max_browsers must be in 1..20` / `fail_streak_stop must be in 0..50`, nothing saved; a saved `max_browsers` kicks the engine. A changed `fail_streak_stop` applies from the next failure on and clears no halt.
- Every command that needs the engine gets it through `runner::engine()`; `call` asks for it once at its top and ignores an error there, so the sweep happens with the app's first command (the sidebar's `nav_counts`), not with the first Make.

- [ ] **Step 1: Failing tests:** `run_state_matches_types` (keys of `RunState`, its job and halted rows against `types.ts`, the existing `fields()` helper); `run_state_lists_every_job_still_making`; `job_stop_cancels_queued_rows_of_that_job_only`; `run_state_shows_a_halt_until_a_make_or_retry_clears_it`; `retry_accounts_requeues_and_reports_refusals`; `engine_settings_default_validate_and_save` (3 and 3 by default; 0 and 21 refused for `max_browsers`; 51 refused for `fail_streak_stop`, 0 saved; a stored value from a copied store is answered as it is); `no_command_answer_holds_a_token_or_client_id` extended with the new answers; `the_first_command_sweeps_the_store`.
- [ ] **Step 2:** Box: FAIL. **Step 3:** Implement. **Step 4:** Box: `cargo test … creator::` PASS; rustfmt clean.
- [ ] **Step 5:** Commit `Creator: queue commands (stop a job, retry, engine settings)`.

### Task 6: Pages (parallel with Task 5 and Task 7)

**Files:**
- Modify: `src/creator/mock.ts`, `types.ts` (comments only: `CreatorSettings` says which fields the real backend answers), `i18n/{en,ru,zh}.json` (the one halted sentence), `pages/AccountsPage.tsx`, `widgets/AccountPeek.tsx`, `settings/CreatorSettingsSection.tsx`
- Test: `npm run check` (incl. `i18n.test.ts`), `e2e/creator-mockups.spec.ts` in `dev:mock`

**Interfaces** (Task 5's commands; only what the approved mockups already show; no new text key, one reworded):
- The mock answers the new commands (`job_stop`, `retry_accounts`); `breeze_stop` goes.
- Accounts job card: Stop -> `job_stop { job_id }`, the existing toast. One card per job (already a list).
- Halted card: in the real build its button is "Make accounts" (the existing `creator.make.open` text) and opens the Make dialog; a Make or a Retry of that platform clears the halt, and the card goes at the next `run_state`. The mock keeps its Resume toast. `creator.needs.haltedWhy` says "until you resume it" today: reword it in en, ru and zh to say no new sign-ups start until the person makes or retries {name} accounts.
- The header menu ("Pause all queues", "Close all browsers") stays mock only: Pause all is not built (manager, 2).
- Peek: Retry is real for a failed, retryable account -> `retry_accounts { ids: [id] }`; `retried == 0` -> `toast.error` with the answer's `error`. Log in again, Check, Open browser and the More menu stay mock only.
- Settings: the rows "browsers at once" and "stop a queue after N failures" (0 = off, as the mock) show in the real build and save through `settings_set`; "wait for a code" and "delete the browser of a failed sign-up" stay mock only (Breeze reads neither).
- The run poll (2 s while a job is live) is unchanged; it now also runs while `halted` is not empty, so the card goes without a reload.

- [ ] **Step 1:** Box: `npm run check` and the mockup spec pass on `main` first (baseline).
- [ ] **Step 2:** Wire as above. `npm run check` PASS; `E2E_PORT=4794 npx playwright test e2e/creator-mockups.spec.ts --project=dev` passes (screens unchanged).
- [ ] **Step 3: Dev run on the box (sandboxed, fake data only),** through `dev_bridge` + `npm run dev:mock` with `VITE_CREATOR_BRIDGE`: Make 5 on made-up Outlook rows whose token call is refused with something other than a dead grant (`Outlook unavailable`, counted) -> the halted card shows at 3 with 2 still Making; Retry in Peek on one of the failed rows clears it and the 2 start; a second Make clears it too; Stop on a fresh job takes the queued ones out. Screenshots light/dark for the report, emails blurred.
- [ ] **Step 4:** Commit `Creator: Accounts and Settings on the queue (stop, retry, engine settings)`.

### Task 7: Live harness (parallel with Task 5 and Task 6)

**Files:**
- Modify: `src-tauri/src/creator/runner/live.rs`, `docs/creator/windows-build.md` (one paragraph per new test), `docs/creator/working-process.md` (§7 risky areas: `src-tauri/src/creator/engine.rs`)

**Interfaces:** all `#[ignore]`, the same sandbox setup as `live_breeze`, printing statuses, codes, seconds and counts only (never an address, token, proxy or path of the owner's data).
- `live_engine`: the `live_breeze` variables plus `CREATOR_LIVE_CONCURRENCY=K` (saved as `max_chrome`), `CREATOR_LIVE_OUTLOOK_N` and `CREATOR_LIVE_ELE_N` (one job each, queued together), and one optional scenario:
  - `CREATOR_LIVE_STOP_AFTER=n`: after `n` accounts ended, `stop_job` on the first job; prints how many it cancelled.
  - `CREATOR_LIVE_CLOSE_AFTER_S=s`: `s` seconds after the first browser opened, stops one running account's profile through `shard::stop` (what Shard's Close does) and prints how many seconds its row took to read `Interrupted`.
  - `CREATOR_LIVE_EXIT_AFTER_S=s`: `s` seconds after the first browser opened, prints the live row count and ends the process with `std::process::exit(0)`: the app closing mid-run, by its own exit, never a kill.
  - Every second it samples `Tracker::shared().running().len()` and prints the maximum at the end; it asserts the maximum is at most K, that no two rows of the run share a mailbox, and, in `rotate` with at least as many usable proxies as accounts, that no two new browsers of the run share a proxy.
  - The report per account: status, code, seconds, queue wait; then linked / failed, the halted state if the queue stopped, and the ids of proxies the run marked failing.
- `live_sweep`: `CREATOR_LIVE_OUT`. Opens the store, prints the live row count, runs `engine::sweep`, prints how many it failed and asserts none is live. With `CREATOR_LIVE_RETRY=1` (and the `live_breeze` engine variables) it then retries those rows through an `Engine` and prints their results.

- [ ] **Step 1:** Write the tests and the doc paragraphs.
- [ ] **Step 2:** Box: `cargo test --lib creator::runner::live -- --list` shows them; `cargo test … creator::` PASS; rustfmt clean.
- [ ] **Step 3:** Commit `Creator: live tests for the queue (slots, stop, close, sweep)`.

### Task 8: Box gate and live gate

- [ ] **Step 1: Full box gate** on the branch (`box-workflow.md` flow), against `main`'s baseline: `npm run check`; `cargo test --locked` (in `src-tauri`, full); rustfmt check; `node --test scripts/creator/*.test.mjs`; `node scripts/creator/check-seams.mjs` (no new file, no new seam); the e2e lane from §7.
- [ ] **Step 2: Whole-branch review** by a fresh reviewer (opus or fable) before any live run.
- [ ] **Step 3: Live gate, ONLY after the owner's explicit approval, owner present.** In the sandbox, under `box-env.ps1`, in the desktop session (scheduled task), on the sandbox store (a copy of the rehearsal store; never the production database, never stock Shard's folders). Real Ele, real never-used active Outlook mailboxes of that store, real proxies. Rules: back up first, one sample first, stop only by letting a run finish or by the harness's own scenario, never a kill.
  1. **Back up** the sandbox `creator.sqlite` (with `-wal`) and note its account count.
  2. **One sample:** `live_engine` with 1 Outlook account at K=1. Expected `linked`. Stop here on anything unexplained.
  3. **The mixed run:** 20 accounts at K=3, 10 Outlook and 10 Ele, with `CREATOR_LIVE_STOP_AFTER=12`. Expected: never more than 3 browsers, no shared mailbox, no shared proxy; after Stop the first job's waiting accounts read stopped and nothing more of it starts, the running ones and the other job finish.
  4. **Forced close:** 6 accounts at K=3 with `CREATOR_LIVE_CLOSE_AFTER_S=40`. Expected: that account `Interrupted` within 5 s, its slot taken by the next, the streak not raised by it. Then the same once more with the owner closing one browser window by hand.
  5. **A dead proxy:** on a store copy with every proxy turned off except one the owner knows is dead, 4 accounts at K=3 in mode `rotate`. Expected (when the failure is the proxy check or a refusal, not a page that did not load): the accounts that started on it fail with a proxy error code, the queue is NOT halted, the proxy is marked failing at the first failure, and every account started after that fails `NO_PROXY`. Skip this step if no dead proxy is at hand; the fixture tests cover the rule.
  6. **Close mid-run and sweep:** 6 accounts at K=3 with `CREATOR_LIVE_EXIT_AFTER_S=40`. Then `live_sweep`: the rows it fails equal the live count printed at exit, none is live. Then `live_sweep` with `CREATOR_LIVE_RETRY=1`: every row ends linked or with an explained code; a half-made account that Breeze Blue already knows is reported as it comes (the sign-in path or `ACCOUNT_EXISTS`).
  7. **The real app, only if the owner gives a window with stock ShardBrowser closed** (same app id, single instance; it stops the Python app's runs meanwhile): our dev build under `box-env.ps1`: Make 6 at 3, Stop on the job card, Retry in a Peek, "browsers at once" and "stop a queue after N failures" changed in Settings, Shard's Close all, then quit the app through Shard's own prompt mid-run and start it again: the rows read Interrupted. Without that window this step is reported as not verified and is done at M8's rehearsal.
  8. **Report table:** per account its result, code, seconds; per scenario expected vs seen; the maximum browsers at once; the linked rate. Pass rule: every scenario's expectation holds and every failed account is explained by its code. Results go to the owner report, never the repo.
- [ ] **Step 4:** Report to the owner (result, test count, screenshots, what is unverified). Merge only on "merge".

---

## Execution notes

- **Order:** T0 -> T1 -> (T2 ∥ T3) -> T4 -> (T5 ∥ T6 ∥ T7) -> T8. T0 and T1 both edit `store/mod.rs`, and T0, T4 and T7 all edit `runner.rs` or `runner/live.rs` and `windows-build.md`, so those run one after the other, never side by side. No two parallel tasks share a file: T2 owns `engine.rs`, `engine/tests.rs`, `errors.rs`, `mod.rs`, `docs/creator/design/index.md`; T3 owns `proxy.rs` and `profile.rs`; T5 owns `pages.rs`; T6 owns `src/creator/**`; T7 owns `runner/live.rs`, `windows-build.md` and `working-process.md`. T4 (alone) owns `runner.rs`, `runner/tests.rs`, `web/mod.rs` and touches `live.rs` and `pages.rs` only to keep them compiling, before T5 and T7 start. T6 may start as soon as T4 is in; its Step 3 needs T5.
- **Models:** T0 sonnet, reviewer opus (store, and a runbook whose wrong step costs the production data). T1 sonnet, reviewer opus (store is a risky area). T2 opus, reviewer fable or opus (the scheduler is the core risk). T3 sonnet, reviewer opus. T4 opus, reviewer opus or fable (`runner.rs`, `web/` and the live flow). T5 sonnet. T6 sonnet. T7 sonnet. Two fix rounds not enough -> a higher model.
- Each task: implementer -> fresh reviewer -> fix rounds, in its own worktree. Push a task branch to `origin` only on the owner's word (the box fetches from GitHub); until then it can be reviewed on the Mac but its gates wait.
- Risky areas touched: `store/` (no schema change); `web/mod.rs` (one flag on `Session`); `runner.rs` changes how a run starts and ends, hence the live gate. `shard.rs` and the provider flow are NOT changed.

## Exit criteria

- The Python engine, fail-streak and scheduler tests named in Tasks 1-2 are ported and green on the box; the full box gate shows no new failure against `main`.
- No seam, upstream file or dependency added; the store is still v24 (`new_db_is_v24_and_matches_python_tables`).
- No import code is left in the app; `cutover.md` describes the manual copy; a copied v24 store opens as it is.
- No background proxy check exists; no proxy code counts toward a streak; a run's clear proxy error (never a page load) takes its proxy out of picks and Retest brings it back (tests of Tasks 1-4).
- A halted platform goes on with the next Make or Retry of that platform, and with nothing else.
- Live gate steps 1-4 and 6 pass with the owner present; steps 5 and 7 pass or are listed as not verified.
- `runner.rs` holds no single-run gate and none of the three M5 ponytail notes (watcher, sweep, pick and touch).

## Risks

- **A half-made account after a close:** the sweep cannot know whether Breeze Blue already made the account. Retry reuses the saved password and the flow's own sign-in path; if the site says the email exists and sign-in fails, the row ends `ACCOUNT_EXISTS` (not retryable). Live gate step 6 shows which.
- **Queued accounts do not survive a restart** (Python parity): after a crash a big queue is a list of Interrupted rows, and Retry is one at a time until M7's bulk bar.
- **A failing proxy has no button until M7:** the mark is set in M5 (on `PROXY_CHECK_FAILED` and `PROXY_REFUSED` only), but Retest and Change proxy are on M7's pages. Until then a marked proxy stays out of new picks and accounts already on it fail `PROXY_UNUSABLE`; `test_proxy` is reachable only through the dev bridge. If the usable pool shrinks before M7, the smallest fix is M7's Retest brought forward (the command exists).
- **A dead proxy that only shows as page loads failing:** it is not marked (owner: a site outage must not drain the pool), so it stays in the rotation and its runs fail `PAGE_LOAD_FAILED` without stopping the queue. In `rotate` it comes round once per pass of the pool. Most dead proxies fail Shard's launch check first (`PROXY_CHECK_FAILED`) and are marked there; watch the mixed run for one that does not.
- **`GEO_BLOCKED` no longer counts toward the streak** (Python counted it by an earlier owner ruling for Grok). Nothing produces it before M6; raise it again with Grok.
- **A closed tab, not a closed browser:** closing only the run's tab while another window keeps the browser alive does not drop the connection; the run then ends on its next page call (`window closed during …`, counted), as today.
- **Mail calls in parallel:** K runs poll Outlook at once from one IP (D-M3-5). Throttling shows as `MAIL_API_DOWN`, which counts toward the streak and may stop the queue: correct, but watch for it in the mixed run.
- **`max_chrome` and `fail_streak_stop` on copied data:** the cutover brings Python's saved values; the defaults (3 and 3) apply only to a fresh store.
- **The manual copy has no checks of its own:** a file copied while the Python app is open, or without a `-wal` that still held rows, opens short of the newest rows or not at all; `Db::open` refuses any version but 24. `cutover.md`'s clean-close step and count check are the guard, and the backup is the way back.
- **Stop then Make on Ele:** a stopped Ele account's inbox is not reused by the next Make (no "free Ele first" yet); Retry on the row uses it.
