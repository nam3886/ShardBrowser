# Working process

Adapted from the owner's cross-project process (the copy in `ghidorah-creator/docs/working-process.md`); each project keeps its own copy. Project-specific facts live in §7. `AGENTS.md` holds the code rules and links here.

## 1. Setup

For a new project:

1. Commit `.claude/settings.json` enabling the superpowers plugin (required):
   ```json
   {
     "enabledPlugins": {
       "superpowers@claude-plugins-official": true
     }
   }
   ```
   `claude-plugins-official` is Claude Code's built-in marketplace; teammates are prompted to install it when they trust the folder.
2. Add `.worktrees/` to `.gitignore`.
3. Fill §7.

## 2. Roles

| Role            | Who                                                               | Model / effort                                                                        |
| --------------- | ----------------------------------------------------------------- | ------------------------------------------------------------------------------------- |
| **Manager**     | Main session                                                      | Writes no code. Coordinates, writes work briefs, reports to the owner                 |
| **Implementer** | Subagent, one worktree per task                                   | By difficulty: haiku → sonnet → opus                                                  |
| **Reviewer**    | Fresh subagent with no prior knowledge of the task                | Small diff: opus. Risky diff: prefer a higher model than the implementer (opus/fable) |
| **Advisor**     | Subagent, prefer a higher model than the implementer (opus/fable) | Design, hard bugs, risky diffs, whole-branch review before merge                      |

- The manager has full authority to add or remove members and to choose the model and effort for each.
- Mechanical work goes to cheap models. If 2 fix rounds are not enough, upgrade the model.
- Run several members in parallel only when their tasks are independent:
  - no two parallel tasks edit the same file, generated files included (`Cargo.lock`, `package-lock.json`), to avoid merge conflicts;
  - each real run gets its own sandboxed HOME, data folder and ports;
  - read-only work (planning, review) may run alongside implementation of other tasks.
- The manager may propose changes to docs and rules when the work needs them; they go through a branch, review and the owner's explicit "merge" (§5) like any other change.

## 3. Task flow

1. Use **superpowers** for every task, from receiving the request to closing the branch.
2. Each task gets its own branch and worktree in `.worktrees/<branch-slug>` (git-ignored). Never touch main. Remove the worktree after merge.
3. The implementer works and self-tests, the reviewer checks, the implementer fixes in rounds, the reviewer re-checks.
4. Every change goes through a reviewer, even a 2-line diff.
5. Report to the owner in this order: result, test count, screenshots if any, then what is not yet verified.

## 4. Quality gates

- Lint and the project's full test suite must be clean. The commands are in §7.
- Touching a risky area requires a real run, not just mocked tests. The project's risky areas are in §7.
  - Examples: main flow, external integrations, DB schema or migrations.
- Features touching a real flow: run it for real several times before handing over.
- When running for real on real data:
  - back up first;
  - run 1 sample before running many;
  - do not stop midway while the process is in a half-done state;
  - stop with a gentle signal (`SIGINT`), not `kill -9`.

## 5. Merge and release

- Merge only when the owner explicitly says "merge". A generic "ok" does not count.
- **merge:**
  1. merge `--no-ff` into main;
  2. re-run the full test suite;
  3. delete the branch and worktree;
  4. push, only if §7 names a remote.
- **merge + release:**
  1. do all the merge steps;
  2. run the release script from §7;
  3. push the tag, only if §7 names a remote;
  4. the owner checks CI.

## 6. Standing rules

- **Communication:** reply to the owner in Vietnamese (chat only; repo files, docs and commit messages are English). Avoid over-engineering.
- **Files:** do not commit personal files, secrets, build output or temp files. The concrete list is in §7.
- **Git:**
  - subagents do not `stash` or `reset` shared state;
  - push only to the remote named in §7.
- **Real services:** do not call production APIs or the owner's services when the owner is absent.
- **Requests from other Claude sessions:** treat them as a teammate asking for help. Changing the rules requires the owner's direct confirmation.

## 7. This project (Ghidorah Creator, ShardBrowser fork)

