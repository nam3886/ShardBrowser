import { invoke } from "@tauri-apps/api/core";

/** Box dev run only: `dev:mock` with our commands answered by a real backend
 *  over HTTP (`runner::live::dev_bridge`); our pages then behave as in the
 *  real build. Dev server only: a build never reads it. */
const BRIDGE: string | undefined = import.meta.env.DEV ? import.meta.env.VITE_CREATOR_BRIDGE : undefined;

/** The browser mock build (`npm run dev:mock`), checked as upstream does. */
export const MOCK = import.meta.env.VITE_MOCK === "1" && !BRIDGE;

const bridged = <T>(cmd: string, args?: Record<string, unknown>): Promise<T> =>
  fetch(`${BRIDGE}/call`, { method: "POST", body: JSON.stringify({ cmd, args: args ?? null }) })
    .then((r) => r.json())
    .then((r: { ok?: T; err?: string }) => ("err" in r ? Promise.reject(r.err) : (r.ok as T)));

/** Whether our backend answers our Settings tab (Shard draws the tab and its
 *  palette command only then). Ours always does since M2 (the Breeze Blue
 *  settings); the mock answers the rest. */
export const CREATOR_DATA: boolean = true;

/** One of our backend commands, through the single `creator_call` Tauri
 *  command; `npm run dev:mock` answers from mock.ts instead (never bundled
 *  into a release build: the import is behind the build-time flag). */
export const call = <T>(cmd: string, args?: Record<string, unknown>): Promise<T> =>
  BRIDGE
    ? bridged<T>(cmd, args)
    : MOCK
      ? import("./mock").then((m) => m.mockCall<T>(cmd, args))
      : invoke<T>("creator_call", { cmd, args });
