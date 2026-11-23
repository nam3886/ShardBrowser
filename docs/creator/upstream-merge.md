# Taking an upstream release

Upstream = ProxyShard/ShardBrowser (remote `upstream`, fetch only). Upstream squashes each release into one or two large commits; we merge release tags, never cherry-pick. Our code is in `src-tauri/src/creator/` and `src/creator/`; upstream files change only at the seams listed in `scripts/creator/seams.json`.

## Steps

1. Fetch: `git fetch upstream --tags`.
2. Worktree and branch from `main`: `git worktree add .worktrees/merge-upstream-vX.Y.Z -b merge/upstream-vX.Y.Z main`.
3. Merge the tag: `git merge --no-ff vX.Y.Z`.
4. Conflicts:
   - `src-tauri/Cargo.lock`: never hand-merge. `git checkout --theirs src-tauri/Cargo.lock`, then `cargo metadata --manifest-path src-tauri/Cargo.toml > /dev/null` to regenerate it with our dependencies, and `git add` it.
   - A seam line: keep upstream's new code and put our seam line back with its `creator fork seam` marker. `mod creator;` stays on line 2 of `src-tauri/src/lib.rs`.
   - Our Proxies page is a copy of Shard's (D15): when upstream changed `src/widgets/ProxyList/`, `src/widgets/ProxyPeek/` or `src/features/manage-proxies/ui/ProxyBulkImporter.tsx`, carry the change into `src/creator/pages/ProxiesPage.tsx`, `src/creator/widgets/ProxyPeek.tsx` and `src/creator/widgets/AddProxiesDialog.tsx`.
5. Seam check: `node scripts/creator/check-seams.mjs --base vX.Y.Z` (the tag being merged; the default is `upstream/main`). It compares the working tree with that ref, so it runs mid-merge too, and fails on: an upstream file changed outside the seams; a lost or extra marker; a line added to a seam file without the marker (or outside a block opened by a marker comment line, such as `.gitignore`'s and `Cargo.toml`'s trailing tables); upstream lines deleted without a marked replacement; `mod creator;` off line 2; chromiumoxide drifting from `sdks/rust`. If upstream moved `sdks/rust` to a new chromiumoxide minor, move ours in the trailing `# creator fork seam` tables of `src-tauri/Cargo.toml` and fix `creator/` for the new API in the same branch.
   - `src-tauri/tauri.conf.json` (JSON, no marker): keep upstream's new version and everything else, but our updater `pubkey` and `endpoints` (`creator::tests::updates_come_from_our_repo` fails otherwise).
   - **Expected failures in Shard's own e2e** (ours on purpose; any other new failure is a bug):
     - `e2e/settings.spec.ts` "Settings has twelve sections": our Creator tab is the thirteenth.
   - If Shard's settings fail to load, the Settings page shows its own load state for every tab, ours included (our tab draws only once Shard's draft is read).
6. All gates of `docs/creator/working-process.md` §7 (`npm run check`, `SEAM_BASE=vX.Y.Z node --test scripts/creator/*.test.mjs`, the full `cargo test --manifest-path src-tauri/Cargo.toml`, `cargo check --locked`, rustfmt on `creator/`, Shard's e2e lane).
7. One dev run (`npm run tauri dev`) with a sandboxed `HOME` (`HOME=$PWD/.spike-home`: our build uses Shard's own data folder): start a profile, open our page, press "Open example.com" (profile starts with CDP, title read back).
8. Engine gate: `runtime.json` `min_launcher_version` must be `<=` the `version` in `src-tauri/Cargo.toml` (both upstream's: we keep Shard's version).
   - Version: the merged `tauri.conf.json` `version` must be higher than our last release; if upstream's new version is not (we bumped the patch for a Creator-only release, e.g. 2.1.7 over upstream 2.1.6, and upstream ships 2.1.7), bump its patch past ours. **Versioning:** we keep Shard's version line, and the updater only installs a release whose version is higher than the installed one. A Creator-only release between upstream releases bumps the patch past upstream's current version (upstream 2.1.6 -> ours 2.1.7, then 2.1.8, ...), set in `src-tauri/tauri.conf.json` `version` (and `Cargo.toml`/`package.json` if the release script reads them); the next upstream merge must then end higher than our last release (if upstream's new version is not higher, bump its patch past ours in the merge).
9. Skim the release diff of the upstream code we call and note what changed in the merge commit body:
   - `src-tauri/src/launch.rs`, `src-tauri/src/process.rs` (start, stop, CDP endpoint);
   - `src-tauri/src/space/local/profile.rs`, `src-tauri/src/space/local/proxy.rs`;
   - `sdks/rust/src/motion.rs` (the hidden `Motion` domain we drive input through);
   - anything new that reaches `ProxyShard/ShardBrowser` for launcher updates (it would need a seam to point at our repo).
10. Review as usual, then merge into `main` with `--no-ff` only after the owner says "merge". Push to `origin` only on the owner's word.
11. Remove the worktree and branch.
