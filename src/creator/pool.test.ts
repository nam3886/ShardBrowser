import { test } from "node:test";
import assert from "node:assert/strict";
import { runPool } from "./pool.ts";

test("never more than `size` at once, every item once, results in item order", async () => {
  let running = 0;
  let peak = 0;
  const out = await runPool([5, 1, 4, 2, 3, 1, 1], 3, async (ms) => {
    running++;
    peak = Math.max(peak, running);
    await new Promise((r) => setTimeout(r, ms));
    running--;
    return ms * 10;
  });
  assert.equal(peak, 3);
  assert.deepEqual(out, [50, 10, 40, 20, 30, 10, 10]);
});

test("a worker takes the next item as soon as it is free (no fixed chunks)", async () => {
  const started: number[] = [];
  // Item 0 is slow; with chunks of 2, item 2 would wait for it.
  await runPool([30, 1, 1, 1], 2, async (ms) => { started.push(Date.now()); await new Promise((r) => setTimeout(r, ms)); });
  assert.ok(started[3] - started[0] < 20, "items 2 and 3 started while item 0 still ran");
});
