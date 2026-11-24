# Creator M4: data import Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Bring the Python app's `ghidorah.sqlite` (mailboxes, accounts, proxies, browser rows, settings, mail) into our store as an exact copy, rehearse it on the Windows test box sandbox with the production backup, fix whatever the real data makes slow, and write the cutover runbook that M8 executes.

**Architecture:** No conversion code (D7): our store already is Python's schema v24. Import copies the Python file (and its `-wal` if present) byte for byte into a temp file next to our store, so SQLite never opens the source; then `VACUUM INTO` from that temp copy, a version and `quick_check`, and a rename to `creator.sqlite`. It runs once, on a background thread started by the first open of our store, when `creator.sqlite` is missing and Python's file is at its default place. Browser profiles need no copying: Python's `browsers.path` values are profile ids in stock ShardBrowser's own data folder, which our app takes over at cutover (D1, D17).

**Tech Stack:** Rust (rusqlite 0.32 bundled, already in use). No new dependency, seam or upstream file.

**Spec:** `docs/plans/2026-11-22-creator-m0-m2.md` (D1, D7, D17, roadmap M4/M8), `docs/plans/2026-11-23-creator-m3-outlook.md` (Risks: the platform-mail scan, token divergence), `docs/creator/working-process.md` §7, `docs/creator/box-workflow.md`, `docs/creator/windows-build.md`. Python source of truth (read-only): `ghidorah_creator/paths.py` (`app_dir`, `db_path`), `store/schema.py` (`MIGRATIONS`: 24 entries), `browser/shard_session.py` (`FOLDER = "Ghidorah"`, `ensure_proxy`, `open_shard_profile`).

**Facts:** production DB on the box `%LOCALAPPDATA%\ghidorah-creator\ghidorah.sqlite`, about 1 GB: tens of thousands of Outlook mailboxes (more gone than active), Ele mailboxes, a few hundred accounts across platforms, about a thousand proxies, secrets in plain columns. The Python app stays in production until cutover and is retired after it (owner, no comparison runs).

---

## What the code already tells us (verified while planning)

