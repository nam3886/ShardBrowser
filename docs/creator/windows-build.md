# Windows build

How the Creator fork is built on Windows (exe only; installers come with the release work in M8).

## Toolchain

Recorded on the test machine, 2026-11-23:

| Tool | Version |
|---|---|
| Windows | 10 Pro 22H2 (10.0.19045) |
| Visual Studio Build Tools 2022 (Desktop C++, Windows SDK 10.0.26100) | 17.14.45 |
| LLVM / clang (from the Build Tools, for BoringSSL's bindgen) | 19.1.5 |
| CMake | 4.5.1 |
| NASM | 3.02 |
| Rust (stable, `x86_64-pc-windows-msvc`) | rustc 1.100.0, cargo 1.100.0 |
| Node / npm | 22.23.5 / 10.9.9 |
| Git for Windows | 2.56.1 |
| WebView2 runtime | 157.0.4341.60 |

The build needs these on `PATH` (Rust, Node, CMake, Git, NASM) and `LIBCLANG_PATH` pointing at the Build Tools' `VC\Tools\Llvm\x64\bin`. Put `CARGO_TARGET_DIR` on a drive with room: one target dir holds tens of GB across branches; `cargo clean` it when the drive drops under 20 GB free.

## Build

From the repo root, in PowerShell:

```powershell
npm ci
npm run build:ui-kit
npx tauri build --no-bundle
```

The exe is `$env:CARGO_TARGET_DIR\release\shardbrowser-launcher.exe` (`src-tauri\target\release\` without `CARGO_TARGET_DIR`). A clean release build took about 45 minutes on the test machine.

## Running it next to a stock ShardBrowser

Our build keeps Shard's identity: data folder `shardx-launcher`, app id `com.shardx.launcher`, API port 40325. On a machine that also runs stock ShardBrowser:

- Run ours only with `APPDATA`, `LOCALAPPDATA`, `USERPROFILE`, `HOME` and `TEMP` redirected to a sandbox folder, so its data never lands in the stock app's folders. Check that the data paths resolve inside the sandbox before the first run.
- The app is single-instance per Windows session (same app id as stock): in the session where stock ShardBrowser runs, starting ours only focuses the stock window. Close stock first, or use the live harness below instead of the app.
- If both apps must run at once, turn our API off (`api_enabled: false` in the sandbox's `settings.json`) or give it another `api_port`, so it does not take 40325 from the stock launcher.

## Tests on Windows

Rust test binaries need a Common-Controls v6 manifest, or they die at start with `0xc0000139` (`STATUS_ENTRYPOINT_NOT_FOUND`, `TaskDialogIndirect`). Set a cargo runner (`CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUNNER`) that embeds one with `mt.exe -manifest <comctl6.manifest> -outputresource:"<exe>;#1"` before running the binary. Browser-backed tests need `CREATOR_TEST_BROWSER` (a ShardX `chrome.exe` copy inside the sandbox) and `CREATOR_REQUIRE_BROWSER=1`.

## Live Breeze Blue run

`creator::runner::live::live_breeze` (ignored) makes real accounts through the app's own run path (`begin_job` + `make_breeze` on Shard profiles) without starting the app. Its header lists the variables: the engine, a fingerprint library folder, a copy of the Python `ghidorah.sqlite` to import proxies and Ele settings from, an output folder that keeps our store between runs, the count, the proxy mode and the mailbox type (`CREATOR_LIVE_MAILBOX=ele|outlook`; Outlook takes the store's free mailboxes, oldest first). `creator::runner::live::live_outlook_test` (ignored) pastes a file of Outlook combo lines into the same store and tests each new mailbox once, 5 at a time; it needs no browser. `creator::runner::live::dev_bridge` (ignored) answers our pages' commands over HTTP on a fresh sandboxed store, so `npm run dev:mock` with `VITE_CREATOR_BRIDGE=http://127.0.0.1:4795` shows our real pages from an SSH session; the built app cannot run there (WebView2 needs the desktop session, where stock ShardBrowser holds the single instance). Name the test: the module holds several.

```powershell
cd src-tauri
cargo test --locked --lib creator::runner::live::live_breeze -- --ignored --nocapture --test-threads=1
```

Run it in an interactive desktop session (for example a scheduled task with "run only when the user is logged on"), not from a non-interactive SSH session, so the browser windows have a desktop. Import from a copy of the Python database (with its `-wal` and `-shm`), never from the file the Python app has open.
