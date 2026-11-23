# AGENTS.md

Ghidorah Creator: a local-only fork of the ShardBrowser launcher (Tauri: Rust + TS).

The working process and all project facts (gates, risky areas, remotes, never-commit list) are in `docs/creator/working-process.md` (§7 for this project). How to take an upstream release: `docs/creator/upstream-merge.md`.

Code rules:

- Repo files, docs, code comments and commit messages are English.
- Our code lives in `src-tauri/src/creator/` and `src/creator/`. Upstream files change only at seams marked `creator fork seam`, listed in `scripts/creator/seams.json`; `node scripts/creator/check-seams.mjs` must pass.
- Never modify the closed ShardX engine binary.
