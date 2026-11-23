import { invoke } from "@tauri-apps/api/core";

/** The browser mock build (`npm run dev:mock`), checked as upstream does. */
export const MOCK = import.meta.env.VITE_MOCK === "1";

/** Whether our backend answers the page-data commands (proxy meta, settings).
 *  Until M7 wires them only the mock does, so a real build leaves Shard's
 *  Proxies page and Settings exactly as Shard draws them. M7 replaces this
 *  with the backend's answer. */
export const CREATOR_DATA = MOCK;

/** One of our backend commands, through the single `creator_call` Tauri
 *  command; `npm run dev:mock` answers from mock.ts instead (never bundled
 *  into a release build: the import is behind the build-time flag). */
export const call = <T>(cmd: string, args?: Record<string, unknown>): Promise<T> =>
  MOCK
    ? import("./mock").then((m) => m.mockCall<T>(cmd, args))
    : invoke<T>("creator_call", { cmd, args });
