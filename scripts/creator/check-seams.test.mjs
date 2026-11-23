import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { checkSeams, gitInputs } from "./check-seams.mjs";

const config = JSON.parse(readFileSync(new URL("./seams.json", import.meta.url), "utf8"));
// The upstream ref this tree is based on: upstream/main, or the tag being merged (SEAM_BASE=v2.1.6).
const real = gitInputs(process.env.SEAM_BASE ?? "upstream/main");
const run = (over = {}) => checkSeams({ ...real, changed: [], config, ...over });
const swap = (path, fn) => (p) => (p === path ? fn(real.read(p)) : real.read(p));

test("passes_on_the_current_tree", () => {
  assert.deepEqual(run({ changed: real.changed }), []);
});

test("flags_an_unlisted_upstream_file", () => {
  const problems = run({ changed: ["src-tauri/src/api.rs"] });
  assert.equal(problems.length, 1);
  assert.match(problems[0], /src-tauri\/src\/api\.rs/);
});

test("flags_a_lost_marker", () => {
  assert.deepEqual(run({ read: swap("src/app/App.tsx", (t) => t.replace("creator fork seam", "")) }), ["src/app/App.tsx: 1 of 2 seam markers"]);
});

test("flags_mod_creator_moved", () => {
  const problems = run({ read: swap("src-tauri/src/lib.rs", (t) => t.replace(/^(.*\n)(mod creator;.*\n)/, "$1\n$2")) });
  assert.equal(problems.length, 1);
  assert.match(problems[0], /line 2/);
});

test("flags_chromiumoxide_drift", () => {
  const problems = run({ read: swap("src-tauri/Cargo.toml", (t) => t.replace(/(\[dependencies\.chromiumoxide\]\s*\nversion = ")[\d.]+/, "$10.9")) });
  assert.equal(problems.length, 1);
  assert.match(problems[0], /0\.9.*0\.7/);
});

test("flags_an_unmarked_line_in_a_seam_file", () => {
  const diff = (p) => (p === "src/app/App.tsx" ? real.diff(p) + "@@ -10,0 +11 @@\n+const sneaky = 1;\n" : real.diff(p));
  assert.deepEqual(run({ diff }), ["src/app/App.tsx: added line without the seam marker: const sneaky = 1;"]);
});

test("accepts_a_block_under_a_marker_comment", () => {
  const diff = (p) => (p === ".gitignore" ? "@@ -0,0 +1,3 @@\n+# creator fork seam\n+.worktrees/\n+\n" : real.diff(p));
  assert.deepEqual(run({ diff }), []);
});

test("flags_a_deleted_upstream_line", () => {
  const diff = (p) => (p === "src/app/App.tsx" ? real.diff(p) + "@@ -20 +19,0 @@\n-import { X } from \"./x\";\n" : real.diff(p));
  const problems = run({ diff });
  assert.equal(problems.length, 1);
  assert.match(problems[0], /1 upstream line\(s\) deleted, 0 marked/);
});

test("flags_more_deleted_lines_than_marked_replacements", () => {
  // 3 upstream lines out, 1 marked line in: two lines of upstream code vanish unmarked.
  const hunk = "@@ -17,3 +17 @@\n-    let a = 1;\n-    let b = 2;\n-    let c = 3;\n+    let a = 1; // creator fork seam\n";
  const diff = (p) => (p === "src-tauri/src/runtime.rs" ? real.diff(p) + hunk : real.diff(p));
  assert.deepEqual(run({ diff }), ["src-tauri/src/runtime.rs: 3 upstream line(s) deleted, 1 marked line(s) added: let a = 1;"]);
});
