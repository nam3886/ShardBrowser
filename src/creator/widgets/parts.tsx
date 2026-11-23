// Small pieces our pages share: data loading, the platform mark and the
// status pills. Each is drawn from Shard's kit recipes (Badge pill + StatusDot,
// the label-chip tint), never a style of our own.
import { useCallback, useEffect, useId, useState, type CSSProperties } from "react";
import { Badge, Button, CopyButton, type BadgeColor } from "@proxyshard/shardx-ui-kit";
import { clip, copySecret } from "../../shared/lib/clipboard";
import { useT } from "../../shared/i18n";
import { labelColor } from "../../shared/constants";
import { SpinnerIcon } from "../../shared/icons";
import { StatusDot, type StatusDotTone } from "../../shared/ui/StatusDot";
import { useHeldLoading } from "../../shared/hooks/useHeldLoading";
import { confirmModal } from "../../shared/model/confirm";
import { toast } from "../../shared/model/toast";
import { useProfileUi } from "../../entities/profile";
import { call } from "../api";
import { ct, useCt } from "../i18n";
import type { AccountRow, StatusGroup } from "../types";

/** One backend read, again whenever `key` changes: Shard's held loading
 *  (placeholder rows wait 150 ms, stay 400 ms) and an error with Retry. */
export function useCall<T>(cmd: string, args: Record<string, unknown> | undefined, key: string) {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(true);
  const load = useCallback(() => {
    setBusy(true);
    call<T>(cmd, args).then((d) => { setData(d); setError(null); }, (e) => setError(String(e))).finally(() => setBusy(false));
    // eslint-disable-next-line react-hooks/exhaustive-deps -- `key` stands for `args`
  }, [cmd, key]);
  useEffect(load, [load]);
  const loading = useHeldLoading(busy && data === null && error === null);
  return { data, error, loading, status: (loading || (busy && data === null && !error) ? "loading" : error ? "error" : "ready") as "loading" | "error" | "ready", reload: load };
}

/** How many rows one read brings: the list asks for the next window as its
 *  end scrolls into view (38k emails never load at once). */
export const WINDOW = 50;

/** A list read in windows behind one scrolling list (Shard's lists scroll,
 *  they never page): the first window carries the counts, `more()` appends
 *  the next one. A new `spec` starts over. */
export function useWindowed<R extends { items: unknown[]; total: number }>(cmd: string, spec: Record<string, unknown>) {
  const key = JSON.stringify(spec);
  const first = useCall<R>(cmd, { ...spec, offset: 0, limit: WINDOW }, key);
  const [extra, setExtra] = useState<{ key: string; items: R["items"] }>({ key, items: [] });
  const [busy, setBusy] = useState(false);
  const rest = extra.key === key ? extra.items : [];
  const items = [...(first.data?.items ?? []), ...rest] as R["items"];
  const more = () => {
    if (busy || !first.data || items.length >= first.data.total) return;
    setBusy(true);
    call<R>(cmd, { ...spec, offset: items.length, limit: WINDOW })
      .then((r) => setExtra({ key, items: [...rest, ...r.items] }), () => {})
      .finally(() => setBusy(false));
  };
  return { ...first, items, more };
}

// ponytail: initials on a label-chip tint stand in for brand logos (owner gap: logo assets).
const MARK: Record<string, { color: string; text: string }> = {
  google: { color: "blue", text: "G" }, qwen: { color: "violet", text: "Q" }, chatgpt: { color: "green", text: "AI" },
  grok: { color: "sky", text: "X" }, elevenlabs: { color: "orange", text: "11" }, breezeblue: { color: "magenta", text: "BB" },
  outlook: { color: "blue", text: "O" }, ele: { color: "yellow", text: "E" }, noiz: { color: "orange", text: "N" },
};

/** Real logos (src/creator/logos/SOURCES.md), inlined so mono ones draw in the
 *  text color (currentColor) and hold in both themes. */
const LOGOS: Record<string, string> = Object.fromEntries(
  Object.entries(import.meta.glob("../logos/*.svg", { query: "?raw", import: "default", eager: true }) as Record<string, string>)
    .map(([path, svg]) => [path.slice(path.lastIndexOf("/") + 1, -4), svg]),
);

/** One size per context, Shard's: 14 px in rows, menus, chips and fields
 *  (its icon slot, `size-3.5`); `card`: ExtensionCard's 35 px tile with the
 *  mark at 20 px. A service with no logo file keeps its initials at the same sizes. */