- **Profiles map 1:1.** Python made each account's profile through stock Shard's Automation API (`client.create(name, folder="Ghidorah", fingerprint)`) and stored the returned id in `browsers.path`. Our `profile::ensure_profile` reads the same column the same way and checks `crate::store::profiles_dir()/<id>.json`. Same data folder after cutover → the same ids resolve. An id Shard no longer has (deleted or trashed in Shard; the v24 migration removed every older browser row) is already handled: cleared, `ShardBrowser profile is gone`, a new profile on the next run.
- **Proxies convert themselves.** Python bound a profile to a Shard proxy-store entry (`ensure_proxy` → `meta.proxy_id`). Our `shard::open_session` calls `profile::set_proxy` on every open: it writes the browser row's pool proxy inline and clears `proxy_id`. So an imported profile becomes D17-shaped on its first run in our app. Until then, opening it by hand in Shard uses its Shard proxy entry, which is still in the same `proxies.json`. Nothing to migrate.
- **Folder.** Python's profiles sit in Shard folder "Ghidorah"; ours are made in "Creator". No code reads the folder.
- **Not carried:** Python failure screenshots (`ghidorah-creator\shots`; our Peek reads only `creator\screens`, so old rows show none), the YesCaptcha baked copies (Shard's extension library already holds what the profiles reference), Shard settings `shard_api_token`/`shard_api_port` (stay in the table, unused).
- **No startup sweep yet** (M5, `runner.rs` ponytail note): rows Python left `queued`/`running` show as "making" (`waiting` shows as its raw status). The import reports all three (`live_accounts`); the cutover needs 0.
- **`import_basics` becomes dead code after M8** (the full import replaces it); remove it then, not in M4.

## Sequencing (recommended)

M4 builds the import and rehearses it **in the box sandbox on a backup copy now**; the **real cutover runs at M8**, after M5–M7 and every provider in M6 is ported and live-gated. Cutting over earlier is not possible without breaking something: our app replaces stock ShardBrowser (same app id, port, data folder), so the Python app would lose its launcher, and two stores would diverge. Before the M8 cutover, Task 3's rehearsal is repeated on a fresh backup with the release build.

## Decisions (approved by the owner, 2026-11-24, all as recommended)

| # | Decision | Recommendation | Why |
|---|---|---|---|
| D-M4-1 | When the real cutover happens | **At M8**, once every provider (Qwen, Noiz, Fish Audio, Grok, ChatGPT, ElevenLabs, Google and the "via Google" routes) runs in our app. M4 = import code + sandbox rehearsal + perf fixes + the runbook | The Python app needs stock Shard and its own DB for every unported provider. One switch, no period with two stores |
| D-M4-2 | How the import starts | **Automatically on first open of our store** when `creator.sqlite` is missing and `%LOCALAPPDATA%\ghidorah-creator\ghidorah.sqlite` exists. A failed import leaves no `creator.sqlite`, shows its error on our pages and retries on the next start | Zero UI and zero steps for the owner at cutover. A Settings button would have to swap an open store under running clients |
| D-M4-3 | The Python app's profiles and their proxies | **Leave them as they are**: in Shard folder "Ghidorah", on their Shard proxy entries; the first run in our app writes the pool proxy inline (code already does it). Python's Shard proxy entries stay in Shard's Proxies list (delete by hand if wanted) | No migration code. Mass-rewriting hundreds of profiles buys nothing the first run doesn't do |
| D-M4-4 | Checking an imported profile opens, before cutover | **Yes, in the sandbox**: copy 2 non-running account profiles (`profiles/<id>.json` + `user-data/<id>/`) read-only out of stock Shard's data folder, with the owner present. Without it, the profile check waits for the cutover itself | The only part of M4 that touches production Shard data; it is a read-only copy |
| D-M4-5 | Accounts and token rotations made in the sandbox (M3 gate, this rehearsal) | **Accept that cutover drops them**: the cutover imports Python's file, not the sandbox store. Copy the M3 Breeze keys by hand first if they are wanted | Test data. Python's older refresh tokens stay valid (M3 Risks) |

## Global Constraints

The M0–M2 and M3 Global Constraints apply unchanged. M4 adds:

- SQLite **never opens the Python file**: `std::fs::copy` of `ghidorah.sqlite` (and `-wal` when it exists; never `-shm`) to `<our dir>/creator.sqlite.src` (+ `.src-wal`), then `VACUUM INTO` `<our dir>/creator.sqlite.import` from that copy; the result becomes `creator.sqlite` only after every check passes; both temp files are removed in every case. Our dir: `crate::store::local_root()` (`%LOCALAPPDATA%\shardx-launcher`). Python's file: `local_root().parent()/ghidorah-creator/ghidorah.sqlite` (same `%LOCALAPPDATA%` parent; in tests the test root's).
- Checks: source `user_version == 24` (else `"ghidorah.sqlite is version {n}, expected 24"`), copy `user_version == 24`, `PRAGMA quick_check` == `ok` (else `"ghidorah.sqlite failed the integrity check"`). Errors never carry a path holding a user name, a row value or a secret.
- The import report holds counts only, never an email, token, password, key or proxy: it is written as `creator-import.json` next to `creator.sqlite`.
- Perf budget (M3's): each Emails window + counts, free count, Accounts window and nav counts under **300 ms** on the box, release build, imported data.
- No schema version change in M4: an index Task 4 may add is a plain `CREATE INDEX IF NOT EXISTS` in `Db::open`, `VERSION` stays 24. Risky area: real run on the imported copy.
- The import never blocks a runtime worker: it runs on a `std::thread`; page calls meanwhile return `"Importing the Python database…"`.
- Live steps run on the box under `box-env.ps1`, in the sandbox, **with the owner present**; the backup copy only, never the live Python file or stock Shard's live folder. Secrets never printed.

## Review Focus

1. **The source is touched, or its WAL is lost.** Opening `ghidorah.sqlite` with SQLite at all can checkpoint or rewrite `-shm`, which breaks rollback; skipping the `-wal` loses the newest rows. Pinned in Task 1 (`unmerged_wal_rows_are_imported_and_source_is_byte_identical`).
2. **A half copy becomes our store, or a failure loops.** A crash or error mid-copy must leave no `creator.sqlite`, so the next start retries; within one run a failure is reported, not retried per call. Pinned in Task 1 (`failed_import_leaves_no_store`, `leftover_import_file_is_replaced`, `db_reports_importing_then_the_same_failure`).
3. **Import runs again over live data.** An existing `creator.sqlite` must never be replaced, whatever sits at Python's path. Pinned in Task 1 (`existing_store_is_never_reimported`).
4. **Secrets in the report or an error.** Pinned in Task 1 (`report_and_errors_hold_no_secret`).
5. **The platform-mail scan on about 1 GB of real mail.** `breeze_free_outlook_count` scans `mail_messages` senders per call; real bodies make the table large. Measured in Task 3, fixed in Task 4.

---

## File map

```
src-tauri/src/creator/
  store/mod.rs     + python_db_path, ImportReport, import_python                         (T1)
                   + idx_mail_sender in Db::open                                         (T4, only if T3 says so)
  runner.rs        db(): background import state                                          (T1)
  runner/live.rs   live_import, live_perf, live_profiles_check, live_open_imported;
                   live_outlook_test CREATOR_LIVE_SAMPLE; live_breeze CREATOR_LIVE_MAILBOX_ID (T2)
docs/creator/
  windows-build.md the new live tests and their variables                                (T2)
  cutover.md       the M8 cutover and rollback runbook                                   (T5)
```

---

### Task 1: Import on first open

**Files:**
- Modify: `src-tauri/src/creator/store/mod.rs`, `src-tauri/src/creator/runner.rs` (`db()` and its static only)
- Test: store tests module; runner tests module for `db()`'s states

**Interfaces:**
- Produces (store):
  - `pub fn python_db_path() -> Result<PathBuf>`: `local_root()?.parent()/ghidorah-creator/ghidorah.sqlite`.
  - `#[derive(Debug, Serialize)] pub struct ImportReport { pub version: i64, pub mailboxes: BTreeMap<String, i64> /* "outlook/active" */, pub accounts: BTreeMap<String, i64> /* "<platform>/<status>" */, pub proxies: i64, pub browsers: i64, pub browsers_with_profile: i64 /* path non-empty */, pub live_accounts: i64 /* status IN ('queued','running','waiting') */, pub mail_messages: i64, pub seconds: f64 }`, counted on the result with `GROUP BY`.
  - `pub fn import_python(src: &Path, dest: &Path) -> Result<ImportReport>`: remove leftovers (`dest.src`, `dest.src-wal`, `dest.import` and their `-wal`/`-shm`); `fs::copy(src, dest.src)` and, only if `src-wal` exists, `fs::copy(src-wal, dest.src-wal)` (never `-shm`); open `dest.src` (read-write is fine: it is our copy) and check `user_version`; `VACUUM INTO ?1` = `dest.import`; open it, check version + `quick_check`, count, close; rename to `dest`; write `creator-import.json` beside it. Temp files are removed on success and on every error.
  - `pub fn open_or_import(dest: &Path, python: &Path) -> Result<(Db, Option<ImportReport>)>`: `dest` exists → `Db::open(dest)`, `None`. Missing and `python` exists → `import_python` then `Db::open`. Both missing → `Db::open` (fresh v24, as now).
- Produces (runner): `db()` keeps its signature. State in its static: `enum DbState { Ready(Arc<Db>), Importing, Failed(String) }` (`None` = not started). First call: `dest` exists or `python` missing → open inline as now. Otherwise set `Importing`, start a `std::thread` running `open_or_import`, return `Err("Importing the Python database…")`; the thread sets `Ready` or `Failed(format!("{e:#}"))`. `Importing` → the same error text; `Failed(t)` → `Err(t)` on every call until the app restarts (no re-import per call). No runtime worker ever blocks on the import.

- [ ] **Step 1: Failing tests** (fixture helper `python_fixture() -> TempFile`: a `Db::open` file plus plain-SQL rows: 2 outlook mailboxes (active, gone), 1 ele; accounts `breezeblue/linked`, `qwen/running`, `grok/failed` with `frozen=1`, `google/linked`; 1 proxy with password `PXSECRET`; 1 browser with path `pid-1`; mailbox token `TOKSECRET`; 3 mail rows):
  - `import_copies_everything_and_reports_counts`: `report.mailboxes == {"ele/active":1,"outlook/active":1,"outlook/gone":1}`, `accounts` holds the 4 keys above with 1 each, `proxies == 1`, `browsers_with_profile == 1`, `live_accounts == 1`, `mail_messages == 3`; `Db::open(dest)` works, `email_list` totals 3, `browser_get(1).path == Some("pid-1")`.
  - `unported_platform_accounts_list_and_count`: on the result, `nav_counts().0 == 4` and `account_list` (no filter, then tab `grok`, then tab `google`) returns them without error, the frozen grok row included.
  - `unmerged_wal_rows_are_imported_and_source_is_byte_identical`: a writer connection on the fixture with `PRAGMA wal_autocheckpoint=0` inserts 2 more mailboxes and **stays open** during the import; result has 5 mailboxes; `ghidorah.sqlite` and its `-wal` hash the same before and after; no temp file left.
  - `import_refuses_other_versions`: source at `user_version=23` → error text `"ghidorah.sqlite is version 23, expected 24"`; no `dest`, no temp file.
  - `failed_import_leaves_no_store`: a source of 4 KB of random bytes → `Err`; no `dest`, no temp file.
  - `leftover_import_file_is_replaced`: junk `dest.import` and `dest.src` beforehand → import succeeds.
  - `existing_store_is_never_reimported`: `dest` exists with 0 mailboxes, `python` exists → `open_or_import` returns `None` and still 0 mailboxes.
  - `open_or_import_without_python_makes_a_fresh_store`.
  - `report_and_errors_hold_no_secret`: the serialized report and `creator-import.json` contain neither `TOKSECRET`, `PXSECRET` nor any fixture email; the version error contains no path.
  - `python_db_path_sits_beside_our_local_root` (under `crate::store::test_root`).
  - `db_reports_importing_then_the_same_failure` (runner, under `test_root`, `migrate::serial()`): a corrupt Python file → first `db()` is `Err("Importing the Python database…")`; poll until it changes → the integrity/open error; a second call returns the same text and the file is not copied again (its temp files never reappear). A good fixture instead → eventually `Ok`, 3 mailboxes. The test resets the static between cases through a `#[cfg(test)]` helper.
- [ ] **Step 2:** Box: `cargo test --manifest-path src-tauri/Cargo.toml creator::` → the new tests fail.
- [ ] **Step 3:** Implement as the Interfaces block says.
- [ ] **Step 4:** Box: `cargo test … creator::` PASS; `rustfmt --edition 2021 --check` clean; `check-seams` shows no new file.
- [ ] **Step 5:** Commit `Creator: import the Python database on first open of the store`.

### Task 2: Live harness for the rehearsal

**Files:**
- Modify: `src-tauri/src/creator/runner/live.rs`, `docs/creator/windows-build.md` ("Live" section: one paragraph per new test with its variables)

**Interfaces:** consumes Task 1's `open_or_import`, `ImportReport`. All tests are `#[ignore]`, print counts and timings only (no email, token, proxy, path of the owner's data).

