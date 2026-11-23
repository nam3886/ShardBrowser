// Verifies the fork's seams after every upstream merge (docs/creator/upstream-merge.md):
// (a) every file changed against upstream is ours or a listed seam;
// (b) each seam file carries exactly its count of "creator fork seam" markers;
// (c) every line we add to a seam file carries the marker, or sits in a block
//     opened by a marker comment line (.gitignore's, Cargo.toml's trailing tables);
//     a hunk may not delete more upstream lines than it adds marked ones;
// (d) `mod creator;` stays on line 2 of src-tauri/src/lib.rs;
// (e) our chromiumoxide has the same major.minor as upstream's sdks/rust.
// A seam file listed with 0 markers (Cargo.lock, tauri.conf.json: no comments
// possible) may differ freely; tauri.conf.json's updater is pinned by a Rust test.
//
// Run: node scripts/creator/check-seams.mjs [--base <ref>]   (default upstream/main)
// It compares the working tree with <base> itself, so it also works mid-merge.
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

export const ROOT = new URL("../../", import.meta.url);
const MARKER = "creator fork seam";
const MOD_LINE = "mod creator; // creator fork seam";

/** `**` spans folders, `*` stays inside one. */
const globRe = (g) =>
  new RegExp(`^${g.replace(/[.+^${}()|[\]\\]/g, "\\$&").replace(/\*\*/g, "\0").replace(/\*/g, "[^/]*").replace(/\0/g, ".*")}$`);

/** "0.7" from a `chromiumoxide = { version = "0.7.x", ... }` line or a `[dependencies.chromiumoxide]` table. */
const oxideMinor = (toml) =>
  (toml.match(/^chromiumoxide\s*=\s*\{[^}\n]*version\s*=\s*"(\d+\.\d+)/m) ??
    toml.match(/^\[dependencies\.chromiumoxide\][^[]*?^version\s*=\s*"(\d+\.\d+)/m))?.[1] ?? null;

/** Problems in one seam file's `git diff -U0` text. */
function lineProblems(path, diff) {
  const problems = [];
  for (const hunk of diff.split(/^@@/m).slice(1)) {
    const lines = hunk.split("\n").slice(1);
    const added = lines.filter((l) => l.startsWith("+") && !l.startsWith("+++")).map((l) => l.slice(1));
    const removed = lines.filter((l) => l.startsWith("-") && !l.startsWith("---"));
    let block = false;
    let marked = 0;
    for (const line of added) {
      if (line.includes(MARKER)) { block = /^\s*(#|\/\/)/.test(line); marked++; continue; }
      if (!line.trim()) continue;
      if (block) { marked++; continue; }
      problems.push(`${path}: added line without the seam marker: ${line.trim()}`);
    }
    // Each upstream line we take out is replaced by a marked one, one for one.
    if (removed.length > marked) {
      problems.push(`${path}: ${removed.length} upstream line(s) deleted, ${marked} marked line(s) added: ${removed[0].slice(1).trim()}`);
    }
  }
  return problems;
}

/** Problems found; empty means the seams hold.
 *  changed: paths that differ from upstream; read(path) -> text; diff(path) -> `git diff -U0` text. */
export function checkSeams({ changed, read, diff, config }) {
  const problems = [];
  const ours = config.ours.map(globRe);
  for (const path of changed) {
    if (!(path in config.seams) && !ours.some((re) => re.test(path))) {
      problems.push(`${path}: upstream file changed outside the seams (add a seam to seams.json or revert)`);
    }
  }
  for (const [path, want] of Object.entries(config.seams)) {
    const got = read(path).split(MARKER).length - 1;
    if (got !== want) problems.push(`${path}: ${got} of ${want} seam markers`);
    if (want > 0) problems.push(...lineProblems(path, diff(path)));
  }
  if (read("src-tauri/src/lib.rs").split("\n")[1] !== MOD_LINE) {
    problems.push(`src-tauri/src/lib.rs: line 2 must be "${MOD_LINE}"`);
  }
  const mine = oxideMinor(read(config.chromiumoxide.ours));
  const theirs = oxideMinor(read(config.chromiumoxide.theirs));
  if (!mine || mine !== theirs) {
    problems.push(`chromiumoxide: ours ${mine} (${config.chromiumoxide.ours}) vs upstream ${theirs} (${config.chromiumoxide.theirs})`);
  }
  return problems;
}

export function gitInputs(base) {
  const cwd = fileURLToPath(ROOT);
  const git = (...args) => execFileSync("git", args, { cwd, encoding: "utf8", maxBuffer: 64 << 20 });
  const list = (out) => out.split("\n").filter(Boolean);
  return {
    changed: list(git("diff", "--name-only", base)),
    read: (p) => readFileSync(new URL(p, ROOT), "utf8"),
    diff: (p) => git("diff", "-U0", base, "--", p),
  };
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const at = process.argv.indexOf("--base");
  const base = at > 0 ? process.argv[at + 1] : "upstream/main";
  const config = JSON.parse(readFileSync(new URL("scripts/creator/seams.json", ROOT), "utf8"));
  const inputs = gitInputs(base);
  const problems = checkSeams({ ...inputs, config });
  for (const p of problems) console.log(p);
  console.log(problems.length ? `\n${problems.length} seam problem(s)` : `seams OK: ${inputs.changed.length} changed file(s) vs ${base}`);
  process.exit(problems.length ? 1 : 0);
}
