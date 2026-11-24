# Creator M5: engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make accounts run as a queue: Make queues them, a fixed number run at once, a failure streak stops a platform's queue until Resume, Pause all holds new starts, Retry queues a failed account again, a start of the app fails what a crash or close left half-done, a browser closed mid-run ends its account at once, and a background check keeps dead proxies out of new accounts. Still Breeze Blue only.

**Architecture:** The queue is the `accounts` table, as in Python: a row with `status='queued'` waits, and one function (`Engine::schedule`) is the only place a run starts. `engine.rs` (new) holds the scheduler and its in-memory state (paused, halted platforms, streak counts, running ids); it knows nothing about browsers: it gets "run account `id`" as a function and reads the row when that ends, so its tests need no browser. `runner.rs` keeps the one-account run (`run_account`) and gains Make-as-queue, the browser row made at run start, and the close watcher. `proxy.rs` gains the checker and makes pick+touch (and pick+browser row) atomic. `pages.rs` exposes the commands the existing pages call. No new dependency, seam or upstream file; schema stays v24.

**Tech Stack:** Rust (tokio, rusqlite 0.32, already in use; Shard's own `space::proxy::geo_check` for the proxy check), React + TS (Shard's UI kit).

**Spec:** `docs/plans/2026-11-22-creator-m0-m2.md` (roadmap M5, Global Constraints, D16, D17), `docs/plans/2026-11-23-creator-m3-outlook.md` (pre-flight, platform-mail rule), `docs/plans/2026-11-24-creator-m4-data.md` (no startup sweep yet), `docs/creator/design/index.md` (job card, halted queue card, Pause all, the error table's streak column), `docs/creator/working-process.md` §7, `docs/creator/box-workflow.md`, `docs/creator/windows-build.md`. Python source of truth (read-only): `ghidorah_creator/engine/engine.py` (`_schedule`, `_done`, `_tally`, `_run_svc`, `_set_paused`, `resume_queue`, `mark_interrupted`, `_retry_service`, `_requeue`, `_cap`, `shutdown`), `engine/planner.py` (`make`, `_queue`, `_taken`), `engine/account.py` (`create_browser`), `engine/account_service.py` (`run_service_account`: the closed-window checks), `engine/errors.py` (`Code.streak`, `Code.retryable`), `proxy/checker.py` (whole file), `proxy/assign.py` (`pick_proxy`), `store/models.py` (`DEFAULTS`, `record_failure`), `api.py` (`get_state`, `pause_all`, `resume_all`, `resume_queue`, `retry_accounts`, `make_accounts`, the `max_chrome` and `fail_streak_stop` ranges), tests `tests/test_engine.py`, `test_fail_streak.py`, `test_scheduler.py`, `test_max_chrome.py`, `test_busy.py`, `test_proxy_checker.py`, `test_proxy_assign.py`.

---

## Facts and constraints (verified while planning)

- **Today's run is one at a time.** `runner.rs` has one global `RUN: RunState` (`begin` refuses a second run), `begin_job` + `make_breeze` make accounts one after another, and "Stop" is a flag read between accounts. Its notes name what M5 owes: no close watcher, no startup sweep (`run_account`), pick and touch are two statements (`proxy::pick_proxy`), a rerun after a failed pick reads "proxy deleted" (`make_rows_and_run`).
- **Python's queue is the accounts table.** Make inserts the job and its `queued` rows in one transaction; `_schedule` starts queued rows (oldest id first) while fewer than `max_chrome` run; a run's end calls `_tally` then `_schedule`. Nothing else starts a run. The halted and paused state is memory only and is gone after a restart.
- **Python values, copied:** `max_chrome` default 3, range 1..20; `fail_streak_stop` default 3, range 0..50, 0 = never stops; closed-window poll 1 s; proxy check every 300 s, one proxy at a time, 10 s per check, slow above 2000 ms. `LIVE = queued, running, waiting`.
- **Retry is by hand in Python.** No account is ever retried by itself; `retry_count` only counts the person's retries. M5 adds no automatic retry.
- **A restart empties the queue in Python.** `mark_interrupted` fails every unfrozen row left `queued`, `running` or `waiting` as `Interrupted` (retryable, never counted toward a streak). Queued rows do not resume by themselves: no browser opens at app start without the person asking.
- **Streak rule exists already:** `errors::counts_toward_streak` (marked `dead_code` until now) and `NOT_RETRYABLE` match Python's `Code.streak` / `Code.retryable` and the design's error table.
- **A dead app leaves no browser.** Shard ties every browser to the launcher process (`process/tie.rs`: a job object with kill-on-close on Windows), and `shard::open_session` already stops a profile Shard still tracks before a run. So Python's `stop_leftovers` has nothing to do here and is not ported.
- **Closing is Shard's.** Our profiles are ordinary Shard profiles (D17): Close all, the quit prompt and a crash are Shard's own code. A browser Shard no longer runs already reads `Interrupted` (`shard::failure_text`); M5 only makes the run notice within a second instead of at its next timeout. App quit mid-run needs no shutdown code of ours: the next start's sweep fails the rows (Python relies on the same sweep after a capped shutdown).
- **Shard's proxy test, without its storage.** `crate::space::proxy::geo_check(&ProxyEntry, None)` is public, goes through the proxy and answers the exit IP, country code and ISP; it writes nothing. `space::proxy::full_test` is NOT used: it keeps a history in Shard's own proxy store. `profile.rs` already builds a `ProxyEntry` from our proxy (`entry`).
- **The v24 columns are enough.** `proxies` has `status`, `reason`, `latency_ms`, `exit_ip`, `country`, `org`, `fail_streak`, `checked_at`, `kind`; `jobs` has `choice`, `proxy_mode`, `proxy_kind`; `accounts` has `retry_count`; settings are key/value rows. **No schema change is needed or made.** The approved Proxy pool mock shows fields v24 does not have (name, notes, protocol, IP type, IP history): that is M7's question and will come to the owner as a decision then.
- **What the pages already have:** the Accounts job card with Stop, the halted-queue card with Resume (a toast only today), the header menu's "Pause all queues" (mock only), the Peek's Retry (mock only), and the Settings rows "browsers at once" and "stop a queue after N failures" (mock only). `RunState` (`types.ts`) already carries `jobs[]` and `halted[]`.
- **Public repo:** no production counts, hosts, user names or addresses in this plan, the code, tests or commit messages. Live results go to the owner report only.

## Decisions for the owner

Everything else is a straight port of Python's behaviour or follows a binding owner decision. These three are real forks; the plan is written with the recommendation.

| # | Decision | Recommendation | In plain words |
|---|---|---|---|
| D-M5-1 | What **Stop** on a running job does, now that several accounts run at once (Python has no per-job stop, only Pause all) | **The job's waiting accounts are taken out of the queue** (they become Failed "cancelled", shown as stopped, Retry works on them, they never count toward a failure streak). Accounts already in a browser finish. Other jobs go on | Stop means "start nothing more of this job"; what is half-way is allowed to finish, nothing is deleted |
| D-M5-2 | A free Outlook mailbox that turns out to hold Breeze Blue mail when it is first read. M3 skipped it silently and took the next one, so a job still made N | **Its account fails "already exists" within about a second, no browser opens, and the job makes one fewer.** The mailbox is never offered again | With a queue the N mailboxes are chosen up front. Swapping one mid-job needs extra machinery; a clear failed row and a second Make is simpler |
| D-M5-3 | The proxy check in the background | **Keep Python's: every 5 minutes, each turned-on proxy, one at a time, through Shard's own test.** A proxy that fails is "failing" and gets no new account until a check passes. Nothing is moved (owner): accounts on it fail with their proxy error | Without it a dead proxy is found only by accounts failing on it. The Problems card and Retest (M7) read the same result |

## Not in M5

- The roadmap's "key/verify checks" (they need each provider's check: M6) and "prune" (old events, screenshots, mail bodies: with M7/M8). Say so if the owner wants prune earlier: it is one startup function.
- Python's desktop notification when a queue stops (the halted card shows it), bulk and row Retry, Open browser, Close all browsers of ours (Shard's Close all is there), Export, Delete: M7.
- "Free Ele mailboxes first" (Python's planner): the Ele route makes new inboxes, as today. Make stays 1-20 accounts per job (Python allows 500); several jobs may queue.
- Google's sweep extras ("Quit after OTP", deleting the failed profile): with Google in M6. Until then every swept row reads `Interrupted`.

## Global Constraints

The M0-M2, M3 and M4 Global Constraints apply unchanged. M5 adds:

- **One place starts a run:** `Engine::schedule`. It checks and registers under one lock with no await in between. Nothing else calls `run_account` in the app.
- **At most `max_chrome` runs at once, one per mailbox.** A slot is freed on every end of a run: success, failure, error, panic.
- **Queue state the person sees is the row:** `queued`, `running`, `failed`. Paused, halted and streak counts are memory only (Python): a restart clears them and the sweep fails what was live.
- **Atomic picks:** a proxy pick and its `last_used_at` stamp, and a pick plus the new browser row, happen under one process-wide lock; a job and all its queued rows, including the choice of free mailboxes, are one transaction. The store has one writer process (ours).
- **The check never names a proxy:** no host, user, password or URL in an event, a reason or an error. Reasons are fixed texts.
- **Tests never reach the network:** the proxy check takes its probe as a function; the app's check loop is not started under `cfg(test)`.
- Code is edited on the Mac; build, every test and every live run happen on the Windows test box under `box-env.ps1`. The bar is no new failures against `main`'s baseline. Live runs need the owner's approval.

## Review Focus

1. **Two runs on one mailbox, or more than the cap.** Check-and-register must not be split by an await or a second lock. Pinned in Task 2 (`concurrency_respected`, `one_running_account_per_email`, `retry_respects_cap`).
2. **A slot that never comes back.** A run that panics, errors before its row is written, or leaves its row `running` must free its slot and fail the row. Pinned in Task 2 (`slot_released_when_the_run_panics`, `a_run_that_leaves_its_row_live_is_failed`).
3. **Two slots take the same proxy or mailbox.** Pinned in Task 1 (`concurrent_makes_never_share_a_mailbox`) and Task 3 (`concurrent_picks_never_share_a_proxy`, `concurrent_new_browsers_spread_in_least_mode`).
4. **The streak counts the wrong failures, or stops twice.** Owner stops, proxy and mailbox causes and refusals before a browser never count; runs already going when the queue stops still count but stop it once. Pinned in Task 2 (the `test_fail_streak.py` port).
5. **The sweep fails a healthy row, or runs late.** Only unfrozen live rows; before the first schedule; once per opened store. Pinned in Task 1 (`sweep_fails_only_unfrozen_live_rows`) and Task 4 (`the_store_is_swept_when_it_opens`).
6. **A secret in a proxy reason or event.** Pinned in Task 3 (`check_reasons_and_events_hold_no_host_or_credentials`).

---

## File map

```
src-tauri/src/creator/
  store/mod.rs     + Job, job_get, queue_job, accounts_queued, account_requeue, job_cancel_queued,
                     sweep_live, jobs_live, proxy check writes, setting_int, breeze_free(provider)   (T1)
  engine.rs        new: Engine (schedule, slots, pause, halt, streak, retry, stop job), sweep         (T2)
  engine/tests.rs  new: the Python engine tests, no browser                                          (T2)
  errors.rs        counts_toward_streak loses its dead_code allow                                    (T2)
  mod.rs           pub mod engine                                                                    (T2)
  proxy.rs         + PICK lock, new_browser, check_one, check_loop, live probe                       (T3)
  runner.rs        make (queue), browser row at run start, close watcher, engine(), start;
                   RunState, RUN, begin_job, make_breeze, new_account, used_elsewhere go             (T4)
  runner/tests.rs  the make/watcher tests; the one-at-a-time tests go                                (T4)
  runner/live.rs   live_breeze kept compiling on make + engine (T4); live_engine, live_sweep         (T7)
  pages.rs         run_state, job_stop, pause_all, resume_all, queue_resume, retry_accounts,
                   test_proxy, settings max_browsers / fail_streak_stop                              (T5)
src/creator/
  types.ts, mock.ts, i18n/{en,ru,zh}.json, pages/AccountsPage.tsx,
  widgets/AccountPeek.tsx, settings/CreatorSettingsSection.tsx                                       (T6)
docs/creator/
  windows-build.md   the new live tests                                                              (T7)
  working-process.md §7 risky areas: + creator/engine.rs                                             (T7)
```

---

### Task 1: Store: the queue's rows and the proxy check's writes

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
- Proxy check: `Proxy` gains `exit_ip: Option<String>`, `fail_streak: i64`, `reason: Option<String>` (the `PROXY` select list grows). `proxy_check_ok(id, latency_ms, country, org, exit_ip: Option<&str>, rotating: bool)`: `status` `slow` above 2000 ms else `healthy`, `fail_streak=0`, `reason=NULL`, `checked_at=now`; `exit_ip` only when given; `kind='rotating'` only when `rotating` (never back to static). `proxy_check_failed(id, reason)`: `status='failing'`, `reason`, `checked_at`, `fail_streak=fail_streak+1` in one statement. `proxies_active() -> Result<Vec<Proxy>>`.

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
  - `proxy_check_writes` (the store half of `test_proxy_checker.py`): healthy vs slow at 2000 ms; a failure sets failing and adds 1 to the streak, an ok resets it; an empty IP keeps the stored one; rotating is sticky; `record_failure_is_atomic_increment` (two threads, 50 failures each -> 100).
- [ ] **Step 2:** Box: `cargo test --manifest-path src-tauri/Cargo.toml creator::store` -> the new tests fail.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Box: `cargo test … creator::` PASS; `rustfmt --edition 2021 --check` clean; `new_db_is_v24_and_matches_python_tables` still passes (no schema change).
- [ ] **Step 5:** Commit `Creator: store functions for the account queue and the proxy check`.

### Task 2: The engine (parallel with Task 3)

**Files:**
- Create: `src-tauri/src/creator/engine.rs`, `src-tauri/src/creator/engine/tests.rs`
- Modify: `src-tauri/src/creator/mod.rs` (`pub mod engine;`), `errors.rs` (remove the `dead_code` allow on `counts_toward_streak`)

**Interfaces:**
- `pub type RunFn = Arc<dyn Fn(i64) -> BoxFuture<'static, ()> + Send + Sync>`: runs account `id` and writes its row. `pub type ReadyFn = fn() -> bool`: false while Shard's engine is downloading, updating or swapping.
- `pub struct Engine { db: Arc<Db>, run: RunFn, ready: ReadyFn, st: Mutex<State> }`; `State { paused: bool, halted: HashMap<String, Halt>, streak: HashMap<String, u32>, running: HashMap<i64 /*account*/, i64 /*mailbox*/>, waiting_ready: bool }`; `#[derive(Serialize)] pub struct Halt { pub n: u32, pub error: String, pub code: String }`.
- `Engine::new(db, run, ready) -> Arc<Engine>`.
- `pub fn sweep(db: &Db) -> Result<i64>`: `db.sweep_live()`, and an `events` row `warn` / step `cleanup` / `"{n} interrupted by the last close"` when n > 0.
- `schedule(self: &Arc<Self>)` (Python `_schedule`), under the state lock, no await: paused -> return. Not `ready()` -> if queued rows exist and `waiting_ready` is false, set it and spawn one task that sleeps 5 s, clears it and calls `schedule`; return. `cap = db.setting_int("max_chrome", 3, 1, 20)`. For each `accounts_queued()` row: stop at `running.len() >= cap`; skip a halted platform, an id already running, a mailbox already in `running`; else insert into `running` and `tokio::spawn` the run.
- The spawned run: `tokio::spawn(run(id))` awaited through its `JoinHandle`, so a panic is an `Err`, then `done(id)`.
- `done(id)` (Python `_done` + `_tally`): remove from `running`. Read the row: still `queued`/`running`/`waiting` (the run panicked or returned without writing) -> fail it `"run: task ended"` (code `APP_ERROR`) through `account_update` with `finished_at`. Then tally by platform: `linked` -> streak 0. `failed` whose `error_code` is NULL or `counts_toward_streak(code)` -> streak + 1; with `limit = setting_int("fail_streak_stop", 3, 0, 50)`: when `limit > 0`, `n >= limit` and the platform is not halted yet -> `halted[platform] = Halt { n: limit, error, code (or "OTHER") }` and an `events` row `warn` / step = platform / `"paused after {limit} failures in a row"`. Anything else does not count. Then `schedule`.
- `pause_all()` / `resume_all()` (Python `_set_paused`): set the flag, clear `halted` and `streak`, `schedule`. `resume_queue(platform)`: drop that platform's halt and streak, `schedule`. `kick()`: `schedule`.
- `retry(ids: &[i64]) -> (usize, Vec<String>)` (Python `_retry_service` without parents): per distinct id: missing, frozen, in `running`, or not `failed` -> skipped silently (the caller counts). `error == "mailbox gone"` -> refused `"{label} not retried: mailbox gone — its inbox was deleted, make a new account"`; a code in `NOT_RETRYABLE` -> refused `"{label} not retried: {error}"`; each refusal is also an `events` row `warn` / step `retry` on the mailbox. Else `account_requeue`. Then `kick`. `label` = `providers::label(platform)`.
- `stop_job(job) -> Result<i64>`: `db.job_cancel_queued(job)`, then `kick` (D-M5-1).
- `state() -> (bool /*paused*/, Vec<(String, Halt)> /*sorted by platform*/)`; `has_task(id) -> bool`.
- ponytail note in the file: scheduling reads every queued row per pass (Python does); page it if queues reach tens of thousands.

- [ ] **Step 1: Failing tests** (`engine/tests.rs`; `temp_db`, rows inserted directly, a fake `RunFn` that marks the row `running`, waits on a per-id gate, then writes the scripted `(status, error)` with its `error_code`; no browser, no network). Ports, Python names kept:
  - From `test_engine.py` / `test_max_chrome.py` / `test_scheduler.py` / `test_busy.py`: `concurrency_respected` (cap 2, 5 queued -> 2 running, 3 queued; one ends -> the next starts); `raising_cap_kicks_scheduler` (cap 1 -> 3 and `kick`); `pause_stops_new_starts`; `pause_lets_running_finish_without_new_start`; `one_running_account_per_email` (two platforms' rows on one mailbox run one after the other); `frozen_account_never_starts_until_unfrozen`; `schedule_tolerates_vanished_row`; `queued_rows_start_oldest_first`; `retry_only_failed`; `retry_skips_an_account_still_running`; `retry_respects_cap`; `retry_refuses_what_no_rerun_fixes` (ACCOUNT_EXISTS and mailbox gone: row untouched, the event text above).
  - Ours: `slot_released_when_the_run_panics`; `a_run_that_leaves_its_row_live_is_failed` (`run: task ended`, slot free, the next starts); `nothing_starts_while_the_engine_is_not_ready_then_it_goes_on` (ready flips to true; the 5 s wait is a field the test shortens); `stop_job_cancels_only_its_queued_rows` (running ones finish, the other job runs on, no streak change); `sweep_runs_first` (`sweep` then `schedule`: the rows a crash left are failed, not started).
  - From `test_fail_streak.py`: `queue_halts_after_n_failures` (limit 2, 4 queued -> 2 failed, 2 still queued, `Halt { n: 2, error: "boom", code: "OTHER" }`, one event); `success_resets_streak`; `interrupted_not_counted`; `mailbox_gone_not_counted`; `unusable_browser_proxy_not_counted`; `refusals_before_any_browser_never_halt` (`No browser`, `Email has no address`, `No healthy proxy for a new browser`); `zero_never_halts`; `halt_is_per_queue`; `inflight_results_count_but_halt_once` (cap 3, limit 2, three fail together -> streak 3, halted once, one event); `resume_queue_clears_and_restarts`; `resume_all_clears_halts`; `default_pauses_after_three`; `window_closed_still_counts`; `halt_records_code`.
- [ ] **Step 2:** Box: `cargo test … creator::engine` -> FAIL (does not compile).
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Box: `cargo test … creator::` PASS, run `creator::engine` 5 times in a row (ordering bugs show as flakes); rustfmt clean.
- [ ] **Step 5:** Commit `Creator: the account queue's scheduler (slots, pause, fail streak, retry, sweep)`.

### Task 3: Proxy check and atomic picks (parallel with Task 2)

**Files:**
- Modify: `src-tauri/src/creator/proxy.rs`
- Test: `proxy.rs` tests module

**Interfaces:**
- `static PICK: Mutex<()>`: held across list -> choose -> `proxy_touch` in `pick_proxy` (its ponytail note goes; a new one says: process-wide lock, a transaction if a second process ever writes the store).
- `pub fn new_browser(db: &Db, mailbox_id: i64, mode: &str, kind: &str) -> Result<i64>` (Python `create_browser`): under `PICK`, pick, then `browser_create` with the picked proxy; so `least` counts the row before the next pick. `NoHealthyProxy` -> the error, no row.
- `pub struct Exit { pub ip: String, pub country: String, pub org: String }`; `pub type Probe = Arc<dyn Fn(Proxy) -> BoxFuture<'static, Result<Exit, String>> + Send + Sync>`.
- `pub fn live_probe() -> Probe`: `profile::entry(&p)` (made `pub(super)`), `tokio::time::timeout(10 s, crate::space::proxy::geo_check(&entry, None))`; `Exit { ip, country: country_code, org: isp }`. Errors become fixed texts only: the timeout -> `"connect timeout after 10 s"`; an error text containing `407` -> `"407 · wrong user or pass"`; anything else -> `"proxy check failed"`.
- `pub async fn check_one(db: &Db, id: i64, probe: &Probe) -> Result<Proxy>` (Python `check_one`): time the probe; ok -> `seen` = the stored `exit_ip`, or, when none is stored and an IP came back, a second probe's IP (a failed second probe is no answer); `rotated` = both IPs known, different, and of the same family (`:` in both or in neither); `proxy_check_ok`. Err -> `proxy_check_failed(reason)`. A deleted row -> `Err`.
- `pub async fn check_loop(db: Arc<Db>, probe: Probe, stop: Arc<AtomicBool>, interval: Duration)` (Python `check_loop`, interval 300 s in the app): each pass checks `proxies_active()` one at a time, reading `stop` before each; one proxy's error is an `events` row `warn` / step `proxy` / `"check skipped"` and the pass goes on; then it waits `interval`, waking at once on `stop`.
- No `on_checked` hook and no mover (owner): a failing proxy only stops being picked (`usable`), and an account already on it fails `PROXY_UNUSABLE`.

- [ ] **Step 1: Failing tests** (fake probes; no network):
  - `concurrent_picks_never_share_a_proxy`: 8 proxies, 8 threads, `rotate` -> 8 distinct.
  - `concurrent_new_browsers_spread_in_least_mode`: 4 proxies, 4 threads `new_browser(.., "least", "")` -> 4 distinct proxies, 4 browser rows.
  - `new_browser_without_a_usable_proxy_makes_no_row`; `new_browser_in_mode_none_has_no_proxy`.
  - The `test_proxy_checker.py` port: `check_healthy_and_slow`; `check_failure_increments_streak`; `check_loop_checks_active_only`; `check_loop_survives_one_proxy_failing`; `check_loop_deleted_row_does_not_kill_loop`; `check_loop_stops_between_proxies`; `kind_same_ip_stays_static`; `kind_changed_ip_between_checks_is_rotating_and_sticky`; `kind_first_check_probe_detects_rotation`; `kind_failed_or_empty_lookup_never_flips`; `kind_ignores_address_family_change`; `empty_ip_keeps_the_stored_exit_ip`.
  - `a_failing_proxy_gets_no_new_account_until_a_check_passes`: check fails -> `pick_proxy` skips it; check passes -> picked again.
  - `check_reasons_and_events_hold_no_host_or_credentials`: a probe error carrying the host, user and password -> the stored `reason` is one of the three fixed texts; no event holds any of them.
  - The existing `proxy.rs` tests unchanged.
- [ ] **Step 2:** Box: `cargo test … creator::proxy` -> FAIL.
- [ ] **Step 3:** Implement.
- [ ] **Step 4:** Box: `cargo test … creator::` PASS; rustfmt clean; `check-seams` shows no new file (nothing in `space/` is touched or widened).
- [ ] **Step 5:** Commit `Creator: background proxy check on Shard's test, picks that cannot collide`.

### Task 4: Runner on the queue: Make, the browser row, the close watcher

**Files:**
- Modify: `src-tauri/src/creator/runner.rs`, `runner/tests.rs`, `runner/live.rs` (only what keeps `live_breeze` compiling and working)
- Test: `runner/tests.rs`

**Interfaces:**
- `Browsers` gains `fn running(&self, profile_id: &str) -> bool` (`Shard`: `Tracker::shared().is_running`; `TestBrowsers`: not `gone` and not `stopped`).
- **Close watcher** in `attempt`, around `bb::run`: a task that every `WATCH_POLL` (1 s, Python `watch_poll`; a `Ctx` field so tests shorten it) reads `cx.browsers.running(&profile_id)` and, once false, sets the run's `TtsRun.closed` flag (it exists for this: "the runner sets it") and ends. It is aborted when the flow returns. The flow's waits already end on `closed`; the result is `failure_text` -> `Interrupted`, no screenshot, as today. The ponytail note on `run_account` goes.
- **The browser row at run start** (Python `_run_svc`): in `attempt`, after the mailbox pre-flight (gone, Outlook check, platform mail) and before `browser_proxy`: no browser row for the mailbox -> `proxy::new_browser(db, mb.id, mode, kind)` with the account's job (`job_get(acc.job_id)`; no job: `rotate`, any kind); `NoHealthyProxy` -> the run fails `NO_PROXY` and no browser row is made. So a dead mailbox never uses up a proxy pick, and a Retry after `NO_PROXY` picks again (the old "proxy deleted" note goes).
- **Make queues** (Python `planner.make` + `api.make_accounts`): `pub async fn make(db: &Arc<Db>, ele: Option<&EleClient>, count: u32, proxy_mode: &str, route: &str) -> Result<(i64, usize)>` under `static MAKE: tokio::sync::Mutex<()>` (Python `_make_lock`): `check_start` as today; `email:outlook` -> `queue_job(.., Mailboxes::FreeOutlook(count))`; `email:ele` -> `create_mailbox` `count` times, one after another, then `queue_job(.., These(&ids))`; an Ele error after some were made -> `Err("{n} new emails created, nothing queued: {why}")` (Python's text; `1 new email`). `jobs.choice` holds the route.
- `pub fn app_ctx(db: Arc<Db>) -> Result<Ctx<Shard>>`: today's `start` body that builds the `Ctx`; `ele` is `Some` when the Ele address and key are set.
- `pub fn engine() -> Result<Arc<Engine>>`: one engine per opened store (kept beside it; `reset_db_for_test` drops both). Made on first use after `db()` is ready: `engine::sweep(&db)` first, then `Engine::new(db, run, pages' engine-ready fn)` where `run` builds `app_ctx` and calls `run_account` (a `Ctx` error fails the row with `err_line`), then, outside `cfg(test)` only, `tauri::async_runtime::spawn(proxy::check_loop(db, live_probe(), stop, 300 s))` whose `stop` is set when the engine is dropped.
- `pub async fn start(count, proxy_mode, route) -> Result<(i64, usize)>`: `engine()?`, `make(..)`, `engine.kick()`.
- **Removed:** `RunState`, `RUN`, `EndRun`, `begin_job`, `make_breeze`, `new_account`, `make_rows_and_run`, `used_elsewhere` (D-M5-2: the run's own pre-flight already fails such a mailbox `ACCOUNT_EXISTS` before a browser opens), and `Db::account_queue` if nothing else calls it.
- `runner/live.rs`: `live_breeze` calls `make` then an `Engine` built on its own `Ctx` and waits until no row of the job is live; `CREATOR_LIVE_MAILBOX_ID` queues exactly that mailbox with `Mailboxes::These`. Same variables, same printed report.

- [ ] **Step 1: Failing tests** (browser-backed ones as today, under `CREATOR_REQUIRE_BROWSER=1`):
  - `a_browser_closed_mid_run_ends_the_account_within_seconds`: the site holds the verify mail back (a 180 s wait); `gone` is set 1 s in -> the run returns in under 5 s (poll shortened to 100 ms) with `Interrupted`, code `INTERRUPTED`, no screenshot, and the watcher task is gone.
  - `the_watcher_stops_when_the_run_ends_normally`.
  - `make_queues_outlook_rows_oldest_first_and_starts_nothing`: rows `queued`, no browser row, no open; replaces `outlook_make_takes_free_mailboxes_oldest_first_and_skips_taken` (its skip cases stay, checked on what `make` queued).
  - `make_on_ele_creates_the_inboxes_then_queues_them`; `ele_down_part_way_queues_nothing_and_says_how_many_were_made` (replaces `ele_down_fails_without_rows…`'s Ele half).
  - `a_run_makes_its_browser_row_and_no_proxy_fails_it_without_one` (replaces the no-proxy half): `NO_PROXY`, no browser row; after a proxy is added, the same row run again links.
  - `a_gone_mailbox_uses_no_proxy_pick`: `last_used_at` of every proxy unchanged.
  - `a_free_mailbox_with_breeze_mail_fails_account_exists_without_a_browser` (replaces `a_mailbox_whose_fresh_inbox_has_breeze_mail_is_skipped_without_rows`, D-M5-2).
  - `two_makes_at_once_never_queue_one_mailbox_twice` (through `make`, 2 tasks).
  - `the_store_is_swept_when_it_opens` (under `test_root`, `migrate::serial()`): a store file with a `running` and a `queued` row -> after `engine()`, both failed `Interrupted`; a second `engine()` call sweeps nothing new (a row set `queued` in between stays queued and is run).
  - `the_job_is_seen_from_the_start_and_its_accounts_join_it` rewritten on `make` + `jobs_live`.
  - Removed with their code: `one_run_at_a_time`, `stop_is_checked_between_accounts`.
  - Unchanged and still passing: the happy paths, the failure and secret tests, the pre-flight tests.
- [ ] **Step 2:** Box: `cargo test … creator::runner` -> FAIL.
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
- `pause_all`, `resume_all` -> null. `queue_resume {platform}` -> null; a platform that is not ours -> `unknown queue: {platform}`.
- `retry_accounts {ids}` -> `{ retried, skipped, error? }`: `error` is the first refusal text (`Nothing to retry` for no ids).
- `run_state` -> `{ jobs: db.jobs_live(), halted: [{ platform, n, error, code }], paused, setup }`. The halted `error` is the stored (already masked) text.
- `test_proxy {id}` -> `true` when `proxy::check_one` leaves the proxy not `failing`.
- `settings` adds `max_browsers` (`max_chrome`) and `fail_streak_stop`; `settings_set` takes both in `patch`: out of range -> `max_browsers must be in 1..20` / `fail_streak_stop must be in 0..50`, nothing saved; a saved `max_browsers` kicks the engine.
- Every command that needs the engine gets it through `runner::engine()`; `call` asks for it once at its top and ignores an error there (while the first open imports), so the sweep and the check loop start with the app's first command (the sidebar's `nav_counts`), not with the first Make.

- [ ] **Step 1: Failing tests:** `run_state_matches_types` (keys of `RunState`, its job and halted rows against `types.ts`, the existing `fields()` helper); `run_state_lists_every_job_still_making`; `job_stop_cancels_queued_rows_of_that_job_only`; `pause_and_resume_show_in_run_state`; `queue_resume_refuses_an_unknown_platform`; `retry_accounts_requeues_and_reports_refusals`; `settings_engine_values_default_validate_and_save` (3 and 3 by default; 0 and 21 refused for `max_browsers`; 51 refused for `fail_streak_stop`); `no_command_answer_holds_a_token_or_client_id` extended with the new answers; `the_first_command_sweeps_the_store`.
- [ ] **Step 2:** Box: FAIL. **Step 3:** Implement. **Step 4:** Box: `cargo test … creator::` PASS; rustfmt clean.
- [ ] **Step 5:** Commit `Creator: queue commands (stop a job, pause, resume, retry, engine settings)`.

### Task 6: Pages (parallel with Task 5 and Task 7)

**Files:**
- Modify: `src/creator/types.ts`, `mock.ts`, `i18n/{en,ru,zh}.json`, `pages/AccountsPage.tsx`, `widgets/AccountPeek.tsx`, `settings/CreatorSettingsSection.tsx`
- Test: `npm run check` (incl. `i18n.test.ts`), `e2e/creator-mockups.spec.ts` in `dev:mock`

**Interfaces** (Task 5's commands; only what the approved mockups already show):
- `RunState` gains `paused: boolean`; the mock answers it and the new commands (`job_stop`, `pause_all`, `resume_all`, `queue_resume`, `retry_accounts`).
- Accounts job card: Stop -> `job_stop { job_id }`, the existing toast. One card per job (already a list).
- Halted card: Resume -> `queue_resume { platform }`, then the existing toast and a `run_state` reload.
- Header menu: shown in the real build with one item, "Pause all queues" -> `pause_all`, or, while `paused`, "Resume all queues" -> `resume_all` (new key `creator.accounts.resumeAll` in the three languages; Shard's wording for resume). "Close all browsers" stays mock only.
- While `paused` and a job is live, the job card's `why` line reads the new key `creator.job.paused` ("Paused: accounts in a browser finish, no new one starts").
- Peek: Retry is real for a failed, retryable account -> `retry_accounts { ids: [id] }`; `retried == 0` -> `toast.error` with the answer's `error`. Log in again, Check, Open browser and the More menu stay mock only.
- Settings: the rows "browsers at once" and "stop a queue after N failures" show in the real build and save through `settings_set`; "wait for a code" and "delete the browser of a failed sign-up" stay mock only (Breeze reads neither).
- The run poll (2 s while a job is live) is unchanged; it now also runs while `halted` is not empty or `paused` is true, so Resume shows without a reload.

- [ ] **Step 1:** Box: `npm run check` and the mockup spec pass on `main` first (baseline).
- [ ] **Step 2:** Wire as above. `npm run check` PASS; `E2E_PORT=4794 npx playwright test e2e/creator-mockups.spec.ts --project=dev` passes (screens unchanged except the real header menu).
- [ ] **Step 3: Dev run on the box (sandboxed, fake data only),** through `dev_bridge` + `npm run dev:mock` with `VITE_CREATOR_BRIDGE`: Make 3 on made-up Outlook rows -> three rows fail fast (`mailbox gone` or `Outlook unavailable`), the halted card shows at the default 3, Resume clears it; Pause all then Make -> rows stay Making; Resume all -> they run; Retry in Peek queues one again; Stop takes the queued ones out. Screenshots light/dark for the report, emails blurred.
- [ ] **Step 4:** Commit `Creator: Accounts and Settings on the queue (stop, pause, resume, retry)`.

### Task 7: Live harness (parallel with Task 5 and Task 6)

**Files:**
- Modify: `src-tauri/src/creator/runner/live.rs`, `docs/creator/windows-build.md` (one paragraph per new test), `docs/creator/working-process.md` (§7 risky areas: `src-tauri/src/creator/engine.rs`)

**Interfaces:** all `#[ignore]`, the same sandbox setup as `live_breeze`, printing statuses, codes, seconds and counts only (never an address, token, proxy or path of the owner's data).
- `live_engine`: the `live_breeze` variables plus `CREATOR_LIVE_CONCURRENCY=K` (saved as `max_chrome`), `CREATOR_LIVE_OUTLOOK_N` and `CREATOR_LIVE_ELE_N` (one job each, queued together), and one optional scenario:
  - `CREATOR_LIVE_PAUSE_AFTER=n`: after `n` accounts ended, `pause_all`; waits until nothing runs, then 60 s more and asserts no account started meanwhile; `resume_all`.
  - `CREATOR_LIVE_CLOSE_AFTER_S=s`: `s` seconds after the first browser opened, stops one running account's profile through `shard::stop` (what Shard's Close does) and prints how many seconds its row took to read `Interrupted`.
  - `CREATOR_LIVE_EXIT_AFTER_S=s`: `s` seconds after the first browser opened, prints the live row count and ends the process with `std::process::exit(0)`: the app closing mid-run, by its own exit, never a kill.
  - Every second it samples `Tracker::shared().running().len()` and prints the maximum at the end; it asserts the maximum is at most K, that no two rows of the run share a mailbox, and, in `rotate` with at least as many usable proxies as accounts, that no two new browsers of the run share a proxy.
  - The report per account: status, code, seconds, queue wait; then linked / failed, and the halted state if the queue stopped.
- `live_sweep`: `CREATOR_LIVE_OUT`. Opens the store, prints the live row count, runs `engine::sweep`, prints how many it failed and asserts none is live. With `CREATOR_LIVE_RETRY=1` (and the `live_breeze` engine variables) it then retries those rows through an `Engine` and prints their results.
- `live_proxy_check`: `CREATOR_LIVE_OUT`, `CREATOR_LIVE_SAMPLE=N`: one `check_one` with `live_probe()` on N random active proxies, one at a time; prints `healthy / slow / failing` counts, the three reason counts and the median milliseconds.

- [ ] **Step 1:** Write the tests and the doc paragraphs.
- [ ] **Step 2:** Box: `cargo test --lib creator::runner::live -- --list` shows them; `cargo test … creator::` PASS; rustfmt clean.
- [ ] **Step 3:** Commit `Creator: live tests for the queue (slots, pause, close, sweep, proxy check)`.

### Task 8: Box gate and live gate

- [ ] **Step 1: Full box gate** on the branch (`box-workflow.md` flow), against `main`'s baseline: `npm run check`; `cargo test --locked` (in `src-tauri`, full); rustfmt check; `node --test scripts/creator/*.test.mjs`; `node scripts/creator/check-seams.mjs` (no new file, no new seam); the e2e lane from §7.
- [ ] **Step 2: Whole-branch review** by a fresh reviewer (opus or fable) before any live run.
- [ ] **Step 3: Live gate, ONLY after the owner's explicit approval, owner present.** In the sandbox, under `box-env.ps1`, in the desktop session (scheduled task), on the sandbox store (a copy of the rehearsal store; never the production database, never stock Shard's folders). Real Ele, real never-used active Outlook mailboxes of that store, real proxies. Rules: back up first, one sample first, stop only by letting a run finish or by the harness's own scenario, never a kill.
  1. **Back up** the sandbox `creator.sqlite` (with `-wal`) and note its account count.
  2. **Proxy check:** `live_proxy_check` with a sample of 20. Expected: mostly healthy or slow; every failing one explained by its reason.
  3. **One sample:** `live_engine` with 1 Outlook account at K=1. Expected `linked`. Stop here on anything unexplained.
  4. **The mixed run:** 20 accounts at K=3, 10 Outlook and 10 Ele, with `CREATOR_LIVE_PAUSE_AFTER=5`. Expected: never more than 3 browsers, no shared mailbox, no shared proxy, nothing starts during the pause, the queue goes on after Resume.
  5. **Forced close:** 6 accounts at K=3 with `CREATOR_LIVE_CLOSE_AFTER_S=40`. Expected: that account `Interrupted` within 5 s, its slot taken by the next, the streak not raised by it. Then the same once more with the owner closing one browser window by hand.
  6. **Fail streak:** set `fail_streak_stop` to 2 and run 4 accounts in mode `rotate` on a store copy whose proxies are all turned off except one known-dead one (or any setup the owner prefers that makes sign-ups fail for a counted reason). Expected: the queue stops after 2, 2 stay queued, Resume starts them. Skip this step if no safe way to force counted failures exists on the day; the fake-run tests cover the rule.
  7. **Close mid-run and sweep:** 6 accounts at K=3 with `CREATOR_LIVE_EXIT_AFTER_S=40`. Then `live_sweep`: the rows it fails equal the live count printed at exit, none is live. Then `live_sweep` with `CREATOR_LIVE_RETRY=1`: every row ends linked or with an explained code; a half-made account that Breeze Blue already knows is reported as it comes (the sign-in path or `ACCOUNT_EXISTS`).
  8. **The real app, only if the owner gives a window with stock ShardBrowser closed** (same app id, single instance; it stops the Python app's runs meanwhile): our dev build under `box-env.ps1`: Make 6 at 3, Pause all and Resume all from the header menu, Stop on the job card, Shard's Close all, then quit the app through Shard's own prompt mid-run and start it again: the rows read Interrupted. Without that window this step is reported as not verified and is done at M8's rehearsal.
  9. **Report table:** per account its result, code, seconds; per scenario expected vs seen; the maximum browsers at once; the linked rate. Pass rule: every scenario's expectation holds and every failed account is explained by its code. Results go to the owner report, never the repo.
- [ ] **Step 4:** Report to the owner (result, test count, screenshots, what is unverified). Merge only on "merge".

---

## Execution notes

- **Order:** T1 -> (T2 ∥ T3) -> T4 -> (T5 ∥ T6 ∥ T7) -> T8. No two parallel tasks share a file: T2 owns `engine.rs`, `engine/tests.rs`, `errors.rs`, `mod.rs`; T3 owns `proxy.rs` (and the one-word visibility change in `profile.rs`); T5 owns `pages.rs`; T6 owns `src/creator/**`; T7 owns `runner/live.rs` and the two docs. T6 may start as soon as this plan's command shapes are agreed; its Step 3 needs T5.
- **Models:** T1 sonnet, reviewer opus (store is a risky area). T2 opus, reviewer fable or opus (the scheduler is the core risk). T3 sonnet, reviewer opus. T4 opus, reviewer opus or fable (`runner.rs` and the live flow). T5 sonnet. T6 sonnet. T7 sonnet. Two fix rounds not enough -> a higher model.
- Each task: implementer -> fresh reviewer -> fix rounds, in its own worktree. Push a task branch to `origin` only on the owner's word (the box fetches from GitHub); until then it can be reviewed on the Mac but its gates wait.
- Risky areas touched: `store/` (no schema change), `shard.rs` is NOT changed, the provider flow is NOT changed; `runner.rs` changes how a run starts and ends, hence the live gate.

## Exit criteria

- The Python engine, fail-streak, scheduler and proxy-checker tests named in Tasks 1-3 are ported and green on the box; the full box gate shows no new failure against `main`.
- No seam, upstream file or dependency added; the store is still v24 (`new_db_is_v24_and_matches_python_tables`).
- Live gate steps 1-7 pass with the owner present; step 8 passes or is listed as not verified.
- `runner.rs` holds no single-run gate and none of the three M5 ponytail notes (watcher, sweep, pick and touch).
- The owner has answered D-M5-1..3, and the plan matches the answers.

## Risks

- **A half-made account after a close:** the sweep cannot know whether Breeze Blue already made the account. Retry reuses the saved password and the flow's own sign-in path; if the site says the email exists and sign-in fails, the row ends `ACCOUNT_EXISTS` (not retryable). Live gate step 7 shows which.
- **Queued accounts do not survive a restart** (Python parity): after a crash a big queue is a list of Interrupted rows, and Retry is one at a time until M7's bulk bar. If that hurts before M7, the smallest fix is a "Retry all" on the sweep's result, an owner decision then.
- **Mail calls in parallel:** K runs poll Outlook at once from one IP (D-M3-5). Throttling shows as `MAIL_API_DOWN`, which counts toward the streak and may stop the queue: correct, but watch for it in the mixed run.
- **Shard's geo services refuse or throttle a proxy that works:** the check would call it failing and new accounts would skip it until the next pass. `live_proxy_check` measures it; if it shows, the probe falls back to a plain request through the proxy (Python's ipinfo call, about 20 lines).
- **A full pass over a large pool takes long** (one proxy at a time, up to 10 s each, as Python). Accepted; a pool of 5 at a time is a one-line change if the owner wants fresher results.
- **`max_chrome` on imported data:** the cutover brings Python's saved value; the default 3 applies only to a fresh store.
- **Stop then Make on Ele:** a stopped Ele account's inbox is not reused by the next Make (no "free Ele first" yet); Retry on the row uses it.