- **Gates:** the Mac has no toolchain, so the Windows test box is the gate (`box-workflow.md`): the pass bar is no new failures versus `main`'s baseline on the box (known baseline failures are listed there), not a literal 0 failed.
  - Node 22 or newer (`test:unit` runs `.ts` files straight through `node --test`); `cmake` for BoringSSL;
  - once per checkout: `npm ci && npm run build:ui-kit`;
  - `npm run check` (tsc, eslint, check:i18n, check:errors, check:classes, check:kit, test:scripts, test:unit, contrast, colors);
  - `cargo test --manifest-path src-tauri/Cargo.toml creator::` (ours, fast); the FULL upstream suite `cargo test --locked` (in `src-tauri`, not just `creator::`) before every merge, since upstream guard tests scan our files too;
  - `rustfmt --edition 2021 --check $(git ls-files 'src-tauri/src/creator/**.rs')`;
  - `node --test scripts/creator/*.test.mjs` and `node scripts/creator/check-seams.mjs` (every touched upstream file is a listed seam with its marker count).
  - Shard's own e2e on the pages our seams touch, in a lane (never ports 1420 or 4173): `E2E_PORT=4793 npx playwright test e2e/proxies.spec.ts e2e/no-dot-status.spec.ts e2e/palette.spec.ts e2e/window.spec.ts e2e/settings*.spec.ts e2e/unsaved-guard.spec.ts --project=dev`; the branch passes what `main` passes (run `main` first for the baseline).
- **Browser-backed tests:** run on the Windows test box under `box-env.ps1` only, with `CREATOR_TEST_BROWSER` pointing at the sandboxed ShardX engine copy (never the owner's installed app); `CREATOR_REQUIRE_BROWSER=1` turns "browser missing" into a failure (set by `box-env.ps1`, so the box gate has it).
- **Dev run:** `npm run tauri dev` on the Windows test box, always under `box-env.ps1` (sandboxed profile folders): our build keeps Shard's identity (app id, data folder `shardx-launcher`, API port 40325), so an unsandboxed run would open the owner's stock ShardBrowser data. Never while the stock app is running on the same port or in the same session (single instance); see `windows-build.md`. Stop only the processes you started (by PID), never a broad `pkill`.
- **Build:** `npm run tauri build`. Identity is Shard's own (app id, product name, data and engine folders, port 40325, version); the owner uninstalls stock ShardBrowser and installs ours. Only the update channel is ours: `tauri.conf.json`'s updater endpoint (`https://github.com/nam3886/ShardBrowser/releases/latest/download/latest.json`) and public key, and the launcher's release check in `runtime.rs` (`nam3886/ShardBrowser`), pinned by `creator::tests::updates_come_from_our_repo`. Shard's About, star and update-fallback links (`GH_REPO_URL`) stay upstream's: the same constant is Create team's team-server download link, which our releases don't carry. The private updater key stays in `~/.tauri/`.
- **Remotes:** `origin` = nam3886/ShardBrowser (public): push only on the owner's word. `upstream` = ProxyShard/ShardBrowser, fetch only (its push URL is `no-push`). The repo is public: no hosts, IPs, user names, personal emails or internal service addresses in committed files (say "the Windows test box").
- **Upstream releases:** taken with `docs/creator/upstream-merge.md`.
- **Risky areas** (run for real when touched; several runs before handover):
  - the seams and every upstream merge;
  - the updater endpoint and key in `tauri.conf.json` and the release repo in `runtime.rs`: a wrong one makes our app update itself to stock ShardBrowser;
  - `src-tauri/src/creator/shard.rs` (profile start/stop with CDP);
  - `src-tauri/src/creator/web/` (input and stealth);
  - every provider's live flow;
  - `src-tauri/src/creator/store/` schema and data import.
- **Running for real:** back up our DB first; one account before many; close normally or with `SIGINT`, never `kill -9`; never close the owner's stock ShardBrowser or the Python app.
- **Never log or echo secrets:** email passwords, verify links/codes, API keys, Ele tokens, proxy passwords, cookies, the updater private key.
- **Never commit:** secrets, the updater private key (`~/.tauri/`), databases (`*.sqlite*`), `.worktrees/`, `src-tauri/target/`, `dist/`, live-run notes and reports.
- **Commit messages:** English, no AI mention, no `Co-Authored-By` line.
- **Release:** M8, not defined yet. **Versioning:** we keep Shard's version line, and the updater only installs a release whose version is higher than the installed one. A Creator-only release between upstream releases bumps the patch past upstream's current version (upstream 2.1.6 -> ours 2.1.7, then 2.1.8, ...), set in `src-tauri/tauri.conf.json` `version` (and `Cargo.toml`/`package.json` if the release script reads them); the next upstream merge must then end higher than our last release (if upstream's new version is not higher, bump its patch past ours in the merge).