export function PlatformMark({ k, card = false }: { k: string; card?: boolean }) {
  // Each mark's own ids (Noiz's masks, Qwen's gradient) made unique per mark on the page.
  const uid = useId().replace(/:/g, "");
  const raw = LOGOS[k];
  const logo = raw && raw.replace(/id="([^"]+)"/g, `id="$1-${uid}"`).replace(/url\(#([^)]+)\)/g, `url(#$1-${uid})`);
  if (logo) {
    const svg = <span aria-hidden="true" className={`${card ? "size-[20px]" : "size-3.5"} flex shrink-0 text-text-strong-950 [&>svg]:size-full`} dangerouslySetInnerHTML={{ __html: logo }} />;
    return card
      ? <span aria-hidden="true" className="grid size-[35px] shrink-0 place-items-center rounded-control bg-bg-weak-50 forced-edge">{svg}</span>
      : svg;
  }
  const m = MARK[k] ?? { color: "", text: k.slice(0, 1).toUpperCase() };
  return (
    <span
      aria-hidden="true"
      style={{ "--label": labelColor(m.color) } as CSSProperties}
      className={card
        ? "label-chip grid size-[35px] shrink-0 place-items-center rounded-control text-secondary font-medium"
        : "label-chip inline-grid size-3.5 shrink-0 place-items-center rounded-chip-sm text-secondary font-medium leading-none"}
    >
      {/* At 14 px one letter fits; the card's tile has room for the whole mark. */}
      {card ? m.text : m.text.slice(0, 1)}
    </span>
  );
}

export const platformLabel = (k: string) => ct(`creator.platform.${k}`);

/** The list group an account is in (the status chips). */
export const groupOf = (a: Pick<AccountRow, "status" | "verify_status">): StatusGroup =>
  a.status === "linked" && (a.verify_status === "signed_out" || a.verify_status === "expired") ? "signed_out"
  : a.status === "queued" || a.status === "running" ? "making" : a.status as StatusGroup;

const GROUP: Record<StatusGroup, { color: BadgeColor; dot: StatusDotTone | "spin" }> = {
  linked: { color: "success", dot: "success" },
  signed_out: { color: "warning", dot: "warning" },
  making: { color: "information", dot: "spin" },
  failed: { color: "error", dot: "error" },
};

function Pill({ color, dot, label }: { color: BadgeColor; dot: StatusDotTone | "spin" | null; label: string }) {
  return (
    <Badge
      shape="pill"
      color={color}
      className="max-w-full"
      lead={dot === "spin" ? <SpinnerIcon className="size-3.5 animate-spin" /> : dot ? <StatusDot tone={dot} /> : null}
    >
      <span className="min-w-0 truncate">{label}</span>
    </Badge>
  );
}

/** An account's state as Shard's status pill draws a profile's. */
export function AccountPill({ a }: { a: Pick<AccountRow, "status" | "verify_status"> }) {
  const g = groupOf(a);
  const label = a.status === "queued" ? ct("creator.status.queued") : ct(`creator.status.${g}`);
  return <Pill {...GROUP[g]} dot={a.status === "queued" ? "info" : GROUP[g].dot} label={label} />;
}

/** Deleting accounts asks first, in Shard's confirm (confirmModal: the button names the act). */
export const confirmDelete = (n: number) => confirmModal({
  title: n === 1 ? ct("creator.confirm.deleteAccountOne") : ct("creator.confirm.deleteAccounts", { n }),
  message: ct("creator.confirm.deleteAccountsBody"),
  confirmLabel: ct("creator.action.delete"),
  danger: true,
}).then((yes) => { if (yes) toast.ok(n === 1 ? ct("creator.bulk.deletedOne") : ct("creator.bulk.deleted", { n })); });

/** A failure's words, from its code (engine/errors.py); a code we don't know yet reads as OTHER. */
export const failText = (code: string) => {
  const k = `creator.error.${code}`;
  const t = ct(k);
  return t === k ? ct("creator.error.OTHER") : t;
};

/** A Peek row's copy button, Shard's DetailRow recipe (shared/ui/DetailsCard):
 *  the kit's quiet CopyButton named "Copy {label}", "Copied" in a toast; a
 *  secret clears from the clipboard after a minute. `value` may be a read
 *  (a password, a key): it is fetched when Copy is pressed, never before. */
export function CopyValue({ label, value, secret = false }: { label: string; value: string | (() => Promise<string>); secret?: boolean }) {
  const t = useT();
  const onCopy = async () => {
    try {
      const v = typeof value === "string" ? value : await value();
      if (secret) { await copySecret(v); toast.ok(t("clipboard.copiedSecret")); }
      else { await clip.write(v); toast.ok(t("kit.copied")); }
    } catch (e) {
      toast.fail({ title: t("copyField.failed"), detail: e });
      throw e;
    }
  };
  return <CopyButton size="3xsmall" className="shrink-0" label={t("details.copy", { label })} onCopy={onCopy} />;
}

/** The account's browser, a Shard profile (D17): its name as Shard links a
 *  profile from Proxy Peek's "Profiles using it" (a ghost button that shows
 *  it in All profiles), then Shard's copy button for its id. */
export function ProfileLink({ id, name }: { id: string; name: string }) {
  const ct = useCt();
  return (
    <>
      <Button variant="neutral" mode="ghost" size="3xsmall" className="min-w-0" onClick={() => useProfileUi.getState().showInList(id)}>
        <span className="truncate">{name}</span>
      </Button>
      <span className="ms-auto" />
      <CopyValue label={ct("creator.peek.profileId")} value={id} />
    </>
  );
}
