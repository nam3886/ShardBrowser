# Spike: ghidorah-creator as a fork of the ShardBrowser launcher

Date 2026-11-22. Local clone only (remote `upstream` = ProxyShard/ShardBrowser, nothing pushed).
Branches: `creator-spike` (skeleton on upstream/main), `sim-2.1.5` (first merge simulation),
`sim-2.1.5-eof` (final seam placement, merged v2.1.6 and upstream/main cleanly).

## 1. Build and run (macOS arm64)

- Toolchain: rustup stable 1.100 (user level), Node 20 + npm (the repo uses `package-lock.json`, not pnpm).
- Extra tool needed: **cmake** (`brew install cmake`). `wreq` -> `btls-sys` builds BoringSSL from source.
- Steps: `npm ci && npm run build:ui-kit`, then `cargo build` in `src-tauri` (cold: ~6 min,
  incremental 40 s). `npm run tauri dev` would do the same (`beforeDevCommand: npm run dev`, Vite on :1420).
- Ran the debug binary with `HOME=.spike-home` so it could not touch the owner's installed
  ShardBrowser Launcher 2.1.6 data (`~/Library/Application Support/shardx-launcher`, 378 MB).
- Result: the launcher starts, shows its UI, the API listens on `127.0.0.1:40325`, the engine downloads on first
  run without keys (`ShardX-Mac-arm64.zip`, 357 MB, from R2 `pub-e57a7c60...r2.dev`; the manifest is
  `raw.githubusercontent.com/ProxyShard/ShardBrowser/main/runtime.json`). A profile created via `POST /profiles`
  started with `POST /profiles/:id/start` and returned a CDP endpoint. Network is needed for the manifest and
  engine; there are no keys or accounts. A first-run tour and a "100 MB of residential proxy traffic, free" upsell modal appear.

## 2. Architecture map

| Concern | Where |
|---|---|
| Profile list/CRUD | `src-tauri/src/space/local/profile.rs` (`ProfileMeta`, `list_all`), Tauri cmds in `src-tauri/src/lib.rs` (`profile_list`, `profile_save`...), API `persist_created` in `api.rs` |
| Start | `launch.rs`: `launch_profile_unlocked(id, enable_cdp, headless, bus_port, bus_token, password)` -> `launch_using` -> `start_browser`. UI start = `lib.rs::launch` (cdp **off**); API start = cdp **on** |
| Stop | `process.rs` `Tracker::shared().kill(id)` |
| Fingerprint | `launch.rs::start_browser` writes `<udd>/fingerprint.json`, passed as `--fingerprint-profile=`; `resolve_auto_fields` fills "auto" tz/locale/geo from the proxy |
| Proxy | `launch.rs::command_line` `--proxy-server=`, WebRTC flags `--shardx-webrtc-*`; store in `space/local/proxy.rs` |
| CDP endpoint | `--remote-debugging-port=0` + `--shardx-automation` (enables the hidden **`Motion`** CDP domain = human-like pointer/keys/touch); port read from `<udd>/DevToolsActivePort` (`launch.rs::read_devtools_endpoint`), returned as `process::CdpInfo{port,http_url,web_socket_debugger_url}` |
| Local API | `api.rs` (axum). `serve()` binds `127.0.0.1:<api_port>` (default 40325, `settings.rs:100`); HS256 JWT bearer, secret = `settings.json` `api_secret`; `/health` open; all routes 503 during data migration |
| Own CDP client | `cdp.rs` (tokio-tungstenite, used by the automation studio) and the hidden `Motion` calls in `runner.rs` |
| UI routing | `src/shared/types/index.ts` `SECTIONS`; `src/app/App.tsx` renders `section === "x" && <LazyPage>`; `src/widgets/Sidebar/Sidebar.tsx` `<PageItem id=...>`; state in `src/shared/model/navigation` |
| Tauri IPC | one big `generate_handler![...]` in `lib.rs::run()`; capabilities `src-tauri/capabilities/default.json` |
| Updater | `tauri.conf.json` `plugins.updater` pubkey (minisign) + endpoint `github.com/ProxyShard/ShardBrowser/releases/latest/download/latest.json`; `launcher_update.rs`; fallback check `runtime.rs` `LAUNCHER_RELEASE_REPO = "ProxyShard/ShardBrowser"`; `latest.json` is generated in `.github/workflows/release.yml` and signed with `TAURI_SIGNING_PRIVATE_KEY` |
| Engine update | `runtime.rs` (manifest on upstream GitHub main, archives on R2, `min_launcher_version` gate = 2.0.1 today) |
| Plugin system already upstream | `wasm.rs` + `docs/automation-module-example`: sandboxed WASM "automation modules" (blocks only; no page script, no machine access). Too restricted for us (mail polling, DB, captcha solvers) |
| Upstream Rust SDK | `sdks/rust` (crate `shardx`, MIT) uses **chromiumoxide 0.7** and ships `motion.rs` wrappers for the hidden domain |

