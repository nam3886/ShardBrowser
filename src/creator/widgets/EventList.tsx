// An account's log, drawn as Proxy Peek draws a proxy's IP history: one item
// a step, its message on the first line, when and which step on a muted
// second line; an error says so in the error text, as a failed test's
// reason does there. Nothing logged: Proxy Peek's muted "none yet" line.
import { LoadError, LoadingRows } from "../../shared/ui/ListStates";
import { fmtTs } from "../../shared/lib/utils";
import { useCt } from "../i18n";
import type { EventRow } from "../types";
import { useCall } from "./parts";

export function EventList({ accountId }: { accountId: number }) {
  const ct = useCt();
  const { data, error, status, reload } = useCall<EventRow[]>("account_events", { id: accountId }, String(accountId));
  if (status === "loading") return <LoadingRows rows={3} rowHeight={28} />;
  if (status === "error") return <LoadError what={ct("creator.events.what")} error={error} onRetry={reload} headingLevel={3} />;
  if (!data?.length) return <p className="m-0 text-secondary text-text-soft-400">{ct("creator.events.emptyTitle")}</p>;
  return (
    <ul data-creator-events className="m-0 flex list-none flex-col p-0">
      {[...data].reverse().map((e) => (
        <li key={e.id} data-level={e.level} data-selectable className="py-2">
          <div className={e.level === "error" ? "text-body text-error-text" : e.level === "warn" ? "text-body text-warning-text" : "text-body text-text-strong-950"}>
            {e.level !== "info" && <span className="sr-only">{ct(`creator.events.level.${e.level}`)}: </span>}
            {e.message}
          </div>
          <div className="mt-0.5 text-secondary text-text-soft-400">{fmtTs(e.at)}, {e.step}</div>
        </li>
      ))}
    </ul>
  );
}
