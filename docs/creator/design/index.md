# Creator mockups (MD design gate)

Round 4 (owner reviews 2026-11-22: our own Proxies page, Shard's untouched; every screen × state; "follow Shard as closely as possible"; "one app must not have multiple styles"), then the owner's simplification ("reuse Shard as it is, the simplest path, no mechanism nobody asked for": one settings draft, fully automatic sign-ups, Shard-style proxy failures, no team or Sync handling). Every element below is copied from an existing ShardBrowser screen or component, and every word follows Shard's English (checked by Shard's own copy rules, `scripts/lib/copy-rules.mjs`, in `src/creator/i18n.test.ts`). Anything with no Shard source is listed under "No Shard source" as a question; nothing there was invented silently.

Pages run on mock data (`src/creator/mock.ts`) in the browser mock build. All PNGs come from `e2e/creator-mockups.spec.ts` (Playwright, Chromium, 1280x800 unless noted):

```
E2E_PORT=<free port> npx playwright test e2e/creator-mockups.spec.ts --project=dev
```

Each state has `-light` and `-dark` unless noted. Words come from our dictionaries (`src/creator/i18n/{en,ru,zh}.json`) and follow Shard's language setting. Backend texts (errors, steps, mail) are data and stay as the backend sends them.

## State matrix

The inventory comes from the Python app (`ghidorah_creator/ui/js/*.js`, `ui/index.html`, `api.py`, `engine/errors.py`, `ui/js/mock.js`): every page, drawer, dialog, menu, toast, confirm and status value there has a row here. Each cell is a screenshot (`<name>-{light,dark}.png`) or "n/a" with the reason. Mock modes (localStorage `creator:mock`): `empty`, `error`, `setup` (first run), `engine` (Shard updating its engine), `slow` (4 s answers), `big` (huge counts); `creator:mock.proxies = N` adds N proxies.

### Accounts

| State | Screenshot | Shard source |
|---|---|---|
| List with every status (Ready, Making + step, Failed + reason, Signed out, Frozen, Pushed), a job running, a halted queue | `accounts` | All profiles + overview cards |
| First run: Ele mail not set up, no proxies (Problems to fix), no accounts | `accounts-setup` | `ProblemCard` ("Problems to fix") + `EmptyState` |
| First load (placeholder rows) | `accounts-loading` | `LoadingRows` held 150/400 ms |
| Next window loading near the end | n/a: Shard's own list draws nothing extra; the next 50 rows append (`useWindowed`) | Shard's lists (no pager) |
| Empty, first run | `accounts-setup` / `accounts-empty` | `EmptyState` + primary |
| Search finds nothing | `accounts-nomatch` | "No profiles match “{q}”" |
| Filters find nothing | `accounts-filtered` shows the chip; the empty state is the same `EmptyState` with Clear filters | `FilterChips` |
| Filters menu open with counts | `accounts-filters-open` | `FilterMenu` |
| Display menu | `accounts-display` | `ProxyDisplayMenu` |
| Load error + Retry; backend down | `accounts-error` (the mock answers "database is locked"; a backend that is down gives the same state with its error) | `LoadError` |
| Huge counts (512,340) | `accounts-big` | `CountRoll`, grouped numbers |
| Very long email | every list shot (`nguyen.thi.thanh.huong.marketing.department.team2026@…` cut, whole on hover) | `whenTruncated` tooltips |
| Bulk bar, 3 selected | `accounts-bulk` | `BulkBarFrame` |
| Bulk bar's More: Log in again, Change proxy, Export logins (CSV), cookies (JSON), API keys (TXT), Delete | `accounts-bulk-more` | `BulkBarFrame` More menu |
| Export result with what it skipped | `accounts-export-toast` | `toast.ok` |
| Change proxy (any working proxy, or one) | `accounts-change-proxy` | Assign proxies `DialogModal` |
| Row hover action per state (Retry, Log in again, Copy login) | `accounts-row-action` | `DataList` row actions |
| Unfreeze a pushed account asks first | `accounts-unfreeze-confirm` | `confirmModal` |
| Close all browsers asks first | `accounts-close-all-confirm` | `confirmModal` |
| Done action toast (Retry) | `accounts-toast` | `toast.ok` |
| Delete asks first | `accounts-delete-confirm` | `confirmModal` (danger) |
| Make dialog | `accounts-make` | `DialogModal` (New profile) |
| Make blocked: Ele, proxies (first reason in the footer, Make disabled) | `accounts-make-blocked` |
| Shard is updating or swapping the engine: the running job says it waits, Make says why it can't start | `accounts-engine-update` | `ProblemCard`, `DialogModal` `footerNote` |
| Failed Peek: the last screenshot, then large | `accounts-screenshot` | Shard's live-view frame (`LiveView`), the kit's `Modal` | `DialogModal` `footerNote` + `isDisabled` |
| Make blocked: YesCaptcha key missing for ElevenLabs by email | `accounts-make-captcha` | same |
| Make: not enough free emails | `accounts-make` with a count over the free count shows "Only {n} free emails on this route" (same footer) | same |
| Peek: failed (reason, log) | `accounts-peek-failed` | Proxy Peek's failed test |
| Peek: failed by the proxy (PROXY_REFUSED) | `accounts-peek-proxyfail` | same |
| Peek: failed, not retryable (ACCOUNT_EXISTS: no Retry) | `accounts-peek-exists` | same |
| Peek: ready, secrets to copy | `accounts-peek-linked` | `CopyField` |
| Peek: signed out (Log in again) | `accounts-peek-signedout` | same |
| Peek: frozen and pushed (Unfreeze in More) | `accounts-peek-frozen` | `PeekHeader` menu |
| Peek: running with its step | `accounts-peek-running` | `PropertyRow` "Now" |
| Peek's Profile row: the Shard profile (link to it in All profiles) and its ID to copy | `accounts-peek-linked`, `emails-peek` | Proxy Peek's "Profiles using it" link, `DetailRow` copy |
| Russian | `accounts-ru` | Shard's language store |
| Smallest window (1024x680) | `accounts-compact-light` | Shard's compact project |
| Job stopped / paused all | n/a: the job card goes away; "Pause all queues" is the header's More item, its toast as `accounts-toast` | `HeaderMenu` |