- `live_import`: `CREATOR_LIVE_IMPORT` (backup copy), `CREATOR_LIVE_OUT` (must not hold `creator.sqlite`, else panic "out already holds a store") → `open_or_import(out/creator.sqlite, import)`, prints the report JSON.
- `live_perf`: `CREATOR_LIVE_OUT` → opens the store; runs each query 5 times; prints `PERF <name> median_ms=<n> OK|SLOW` (SLOW over 300 ms). Queries: `email_list` first window (limit 100) unfiltered, type `outlook`, status `active`, status `gone`, free for `breezeblue`, search `"a"`; `breeze_free_outlook_count`; `has_platform_mail` on the newest outlook mailbox; `account_list` first window; `nav_counts`. Also prints `EXPLAIN QUERY PLAN` of the free count (no values).
- `live_profiles_check`: `CREATOR_LIVE_OUT`, `CREATOR_LIVE_SHARD_PROFILES` (a profiles folder) → prints `browsers_with_profile`, `found` (a `<id>.json` there), `missing`.
- `live_open_imported`: `CREATOR_TEST_BROWSER`, `CREATOR_LIVE_FINGERPRINTS`, `CREATOR_LIVE_OUT`, `CREATOR_LIVE_SHARD_COPY` (a folder with `profiles/` and `user-data/` copied from stock Shard), `CREATOR_LIVE_BROWSER_ID` → the same test-root setup as `live_breeze`, copies the two folders into `profiles_dir()`/`user_data_root()`, `shard::open_session(&db, id, proxy of the row)`, **never navigates to the platform** (a visit could rotate the session and sign out the production copy): reads the cookies with CDP `Storage.getCookies` on the open page and counts those of the platform's domains, reads the timezone on the fresh tab as it is (a neutral `about:blank`, no site), prints `OPENED timezone=<tz> cookies=<count>`, stops the profile.
- `live_outlook_test`: with `CREATOR_LIVE_SAMPLE=N` and no combo file, tests N random active Outlook mailboxes of the store that have no account (`ORDER BY random()`), 5 at a time, as now.
- `live_breeze`: `CREATOR_LIVE_MAILBOX_ID=<id>` runs exactly that Outlook mailbox (count forced to 1); not free → panic "mailbox <id> is not free for Breeze Blue" before any browser opens.