## 3. Module skeleton (branch `creator-spike`)

Our code, all new files:
- `src-tauri/src/creator/mod.rs`: one Tauri command `creator_call(cmd, args)` dispatching `profiles` and `open_example`.
  `open_example` calls `launch::launch_profile_unlocked(id, cdp=true, ...)`, connects chromiumoxide to `web_socket_debugger_url`,
  `new_page("https://example.com")`, reads the title.
- `src/creator/CreatorPage.tsx`, `src/creator/index.ts`: the page lists profiles with an "Open example.com" button.
- `creator-spikes/cdp-probe/`: a standalone chromiumoxide probe.

**Upstream files edited (the seams), 11 lines total:**

| File | Lines | Edit |
|---|---|---|
| `src-tauri/src/lib.rs` | 2 | `mod creator;` on line 2 (top of file), `creator::creator_call,` first entry in `generate_handler!` |
| `src-tauri/Cargo.toml` | 5 | `[dependencies.chromiumoxide]` table at end of file |
| `src-tauri/Cargo.lock` | generated | regenerate, never hand-merge |
| `src/shared/types/index.ts` | 1 | `"creator"` in `SECTIONS` |
| `src/app/App.tsx` | 2 | import + one render line |
| `src/widgets/Sidebar/Sidebar.tsx` | 1 | `<PageItem id="creator" ...>` |

Verified live (dev build, sandbox HOME): pressing the button in the Creator page started the profile and showed
`"Example Domain" at https://example.com/ (cdp http://127.0.0.1:50274)`. The first click on an already-running profile
returned "profile ... is already running", so the real module needs an "attach to the running profile" path
(`process::Tracker` knows the CdpInfo; UI-started profiles have no CDP at all).
`tsc` and eslint pass; upstream `check:i18n` flags the 2 hard-coded "Creator" strings (locale JSONs are high-churn files, so our strings should be kept in our own dictionary).

## 4. CDP from Rust

Probe output against the live ShardX engine (`creator-spikes/cdp-probe`):
```
title = Some("Example Domain")
find_element try 1: ok, text=Ok(Some("Learn more")), box=Ok(BoundingBox { x: 591.5, y: 708.9, width: 82.0, height: 19.0 })
evaluate links = Ok(Some(1))
Motion.createPointer -> Ok(Raw(Object {}))
Motion.glideTo -> Ok(Raw(Object {"durationMs": Number(718.216)}))
cookies(example.com) = Ok(0)
frames = Ok(1)
```
(chromiumoxide reports a missing element as the confusing `-32000 Could not find node with given id`.)

The ghidorah code uses patchright (a Playwright fork) over `connect_over_cdp`. The table maps what it uses onto chromiumoxide:

| Used in ghidorah | chromiumoxide | Rating |
|---|---|---|
| CSS/xpath locators (~100), `.first/.nth`, `count`, `is_visible` | `find_element(s)`, `find_xpath`; visibility via JS | easy |
| `get_by_role` (~70), `get_by_text` (~36), `.filter(has_text)` (~48) | **missing**; needs our own role/text engine (inject a JS helper; Playwright's role engine is the reference) | hard (largest port item) |
| Auto-waiting click/fill/press/type | no actionability checks; write `wait_until(visible, stable, enabled)` + click | medium |
| Polling waits (ghidorah mostly polls already), `wait_for_load_state`, `expect_popup` | `wait_for_navigation`, target events | easy-medium |
| `page.on(response/request)` + `response.json()` (~9) | `event_listener::<EventResponseReceived>` + `Network.getResponseBody` | medium |
| `context.request` / API request context (3) | use `reqwest`/`wreq` with cookies pulled via `Network.getCookies` | easy |
| Frames: `page.frames` + `frame_element` (Turnstile, GSI iframe) | `frames()`, but OOPIF cross-origin frames need target auto-attach + per-session commands | hard |
| `page.mouse` move/down/up, bezier drags, `bounding_box` | `Input.dispatchMouseEvent`, `bounding_box()`; **better: hidden `Motion` domain** (glide/drag/tap/enter_text) works via raw commands | easy |
| cookies (`context.cookies`, `clear_cookies(domain)`), local/sessionStorage via evaluate | `Network/Storage.*` generated types | easy |
| `add_locator_handler` overlay auto-dismiss (2) | **missing**; our own pre-action hook | medium |
| popups/multiple pages, `bring_to_front` | `pages()`, `Target.targetCreated` events | medium |
| screenshots | `page.screenshot` | easy |
| patchright stealth (isolated worlds, no `Runtime.enable` leak) | chromiumoxide enables Runtime by default; needs checking against detection | **risk** |

Alternatives: `headless_chrome` (sync, older, weaker event model; no), raw `tokio-tungstenite` (already in the tree as `cdp.rs`, fine for simple calls but rebuilds everything), and
**upstream's own `shardx` SDK crate** (`sdks/rust`, chromiumoxide 0.7 + `Motion` wrappers). Recommended: depend on
`sdks/rust` by path (feature `control`) and pin chromiumoxide 0.7 to match it, instead of the 0.9 used in this spike.

Node Playwright instead: playwright-core needs no browser download (connectOverCDP), but the app must ship a Node runtime
as a Tauri sidecar (`externalBin`): node.exe ~80 MB or a `bun build --compile` exe ~60-100 MB, plus IPC between Rust and Node,
plus a second language. It is the shortest path to porting the providers almost line by line (same API; patchright also has a Node package).
Cost: about 2x the installer size, a process boundary, and AV/SmartScreen exposure for an extra exe.

## 5. Upstream update simulation

Diff sizes (upstream squashes each release into 1-2 commits):
- `v2.1.4..v2.1.5`: 2 commits, 60 files, +1331/-168; mostly `sdks/*` (37 %) and `src-tauri/src` (32 %).
- `v2.1.5..v2.1.6`: 1 commit, 160 files, +11240/-695; sdks (~44 %), `src-tauri/src` (16 %), `space/local` (9 %), `src/` UI (~19 %).
  Seam files touched: `lib.rs` +204 (new `mod`, new commands, **appended tests at EOF**), `App.tsx` +12, `Cargo.toml` 5, `Cargo.lock` 166, `api.rs` 440, `launch.rs` 205, locales 132 each.
- `v2.1.6..main`: 5 commits, 20 files, +114/-38.

Merge results:
- `sim-2.1.5` (`mod creator;` placed in the module list): `git merge v2.1.6` gave **1 conflict, 1 file (`lib.rs`), 2 lines**, because upstream added `mod launcher_update;` there. Trivial fix.
- Tried EOF placement: also conflicted (upstream appended `mod phone_tests` at EOF).
- `sim-2.1.5-eof` (final: `mod creator;` on line 2): `merge v2.1.6` -> **0 conflicts**; then `merge upstream/main` -> **0 conflicts**.
  `Cargo.lock` auto-merged and passes `cargo metadata --locked`; `cargo check --locked` and `tsc` are clean on the merged tree.

## 6. Windows

`.github/workflows/release.yml` builds `windows-latest`, target `x86_64-pc-windows-msvc`, with Node 22, Rust stable, `npm ci`, `npx tauri build`.
It outputs MSI + NSIS + a portable exe and signs updater artifacts with `TAURI_SIGNING_PRIVATE_KEY` (there is no Windows code signing).
`check.yml` is frontend-only (`npm run check`). For the box: VS 2022 Build Tools (Desktop C++ + Windows SDK), CMake
(BoringSSL; on Windows `btls-sys` sets `OPENSSL_NO_ASM`, so no NASM is needed), Rust stable msvc, Node 22, Git;
WebView2 runtime (present on updated Win10 22H2/Win11; the installer bootstraps it). These were read from config only, not built on the box.

## Fork-specific seams still to add (not done in the spike)

- Identity: `identifier com.shardx.launcher`, productName, updater pubkey/endpoint. Put these in an extra `tauri.creator.conf.json` passed with `tauri build --config` so no upstream file changes. (Superseded 2026-11-22: we keep Shard's identity; only the updater endpoint and key in `tauri.conf.json` are ours, see `docs/creator/working-process.md` §7.)
- `runtime.rs` `LAUNCHER_RELEASE_REPO` fallback would offer upstream releases to our build (1-line seam or a patch).
- Data dir `shardx-launcher` is hard-coded in 6 Rust files. The fork shares data with an installed upstream launcher.
  Fine if it replaces it on the box; the two cannot run at once (same port 40325, same single-instance id).
- Versioning: stay >= upstream `min_launcher_version` (runtime.json) or the engine refuses the launcher.
- Engine manifest + R2 stay upstream-controlled (closed binary): ProxyShard can change URLs/gates at any time.

## Recommendation

**Feasible with caveats.** The fork mechanics are cheap: 11 seam lines, and a real upstream release merged with 0 conflicts after placing the seams well.
The cost is the Playwright -> Rust port (role/text locators, actionability waits, OOPIF frames, patchright-level stealth).

Top risks:
1. **Automation port cost/stealth.** chromiumoxide lacks Playwright's locator/auto-wait layer and patchright's anti-detection. Mitigate with the engine's own `Motion` domain + a small locator helper, or ship Node patchright as a sidecar.
2. **Upstream control.** Engine CDN, `runtime.json` gates and min-launcher version are theirs; huge squashed releases (11k lines) make review hard; `lib.rs`/`api.rs`/`launch.rs` churn every release.
3. **Shared identity/data.** The same data dir, port and single-instance id as upstream; updater/release-check rewiring is needed so users never get upstream's update over ours.