### Emails

| State | Screenshot | Shard source |
|---|---|---|
| List (Outlook and Ele, Active and Gone), Peek with mail and a code | `emails-peek` | Proxy Peek |
| Gone mailbox in Peek (why no mail can come) | `emails-peek-gone` | Proxy Peek's failed reason |
| Mail list empty | `emails-peek-gone` ("No mail yet") | `EmptyState` text |
| Mail loading | n/a: Peek body shows `LoadingRows` as `accounts-loading` | `LoadingRows` |
| Older mail loads as the Peek scrolls (50 at a time) | `emails-peek-older` | Shard's lists (scroll, no pager) |
| Free for a platform: "Make {n} {name} accounts" beside the chip, opens Make filled in | `emails-free-for` | `FilterChips` `after` slot |
| Test result toast (active, gone, failed) | `emails-test-toast` | Shard's bulk test summary |
| Delete asks first | `emails-delete-confirm` | `confirmModal` |
| Add emails dialog (Outlook combos) | `emails-add` | `DialogModal` + `Textarea` (Proxy import) |
| Add: lines it can't read reported | `emails-add-report` | the dialog's "Ready to add" line |
| Add: new Ele mailboxes | `emails-add` (Type: Temp mail (Ele), How many) | `SegmentControl` |
| Display menu | `emails-display` | `ProxyDisplayMenu` |
| Empty, error | as Accounts (`EmptyState`, `LoadError`; same components) | |

### Proxy pool (ours)

| State | Screenshot | Shard source |
|---|---|---|
| List: working with exit IP, failed, testing, not tested, rotating, slow (2400 ms), tested days ago, off; a failing proxy's "Problems to fix" card with Retest | `proxies` | Shard's Proxies list; the overview's failed-proxy `ProblemCard` |
| Test in the bar, summary toast | `proxies-test-toast` | `useProxy.bulkTest` |
| Edit proxy | `proxies-edit` | `EditProxyDialog` |
| First load, load error, search finds nothing | `proxies-loading`, `proxies-error`, `proxies-nomatch` | `LoadingRows`, `LoadError`, `EmptyState` |
| Filters (Status, Type, IP type, Resting) | `proxies-filters` | `FilterMenu` |
| Display | `proxies-display` | `ProxyDisplayMenu` |
| Peek: connection, IP history, sign-ups, accounts using it | `proxies-peek` | `ProxyPeek` |
| Peek: failing, resting | `proxies-peek-resting` | `ProxyPeek` |
| 1,010 selected, bulk bar | `proxies-bulk` | `ProxyBulkBar` |
| Add proxies: paste | `proxies-add` | `ProxyBulkImporter` |
| Add proxies: preview after Test all (a bad line skipped) | `proxies-add-preview` | same |
| Add proxies: nothing to read | `proxies-add-error` | Shard's toast |
| Delete asks first | `proxies-delete-confirm` | `confirmModal` |
| None yet | `proxies-empty` | Proxies `EmptyState` |

