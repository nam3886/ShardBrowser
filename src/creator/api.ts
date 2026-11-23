import { invoke } from "@tauri-apps/api/core";

/** The browser mock build (`npm run dev:mock`), checked as upstream does. */
export const MOCK = import.meta.env.VITE_MOCK === "1";

/** Whether our backend answers our Settings tab (Shard draws the tab and its
 *  palette command only then). Ours always does since M2 (the Breeze Blue
 *  settings); the mock answers the rest. */
export const CREATOR_DATA: boolean = true;

/** One of our backend commands, through the single `creator_call` Tauri
 *  command; `npm run dev:mock` answers from mock.ts instead (never bundled
 *  into a release build: the import is behind the build-time flag). */
export const call = <T>(cmd: string, args?: Record<string, unknown>): Promise<T> =>
  MOCK
    ? import("./mock").then((m) => m.mockCall<T>(cmd, args))
    : invoke<T>("creator_call", { cmd, args });