- [ ] **Step 1:** Write the tests and the doc paragraph.
- [ ] **Step 2:** Box: `cargo test --lib creator::runner::live -- --list` shows them; `cargo test … creator::` PASS (ignored ones not run); rustfmt clean.
- [ ] **Step 3:** Commit `Creator: live tests for the database import rehearsal`.

### Task 3: Sandbox rehearsal on the box (owner present)

No code. Results go to the owner report, never the repo. Every command under `box-env.ps1`, release-profile test build (`--release`) for `live_perf`.

- [ ] **Step 1: Baseline.** `live_perf` on the M3 sandbox store (`CREATOR_LIVE_OUT` = the M3 out folder). Record the numbers ("before").
- [ ] **Step 2: Backup copy.** With the owner, either with the Python app closed (then copy `ghidorah.sqlite`, plus `-wal` only if it exists, never `-shm`), or while it runs through SQLite's backup API (`python -c "import sqlite3; s=sqlite3.connect('file:<src>?mode=ro', uri=True); d=sqlite3.connect('<dest>'); s.backup(d)"`). Note its size and `user_version` (must be 24).
- [ ] **Step 3: Import.** `live_import` into a new, empty out folder. Record `seconds`. Compare the report with what the Python app shows (Emails type/status counts, Accounts per platform, Proxies). Expected: equal. `live_accounts` is reported, not required to be 0 here.
- [ ] **Step 4: Perf "after".** `live_perf` on it. Every query OK → skip Task 4. Any SLOW → Task 4, then rerun this step.
- [ ] **Step 5: Profile mapping.** With the owner's OK (D-M4-4): `live_profiles_check` with `CREATOR_LIVE_SHARD_PROFILES` = stock Shard's `profiles` folder (read-only listing). Expected `missing` near 0; explain any non-zero (a profile deleted or trashed in Shard). Then copy 2 non-running account profiles (json + user-data folder) into the sandbox and `live_open_imported` each: expected `OPENED`, the proxy's timezone, platform cookies > 0. No navigation to any platform site from a copied production profile.
- [ ] **Step 6: Mail.** `live_outlook_test CREATOR_LIVE_SAMPLE=20`: expected the gone/active split close to the stored one; every failure explained.
- [ ] **Step 7: One Breeze.** The owner picks one free Outlook mailbox from the imported data (Make's free list); `live_breeze CREATOR_LIVE_MAILBOX_ID=<it>`: expected `linked`.
- [ ] **Step 8:** Report to the owner: result, numbers before/after, what is unverified. Delete the sandbox copy of the production DB when the owner says so.

### Task 4 (only if Task 3 shows SLOW): index for the platform-mail scan

**Files:**
- Modify: `src-tauri/src/creator/store/mod.rs`
- Test: store tests module

**Interfaces:**
- `Db::open` runs `CREATE INDEX IF NOT EXISTS idx_mail_sender ON mail_messages(sender, mailbox_id)` after the version check, on every open. `VERSION` stays 24; no migration list.
- Why: the free condition's `NOT IN (SELECT x.mailbox_id FROM mail_messages x WHERE x.sender LIKE …)` needs only `sender` and `mailbox_id`; a covering index turns a scan of the mail bodies into a scan of a small index. If a SLOW query is not the free count, fix it the same way (an index that covers it); only if an index cannot, Python's revision cache (`revs`, already in v24) for that count.

- [ ] **Step 1: Failing tests:** `open_adds_the_sender_index_and_stays_v24`; `free_count_scans_the_covering_index` (`EXPLAIN QUERY PLAN` of `breeze_free_outlook_count`'s SQL contains `USING COVERING INDEX idx_mail_sender`); `new_db_is_v24_and_matches_python_tables` still passes (it compares tables, not indexes; if it compares indexes, allow `idx_mail_sender`).
- [ ] **Step 2:** Box: tests fail. **Step 3:** Implement. **Step 4:** Box: `cargo test … creator::` PASS, rustfmt clean.
- [ ] **Step 5:** Real run: `live_import` into a new out folder then `live_perf` (Task 3 Step 4) → all OK; record the first-open index seconds.
- [ ] **Step 6:** Commit `Creator: sender index so the platform-mail rule reads no mail bodies`.

### Task 5: Cutover runbook (executed at M8)

**Files:** Create `docs/creator/cutover.md` (no hosts, user names or IPs: "the Windows test box", `%LOCALAPPDATA%`, `%APPDATA%`).

Content, as numbered steps:

1. **Preconditions:** every provider ported and live-gated in our app (D-M4-1); the Python backup's `user_version` is 24; our installer built (M8); the Task 3 rehearsal repeated on a fresh backup with this build; the installed stock ShardBrowser version noted, its installer kept, and our build's version not lower (a lower one would downgrade Shard's data); the owner present; free disk for the backups.
2. **Quiesce Python:** stop every job, wait until no account is making, close the Python app normally. Its next-start sweep is not available afterwards, so Accounts must show 0 making (the import report's `live_accounts` must be 0; otherwise start Python once, let its sweep run, close it).
3. **Close stock ShardBrowser** normally (Quit; no profile running).
4. **Back up**, with a file count and size check after each copy: `%LOCALAPPDATA%\ghidorah-creator`, `%APPDATA%\shardx-launcher` (config, `proxies.json`, profiles), `%LOCALAPPDATA%\shardx-launcher`, and Shard's data location if it was moved in Settings (profiles, user-data, extensions).
5. **Install ours:** first check that no `%LOCALAPPDATA%\shardx-launcher\creator.sqlite` exists (one would stop the import; move it aside). Install over stock (same app id: in-place upgrade); if stock must be uninstalled first, uninstall **without** "delete app data". Never tick "delete app data".
6. **First start:** the import runs in the background (one to two minutes at about 1 GB; our pages say "Importing the Python database…"). Check `creator-import.json` exists and was written today (else the import did not run): counts equal to the Python app's last screen; `live_accounts` 0. Shard's All profiles count equals the pre-cutover count; folder "Ghidorah" present.
7. **Verify live:** Test 20 mailboxes; open 2 imported accounts' profiles from Shard (logged in, proxy timezone); 1 Breeze Blue on an imported free Outlook mailbox; 1 run of each provider. A failure here → rollback or fix, owner's call.
8. **Retire Python:** uninstall it and remove its shortcut; keep its data folder backup (step 4) until the owner deletes it.
9. **Rollback** (any time before the owner declares the cutover done): close ours; uninstall it; restore the step 4 folders over the current ones (replace, not merge); reinstall the kept stock installer; start the Python app. Its DB was only read, so it continues where it stopped. Anything done in our app since cutover is lost (move our `creator.sqlite` aside first if it should be kept).

- [ ] **Step 1:** Write `docs/creator/cutover.md`; link it from `working-process.md` §7 "Release: M8".
- [ ] **Step 2:** Commit `Creator: cutover and rollback runbook for the M8 switch`.

---

## Execution notes

- Order: T1 → T2 → T3 → (T4 → T3 Step 4 again) . T5 is docs only and can run beside T1 (different files).
- Models: T1 sonnet, reviewer opus (store and import are a risky area). T2 sonnet. T3 is the manager on the box with the owner. T4 sonnet, reviewer opus. T5 haiku or sonnet, reviewer opus (a wrong rollback step costs the production data).
- The box fetches from GitHub: push the task branch only on the owner's word (`box-workflow.md`).
- M4's exit gate (replaces the roadmap's "Python keeps running"): import of a backup copy reports the same counts as the Python app; every `live_perf` query under 300 ms; 2 imported profiles open; 20-mailbox Test and 1 Breeze on the imported store pass; the runbook is reviewed.

## Risks

- **A `browsers.path` id Shard no longer has:** the v24 migration deleted every older browser row, so a missing id means the profile was deleted or trashed in Shard. Code already turns it into "profile is gone" and makes a new profile (the account then needs Log in again). Task 3 Step 5 counts them.
- **Unknown extension ids on a copied profile** (YesCaptcha library copies are not in the sandbox): may refuse or warn at launch in the sandbox only. At cutover the library is Shard's own and present.
- **Rows Python left mid-run:** `queued`/`running` shown as "making", `waiting` raw, until M5's startup sweep exists; the runbook requires 0 at cutover.
- **First start is slow once:** the copy, vacuum and check of about 1 GB take a minute or two (Task 3 measures it); pages show "Importing the Python database…" meanwhile and need a refresh after. Accepted; no progress UI. Disk needs about 3x the file size free during the import (temp copy + result).
- **Token rotations between rehearsal and cutover:** the sandbox's Tests rotate refresh tokens only in the sandbox; Microsoft keeps the older ones valid (M3 Risks), and the cutover copies Python's.
