# chromiumoxide 0.7.0: our patches

The crates.io 0.7.0 sources (commit `fe0a01a` "vendor chromiumoxide 0.7.0
unchanged"), wired in by `[patch.crates-io.chromiumoxide]` in the creator seam
block of `src-tauri/Cargo.toml`. Every change carries a `creator patch:` comment.
`git diff fe0a01a -- src-tauri/creator-vendor/chromiumoxide` shows them all.

Goal (D13): a page cannot tell a CDP client is attached, the way patchright
does it for Playwright. Stock 0.7 sends `Runtime.enable` on every page; with it
on, V8 formats a logged error's stack for the client, which a page sees through
a hooked `Error.prepareStackTrace` (`fixtures/detect.html`,
`prepareStackTraceOnLog`).

| File | Change | Why |
|---|---|---|
| `src/handler/frame.rs` | `FrameManager::init_commands` no longer sends `Runtime.enable` | the leak above |
| `src/handler/target.rs` | no `Page.addScriptToEvaluateOnNewDocument("//# sourceURL=____chromiumoxide_utility_world___evaluation_script__", worldName)` (the utility world) at target init | a script in every document, named after a driver |
| `src/handler/target.rs` | `page_init_commands`: no `Performance.enable`, no `Log.enable` (only `Target.setAutoAttach` is left) | nothing reads their events; fewer enabled domains |
| `src/handler/target.rs` | an auto-attached service worker is no longer detached right after `Runtime.runIfWaitingForDebugger` (it stays attached, no domain enabled on it, as Playwright keeps it) | the detach raced the resume and left the worker paused: a page's service worker never answered (creepjs fell back to a SharedWorker after 4 s). Upstream bug, also in stock 0.7.0; pinned by `service_worker_starts_promptly` |
| `src/handler/page.rs` | `execution_context_for_world` asks the browser instead of reading contexts learned from `Runtime.executionContextCreated`: isolated world = `Page.createIsolatedWorld(frame, "__chromiumoxide_utility_world__")` (Blink keeps one world per name and document, so it is reused, pinned by `eval_isolated_cannot_see_page_globals`); main world = the context id inside the object id of `globalThis` (main frame, `Runtime.evaluate` with no context) or of the frame's document (child frame: `DOM.getFrameOwner` -> `DOM.describeNode` -> `DOM.resolveNode`) | evaluation, locators and Motion keep working without `Runtime.enable` |
| `src/handler/page.rs` | `context_of` | parses `<isolate>.<context>.<object>`; every main-world eval in `creator::web` tests runs through it (the vendored crate's own tests are not run: it is not a workspace member) |
| `src/handler/mod.rs`, `src/cmd.rs` | `Page.navigate` is sent at once as a plain command and answered with Chrome's own answer (`loaderId` or `errorText`), as Playwright does; `creator::web::Page::goto` waits for that loader's `DOMContentLoaded`. `CommandMessage::is_navigation/split` and the handler's navigation ids are gone | stock 0.7 held a navigate behind the previous navigation's `load` and answered it from a watcher keeping the loader of when it was queued: a client redirect meanwhile answered it, and the held navigate then reloaded the page in the middle of a flow (live Breeze). Pinned by `goto_is_not_followed_by_a_late_reload` and `goto_waits_for_its_own_page_not_a_client_redirect` |

Left as upstream (not used by creator code, so not patched): `Page::expose_function`
(`Runtime.addBinding` + a main-world script), `Page::evaluate_on_new_document`,
`Page::enable_runtime/enable_log/enable_debugger/enable_css`, the frame
manager's handlers for Runtime events (they never fire now), and its navigation
queue and watchers (`FrameManager::goto`, `NavigationWatcher`: nothing feeds them
now; so `chromiumoxide::Page::goto` returns on Chrome's answer, not at `load`). Viewport emulation
(`Emulation.setDeviceMetricsOverride` + touch) runs only when `HandlerConfig.viewport`
is `Some`; ours is `None`, keep it so.

Not changed and expected by Playwright too: `Page.enable`, `Page.getFrameTree`,
`Page.setLifecycleEventsEnabled`, `Network.enable`, `Target.setAutoAttach`
(`waitForDebuggerOnStart` + `Runtime.runIfWaitingForDebugger`).

## Upstream's `sdks/rust`

`shardx` (`sdks/rust`) depends on chromiumoxide 0.7. Inside this build the patch
applies to it too (one `chromiumoxide` in `Cargo.lock`, the path one). Built on
its own (crates.io users of the SDK), it gets stock 0.7, so `ShardX::session`'s
browser sends `Runtime.enable` on every page and is detectable by the check above;
ShardX itself does not hide that (measured on ShardX 158: real profile, bare
engine). Nothing to do on our side; an upstream report could point at this file.

## Updating chromiumoxide

Vendor the new release unchanged in one commit, then re-apply the rows above in
the next and re-run `stealth_probe` (`cargo test --lib stealth_probe -- --ignored --nocapture`).
