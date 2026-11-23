// Shard's bulk test mechanism (useProxy.bulkTest), for our lists: a
// continuous pool of `size` workers, each taking the next item as soon as
// its last one is done (never fixed chunks), and one summary at the end.

/** Runs `one` over `items`, at most `size` at a time, in order of start. */
export async function runPool<T, R>(items: readonly T[], size: number, one: (item: T) => Promise<R>): Promise<R[]> {
  const out: R[] = new Array(items.length);
  let i = 0;
  await Promise.all(
    Array.from({ length: Math.min(Math.max(1, size), items.length) }, async () => {
      while (i < items.length) {
        const k = i++;
        out[k] = await one(items[k]);
      }
    }),
  );
  return out;
}