### Platforms, Settings, app

| State | Screenshot | Shard source |
|---|---|---|
| Platform cards: ready, pinned, Coming later (Fish Audio, Noiz) | `platforms` | Extensions grid |
| Platform Edit (rule, the platform's own tries and waits) | `platforms-edit` (ChatGPT: sign-in tries, age check tries, chat reply wait) | Edit proxy dialog |
| Platform paused (fail streak) | `accounts` (the halted queue card; Resume) | `ProblemCard` |
| Settings tab | `settings` | Settings tabs |
| Unsaved, Save bar | `settings-unsaved` | `SaveBar` |
| Leaving with changes | `settings-leave` | `useUnsaved` |
| Load failed + Retry | `settings-error` | `LoadError` |
| Test Ele: answered | `settings-ele-ok` | `toast.ok` |
| Test Ele: failed (first run) | `settings-ele-fail` | `toast.fail` |
| Save refused by the backend (validation) | `settings-save-error` | `SaveBar` error |
| Relay (Test relay answered), the sign-up rows | `settings-relay-signup` | `SettingRow`, `Switch` |
| Blocked domains, adding one already there | `settings-blocked` | kit `Tag` |
| Command palette: "Settings: Creator" and our pages under Go to | `palette` | Shard's palette |
| Python's status bar (data size, disk, RAM, CPU, open browsers) | n/a: dropped by the owner | |
| Python's health panel (Ele mail, ShardBrowser, proxies) | `accounts-setup` (the same facts as Problems to fix cards, only when something is missing) | `ProblemCard` |
| Keyboard shortcuts | n/a: the Python app has none; our lists inherit Shard's list keys (arrows, X, Enter) through `DataList` | |
| Events / log | in every account Peek (`accounts-peek-failed`, `accounts-peek-running`) | Proxy Peek's IP history list |

**Our accounts' browsers are Shard profiles (D17, revised).** Each is an ordinary Shard profile in a Shard folder named "Creator", made the first time its mailbox is used, created, started, stopped and tracked by Shard's own code (no seam). Its proxy is Shard's inline proxy on that profile, taken from our Proxy pool, so Shard's Proxies list never gets entries from us. Account and email Peeks show the profile as Shard links one (Proxy Peek's "Profiles using it": a ghost button that shows it in All profiles) with Shard's copy button for its ID. A profile deleted in Shard is reported as "ShardBrowser profile is gone" and made again on the next run. (The mock's Shard demo data has no "Creator" folder: the link only shows where it would lead.)

**Engine and launcher states.** Shard owns them, so our pages show none of their screens: the first download (Shard's `FirstRunGate`, whole window), an update or swap (Shard's `EngineUpdateBanner` and its launch guards), a browser that crashed or was closed (Shard's running set). What our pages still show: while the engine updates or swaps, a running job's card says "Waiting for the browser engine update; it goes on by itself" and Make says why it can't start (`accounts-engine-update`); a browser Shard closed ends its run as INTERRUPTED (retryable); a profile deleted in Shard ends it as "ShardBrowser profile is gone". Python's token and port states don't exist (Shard's own engine).

## Logos, copy buttons, bulk tests

**Service logos** (owner: real logos, one size per context). Vendored SVGs in `src/creator/logos/` (sources in its `SOURCES.md`, the MIT licence in `LICENSE-lobe-icons`) from [Lobe Icons](https://github.com/lobehub/lobe-icons) (`@lobehub/icons-static-svg` 1.70.0, MIT): Google (`google-color`), Qwen (`qwen-color`), ChatGPT (`openai`), Grok (`grok`), ElevenLabs (`elevenlabs`), Fish Audio (`fishaudio`), Outlook (`microsoft-color`, the Microsoft mark: Lobe has no Outlook). The marks are their owners' trademarks; MIT covers the drawings only. Sizes: 14 px wherever a service sits beside text (rows, menus, filter values, chips, Peek rows: Shard's `size-3.5` icon slot); 35 px tile with a 20 px mark on Platforms cards (ExtensionCard's icon tile). Mono marks draw in the text color, so they hold in both themes. Breeze Blue: breezeblue.ai's own favicon (`/favicon.png`, a black mark) redrawn as SVG in the text color. Noiz: noiz.ai's own `favicon.svg`, as is. These two are taken from the services' sites (their trademarks; no licence stated). Ele (the temp-mail service) has no public site to take a mark from and keeps an initial at the same sizes.

**Copy buttons.** Every copyable value in our Peeks has Shard's copy button (its `DetailRow` recipe: the kit's quiet `CopyButton`, "Copy {label}", "Copied", or the secret toast that clears the clipboard in a minute): account ID, login, password, API key, proxy, profile ID (shown as text, no link), email, mail code, proxy address, exit IP. Passwords and keys are read only when Copy is pressed. Cookies are copied from the account's More menu ("Copy cookies and proxy").

**Bulk tests.** Shard's mechanism exactly (`useProxy.bulkTest`, copied as `src/creator/pool.ts` with a test): a "Testing…" toast, a continuous pool of 5 workers (each takes the next item as soon as it is free, never fixed chunks), one summary toast. Proxy pool: Shard's words. Emails' Test (Shard's verb), the same pool of 5: "Tested {n}: {active} active, {gone} gone, {failed} failed".

## Sidebar and app shell

| Element | Copied from |
|---|---|
| Our pages in one folding group "Creator" (heading, chevron, items, folded state kept per viewer) | Sidebar `Group` (Pinned/Folders/Views): same markup and `FoldHeading`; rail mode: its hairline and icons |
| Each item with its count | Sidebar `FolderItem`: `NavItem` badge with `CountRoll` (drawn by Shard's own `PageItem`) |
| Brand row and version row stay Shard's: "ShardBrowser", its logo and its version row, untouched (owner; the mock shows Shard's demo version) | Shard's sidebar, unchanged (no seam) |
| "API on :40325" | Sidebar `ApiStatus`, Shard's own port (we keep Shard's identity) |
| Our pages in ⌘K Search under "Go to" (found by name and by "creator") | Shard's `nav.*` palette commands (one seam in `appCommands.ts`) |
| Page header: title, count, help (i), search, Filters, Display, More, primary | `PageHeader` with the All profiles / Proxies toolbar order (`ProfileToolbar`, `ProxyToolbar`) |
| Display (order, columns, Reset to default) | `ProxyDisplayMenu` (same popover; our columns are the ones the list header saves) |

## Accounts

| Element | Copied from |
|---|---|
| "Running now" and "Problems to fix" folding sections over the list | Profiles overview: "Continue" / "Problems to fix" (`FoldHeading` + card grid) |
| Job card (title + pill, sentence, step, usage bar "3 of 5 done", Stop) | `ProblemCard` with its usage bar (the ShardX plan's traffic card) |
| Halted queue card (title + red count pill, sentence, why, detail, Resume) | `ProblemCard` (Location leak / failed proxy cards) |
| Filters button and menu (Service, Status, Flags, each value with its count) | `FilterMenu` as `ProfileFilterBar` / `ProxyFilterBar` use it |
| Applied filter chips + Clear filters | `FilterChips` as `ProxyFilterChips` |
| List, columns sized to content, column menu, row hover action, row menu | Kit `DataList` + `useListLayout` (Trash page recipe) |
| One scrolling list, next rows load near its end | Shard's lists (virtual scroll, no pages); windows of 50 are ours behind it |
| Status cell: pill + step or reason beside it | Badge pill + `StatusDot` (`StatusPill`); the text beside it as a Proxies row puts the IP beside its test pill |
| Flags cell (Frozen, Pushed) | Trash list's small gray `Badge` |
| Proxy cell (flag + address) | Proxies list address and country cells (`CountryFlag`, mono) |
| Bulk bar (count, actions, More, X) | `BulkBarFrame` (Trash / Proxies bulk bars) |
| Empty, no-match and load-error states | `EmptyState`, `LoadError` as Proxies/Trash |
| Make accounts dialog | `DialogModal` (New profile); `SelectField`, `NumField`; route choices as Sync's Disconnect dialog (Radio + help line); proxy mode `SegmentControl` (Assign proxies dialog) |

## Account Peek

| Element | Copied from |
|---|---|
| Panel, header (name, status pill, Open browser, Retry/Check, More, Close) | `PeekPanel` / `PeekHeader` as Proxy Peek (Edit + Test now) |
| "Why it failed" (error words in the error color, the backend's text under it) | Proxy Peek's failed test reason (`geo_error` block) |
| Log (message, then time and step; newest first) | Proxy Peek's IP history list |
| Details rows | `PropertyRow` / `PropertyValue` (Proxy Peek Details) |
| Login, password, API key to copy | `CopyField` (secret: clipboard clears after a minute) |

## Emails

| Element | Copied from |
|---|---|
| Header, Filters (Type, Status, Free for a platform), chips, list, bulk bar, states | As Accounts above |
| Accounts column as text ("Google, Qwen") | A text cell (Trash/Profiles text columns) |
| Status cell (Active / Gone) | Trash's small `Badge` |
| Email Peek: details rows | Proxy Peek Details |
| Accounts on this email (name button + status pill) | Proxy Peek "Profiles using it" list + `StatusPill` |
| Mail list (subject, then time and sender) | Proxy Peek's IP history list |
| Code to copy | `CopyField` |
| Add emails dialog (type picker, text area, "Ready to add" line) | `DialogModal` + `SegmentControl` + `Textarea` (Proxy import) |

## Platforms

| Element | Copied from |
|---|---|
| Card grid, card (mark, name, subline, two-line description), actions in the card's menu | Extensions page (`DataList variant="grid"`, `ExtensionCard`); the mark in its 35 px icon tile |
| Pinned platform: pin icon after the name; Pin / Unpin in the menu | Profiles row's pin icon; profile menu's Pin / Unpin (`rowActionIcons`) |
| Category filter + chip | `FilterMenu` / `FilterChips` |
| Edit… dialog (one account per email switch, the platform's own tries and waits: Qwen captcha tries; ChatGPT sign-in tries, age check tries, chat reply wait; Grok age check tries, chat reply wait; Fish Audio submit tries) saved by hand | Edit proxy dialog (`DialogModal`), `Switch`, `NumField` |

## Proxy pool (ours, D15)

Owner, round 4: Shard's own Proxies page and data stay exactly as upstream. Our sign-up proxies have their own page in the Creator group, named "Proxy pool" (owner), on our own store. It is a copy of Shard's page; Shard's data-agnostic parts are used as they are, the rest is copied into `src/creator/`.

| Element | Copied from / how |
|---|---|
| Header: title, count, (i), Search, Filters, Display, More, "Add proxies" | `ProxyToolbar` order on `PageHeader` |
| Columns Name, Type, Host and port, Country (flag + city), Accounts, Test result ("OK 530 ms" + exit IP), IP type; Sign-ups and Resting in Display | `ProxyListRow` cells: `ProtocolPill`, `CountryFlag`, `ProxyTestPill` as is |
| Filters Status, Type, IP type, Resting with counts | `ProxyFilterBar` fields, Shard's words |
| Bulk bar "1,010 selected · Test · Turn off · Copy as text · Delete ×" (select all = every match) | `ProxyBulkBar`; Delete in the error variant as `CommandButton` |
| Test (bar, Test all, Peek's Test now): "Testing {n} proxies…", 5 at a time in a continuous pool, then "Tested {n}: {ok} working, {failed} failed" | Shard's `useProxy.bulkTest`, copied as `src/creator/pool.ts` (tested), Shard's own words |
| "On" switch per proxy; Status filter "Off"; Rotation filter (Static, Rotating) | Python's per-proxy On and kind tabs; the kit `Switch` as Extensions cards use it |
| A failing proxy (turned on, last test failed): one "Problems to fix" card, Shard's words ("{proxy} failed its test: {why}", Retest proxy / Retest proxies, "N more", Show = the Failed filter, dismiss). Nothing moves accounts off it: their sign-ups fail with its proxy error (retryable) and Accounts' Change proxy gives them another; new accounts only ever get a healthy proxy | The overview's failed-proxy `ProblemCard` (`useProblemCards` `failedToday`) |
| Edit proxy | Shard's `EditProxyDialog` copied (no change-IP link: Shard's own rotation feature) |
| Add proxies (paste, Preview, Test all, Select working only, Add) | `ProxyBulkImporter` copied; parsing a line uses Shard's pure `proxy_bulk_parse`; testing a line is our `proxy_test_entry` (Shard's `proxy_full_test` would write orphan entries into Shard's test history); Add saves into our store |
| Peek: header (test pill, Edit, Test now, More), Details, Connection with IP history, Sign-ups (ours), Accounts using it | `ProxyPeek` copied; `Details` used as is (Country: the given one, else the last test's). `Details`' ProxyShard signature picker calls ProxyShard only when the notes name a ProxyShard order; our backend never stores that text in our notes, so it never shows |

**IP type is never chosen by hand.** After each test our backend asks ip-api.com through the proxy for `mobile`, `hosting` and `proxy`: mobile gives Mobile, hosting gives Datacenter, otherwise Residential. With no answer, the test's ISP name decides (a hosting-company list); never tested: "—". Peek says which ("Found from"). "Rotating" means the exit IP changed between tests (the proxy's IP history).

**Trade-off (owner chose the copy):**

| | Our own page (chosen) | Round 3: columns on Shard's page |
|---|---|---|
| Shard's page | Untouched: zero seams in its files; it never shows our proxies | 14 markers in 5 of Shard's busiest files |
| Looks | The same look, a second page; "Assign to profiles" has no meaning here and is left out | Identical page |
| Upstream merges | Shard's improvements to Proxies must be copied by hand: each release compare `widgets/ProxyList`, `widgets/ProxyPeek`, `features/manage-proxies/ui/ProxyBulkImporter.tsx` with ours | Re-check 5 files each release |
| Data | Ours owns every proxy; a profile gets it as Shard's inline proxy (D17), never as an entry in Shard's Proxies | Shard's store plus our side table |

## Settings (Creator tab)


| Element | Copied from |
|---|---|
| Groups and rows (title, help, control at the right) | `SettingsGroup` / `SettingRow` |
| Groups, shared settings only: Mail (Ele address, key, Test Ele), Relay (address, admin key, Test relay), Captcha (YesCaptcha), Runs (browsers at once, stop a queue after N failures, wait for a code, delete the browser of a failed sign-up), Proxies (rest time), Blocked domains. A platform's own options are in its Edit dialog (Platforms) | `SettingsGroup` / `SettingRow`, `Switch`, `NumField`, laid out as the Sync and API & MCP tabs (titled groups, a Test button at a row's end) |
| Blocked domains: one removable tag each, a field and Add (Enter adds), "{d} is already blocked" | The kit's `Tag` with `onRemove` |
| Key fields (password, key icon) | ShardX API key form (`PsApiKeyForm`) |
| "Saved" / "Not set" after a key's title | Settings Sync row (a title with a mark) |
| Numbers, choices | `NumField`, `SelectField`, `SegmentControl` (Settings General/API) |
| Own tab "Creator" in the tab list, its heading, the palette's "Settings: Creator" command | Shard's Settings tabs (`SECTIONS`, `TAB_LABEL`, the tab list, `SETTINGS_TABS` in the command registry) |
| Save bar (Unsaved changes, Don't save, Save), pinned at the page bottom | Shard's one `SaveBar`: our draft joins Shard's (`useCreatorDraft(useSettingsDraft())`, one seam line), so Shard's own bar shows, saves and discards both. Ours is written first (10 s at most, edits typed meanwhile kept, Shard's failure words), so "Settings saved" only shows when all of it is saved |
| Leaving with unsaved changes (Shard's one "Save changes to Settings?"), Cmd/Ctrl+S | Shard's own `useUnsaved` question and Cmd/Ctrl+S, unchanged: they see one draft; one "Settings saved" |
| Load failed: "Couldn’t load settings" with Retry | `LoadError` (Shard's lists) |
| Tab shown only once our backend answers (mock build until M7); the palette's "Settings: Creator" likewise | Shard's own rule for Sync (no tab, no command on a backend without sync) |


**Shard's own Settings with Creator untouched.** Shard's Settings e2e specs (`settings.spec`, `settings-save.spec`, `settings-secret.spec`, `settings-rows.spec`, `unsaved-guard.spec`, `borderless-settings-overview.spec`) on `main` and on this branch: the same results except one, `settings.spec` "Settings has twelve sections", which counts the tabs (ours is the thirteenth, the tab the owner asked for). `settings-rows.spec` fails its two "one line of help" cases on `main` too.


## No Shard source (questions for the owner)

Decided (owner, 2026-11-22): real logos (Breeze Blue and Noiz from their sites); Shard's name and logo stay; no system stats bar; no full mail reader; the page is "Proxy pool"; our pages in ⌘K; profiles are Shard profiles in a "Creator" folder (D17); no mover and no Undo for failing proxies (simplification: Shard's Problems to fix card, Change proxy by hand); Slow and not-tested-lately the Shard way (the number and a tooltip); the failed sign-up's last screenshot in its Peek; older mail by scroll; Shard's sidebar running count is enough; Make N from "Free for" stays. Simplification: Shard's own identity and version (only updates come from our repo); fully automatic sign-ups (no "Needs you", no "Wait for me", no Google terms or Qwen puzzle switches); no team-space or Sync handling (our profiles are ordinary Shard profiles); Emails' Test 5 at a time.

1. **Ele's mark.** Ele has no public site to take a logo from; it keeps an initial. A file, if there is one?
2. **Job progress color.** `ProblemCard`'s usage bar is drawn in the warning color (made for traffic running low), so a job's progress shows in orange.

## Error codes and their words

From `engine/errors.py` (code, group, retryable). The backend sends the code only; the words live in our dictionaries as `creator.error.<CODE>` (en, ru, zh; this table is the English), in Shard's wording (sentence case, contractions, no "the app"). Group is the account Filters' failure kind. "Retry" shows only when retryable.

Sign-ups are fully automatic: no step waits for the person. Python's codes that only existed for a hand wait are gone: MANUAL_TIMEOUT (a captcha or Cloudflare check nobody solved in time; automation that can't pass one now fails as CAPTCHA_UNSOLVED or CLOUDFLARE_LOOP) and CLOSED_AT_TERMS (the window closed while Google's terms waited for a hand; terms are always accepted in the page, a closed window is WINDOW_CLOSED). Every failure keeps its code and Retry when retryable.

| Code | Group | Retry | Shown |
|---|---|---|---|
| INTERRUPTED | Stopped | yes | The run stopped: the window closed, all browsers were closed, or the app quit. |
| QUIT_AFTER_OTP | Stopped | yes | The app quit after Google took the code. The Google account may exist. |
| WINDOW_CLOSED | Stopped | yes | The sign-up tab or pop-up closed during a step. |
| PROXY_UNUSABLE | Proxy | yes | The browser’s proxy is off, failing, resting or deleted. Change its proxy. |
| NO_PROXY | Proxy | yes | No working proxy was free for a new browser. |
| PROXY_REFUSED | Proxy | yes | The proxy refused the connection or asked for a sign-in. |
| PAGE_LOAD_FAILED | Proxy | yes | The site’s page didn’t load (a network error through the proxy). |
| GEO_BLOCKED | Proxy | yes | The site blocks the proxy’s country or IP. |
| PROXY_CHECK_FAILED | Proxy | yes | The browser couldn’t use the proxy (refused, or its location lookup failed). |
| CAPTCHA_RATE_LIMITED | Captcha | yes | The captcha service limits this IP. Try another proxy later. |
| CAPTCHA_UNSOLVED | Captcha | yes | The captcha wasn’t solved. |
| CLOUDFLARE_LOOP | Captcha | yes | Cloudflare’s check kept coming back. |
| CAPTCHA_EXTENSION | Captcha | yes | The YesCaptcha extension couldn’t be prepared. |
| NO_CAPTCHA_KEY | Captcha | yes | Add the YesCaptcha key in Settings. |
| MAILBOX_GONE | Mail | no | The mail provider deleted this inbox. Make a new account. |
| MAIL_API_DOWN | Mail | yes | The mail service (Ele or Outlook) didn’t answer. |
| NO_MAIL | Mail | yes | The code or verification mail never arrived. |
| CODE_NOT_SENT | Mail | yes | The site wouldn’t send the code (it usually works on a later try). |
| CODE_REJECTED | Mail | yes | The site didn’t accept the code. |
| VERIFY_LINK_FAILED | Mail | yes | The verification link didn’t finish the sign-up. |
| DOMAIN_BLOCKED | Mail | no | This email’s domain is blocked. |
| EMAIL_REFUSED | Mail | no | This site doesn’t accept this email address. Other services may. |
| GOOGLE_REFUSED | Refused | yes | Google refused the sign-up (usually the proxy IP). |
| SIGNIN_REFUSED | Refused | yes | The site refused the sign-in. |
| AGE_STEP_FAILED | Refused | yes | The age step didn’t go through. |
| SIGNUP_NOT_COMPLETED | Refused | yes | The sign-up form was sent, but the site didn’t finish it. |
| NOT_SIGNED_IN | Refused | yes | The account didn’t end up signed in. |
| API_KEY_FAILED | Refused | yes | The API key wasn’t made or didn’t pass its test. |
| NO_CHAT_REPLY | Refused | yes | The chat check got no reply. |
| ACCOUNT_EXISTS | Account | no | An account already exists for this email (not ours). |
| ACCOUNT_BANNED | Account | no | The site banned or deactivated this account. |
| EMAIL_NOT_VERIFIED | Account | yes | The site says this email isn’t verified. |
| GOOGLE_SIGNED_OUT | Account | yes | Google is signed out in this browser. Log in again from its email first. |
| PARENT_FAILED | Account | yes | The Google account this one signs in with failed. |
| SESSION_NOT_KEPT | Account | yes | Signed in, but not after the browser reopened. |
| CONSOLE_CHECK | Account | yes | Google signed up, but its console check didn’t say active. |
| BROWSER_HUNG | Other | yes | The browser stopped responding. |
| PAGE_CHANGED | Other | yes | A button or field didn’t show or respond. The site may have changed. |
| STEP_FAILED | Other | yes | A page step failed (often a timeout). |
| APP_ERROR | Other | yes | Something went wrong inside the app. Send the log. |
| PROFILE_RUNNING | Other | yes | The browser was still running. Retry once it’s closed. |
| NO_BROWSER | Other | yes | The email has no browser folder or address. |
| ENGINE_UNAVAILABLE | Other | yes | The browser engine wasn’t ready (still downloading or updating). |
| SIGNUP_FAILED | Other | yes | The sign-up failed without a named reason. |
| OTHER | Other | yes | Not in the error list yet. The backend’s text is shown. |

Codes that never count toward stopping a queue (`streak=false`): INTERRUPTED, QUIT_AFTER_OTP, PROXY_UNUSABLE, NO_PROXY, NO_CAPTCHA_KEY, MAILBOX_GONE, DOMAIN_BLOCKED, EMAIL_REFUSED, ACCOUNT_EXISTS, PARENT_FAILED, PROFILE_RUNNING, NO_BROWSER, ENGINE_UNAVAILABLE.

## Words

Every string in `src/creator/i18n/en.json` was rewritten against Shard's `en.json`: sentence case, Shard's terms (Peek, Pin, Test, Display, Filters, "Problems to fix", "Close Peek", "More actions"), its empty states ("No accounts yet", "No accounts match “{q}”", "No accounts match these filters"), commands with no trailing "…", curly quotes, and counts as Shard writes them: grouped in the UI language ("1,010 selected"), a separate "One" key in English ("Make 1 account"), Russian forms as `{n|аккаунт|аккаунта|аккаунтов}`. The test runs Shard's copy rules over all three of our dictionaries; only our product and service names are allowed as capitals inside a sentence.

## Seams

26 markers in 12 upstream files, plus two unmarked JSON/lock seams (`src-tauri/Cargo.lock`, and `src-tauri/tauri.conf.json` for our updater endpoint and key, pinned by `creator::tests::updates_come_from_our_repo`), all listed in `scripts/creator/seams.json` and checked by `scripts/creator/check-seams.mjs`. Before the simplification: 32 markers in 15 files.
- Identity: 0 (Shard's own; the M0 seams in `store.rs`, `runtime.rs`'s data folder, `reset.rs` and `settings.rs` are gone). `runtime.rs` keeps 1: the launcher's release repo. Shard's About, star and update-fallback links (`GH_REPO_URL`) stay upstream's: the same constant is Create team's team-server download link, which our releases don't carry.
- Shard's Proxies files: 0.
- Creator Settings tab: `pages/settings/index.tsx` 7 (import, section, label, `?tab=creator` opening General before our backend as Sync, the one draft line, the tab, its heading), `shared/model/navigation.ts` 2, `features/commands/appCommands.ts` 4 (the label, no "Settings: Creator" command until our backend answers, our pages under Go to). Sidebar: 2 (the Creator group).
