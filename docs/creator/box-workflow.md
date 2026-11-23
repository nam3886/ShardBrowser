# Windows test box workflow

Code is edited on the Mac, which has no Rust toolchain. Everything else (build, all tests, dev runs, live runs) happens on the Windows test box, which fetches from GitHub and never pushes. Toolchain, build, sandboxing and the live harness are in `windows-build.md`; the gates are in `working-process.md` §7. This page holds only the box procedure and its gotchas.

## Flow

1. Mac: commit and push the branch to `origin` (only on the owner's word).
2. Box (ssh, PowerShell). Fetch first, then load the environment: `box-env.ps1` redirects `HOME`, which loses the SSH alias for GitHub.

   ```powershell
   cd <checkout>
   git fetch origin
   . <sandbox>\box-env.ps1
   git checkout <branch>; git pull --ff-only
   ```

3. Run PowerShell scripts with `-ExecutionPolicy Bypass` (the default policy blocks them).
4. Run the full gate list from `working-process.md` §7. The Mac has no toolchain, so the box is the gate: a branch must show no new failures against the Windows baseline below (§7's "0 failed / clean" is the target; the baseline is the known exception).

## Box setup facts

- The checkout has `core.autocrlf=false` set locally (otherwise line endings dirty the tree and break format and seam checks).
- `CARGO_TARGET_DIR` is one shared dir on the data drive (set by `box-env.ps1`). When that drive has under 20 GB free, run `cargo clean` before building. Delete only what you created.
- The seam check (`node scripts/creator/check-seams.mjs`) needs a local `upstream/main` ref: fetch ProxyShard/ShardBrowser into it (`git fetch <upstream-url> main:refs/remotes/upstream/main`).
- Test binaries need the Common-Controls v6 manifest runner, which `box-env.ps1` sets (see `windows-build.md`, "Tests on Windows"). Without it they die with `0xc0000139`; the first run after a fresh build may still need one rerun.

## Known baseline on Windows

A branch passes what `main` passes. On `main`:

- `cargo test`: five `reset::tests::*` (they assume a macOS home layout) and one `extensions::updates…a_silent_store_times_out…` fail. Known flaky upstream tests under load: `space::sync…`, `space::cache…`, `gpu_caps…`, `extensions::usage::a_launch_and_a_swap_never_overlap`; rerun them alone before blaming a branch.
- `npm run check:classes` has known failures in upstream files.

## Live runs

Use the ignored `creator::runner::live::live_breeze` harness (see `windows-build.md`), started as a scheduled task in the interactive desktop session, sandboxed, and only with the owner's approval. The built exe cannot run beside stock ShardBrowser (same app id, single instance).

## Never

Touch the production stock ShardBrowser (app, data, processes) or the Python app on the box. Stop only processes you started, by PID. Never print secrets.
