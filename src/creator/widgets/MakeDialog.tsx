// Make accounts: the Python make bar as a dialog, on the model of Shard's
// New profile (a header primary opens a DialogModal form). The service, the
// proxy mode and the kind are kept per viewer, as Shard's localPrefs keep a
// window's own choices.
import { useEffect, useState } from "react";
import { DialogModal, Radio, SegmentControl } from "@proxyshard/shardx-ui-kit";
import { FieldGroup } from "../../shared/ui/FieldGroup";
import { NumField } from "../../shared/ui/NumField";
import { SelectField } from "../../shared/ui/SelectField";
import { toast } from "../../shared/model/toast";
import { call, MOCK } from "../api";
import { useCt } from "../i18n";
import type { MakeBlock, PlatformCard } from "../types";
import { PlatformMark, platformLabel } from "./parts";

const KEY = "creator:make";
type Choice = { k: string; proxy: "rotate" | "least" | "none"; kind: "" | "residential" | "datacenter" | "mobile" };
const loadChoice = (): Choice => {
  try { return { k: "breezeblue", proxy: "rotate", kind: "", ...JSON.parse(localStorage.getItem(KEY) ?? "{}") }; }
  catch { return { k: "breezeblue", proxy: "rotate", kind: "" }; }
};

/** Most accounts one Make run takes (the backend's `breeze_run` limit). */
export const MAX = 20;

export function MakeDialog({ open, platform, route: givenRoute, count: given, onClose, onStarted }: { open: boolean; platform?: string; route?: string; count?: number; onClose: () => void; onStarted?: () => void }) {
  const ct = useCt();
  const [choice, setChoice] = useState(loadChoice);
  // The real backend makes Breeze Blue only (M2); the mock, every service.
  const k = platform ?? (MOCK ? choice.k : "breezeblue");
  const [count, setCount] = useState(given ?? 5);
  useEffect(() => { if (open && given) setCount(Math.min(given, MAX)); }, [open, given]);
  const [route, setRoute] = useState(givenRoute ?? "");
  useEffect(() => { if (open && givenRoute) setRoute(givenRoute); }, [open, givenRoute]);
  const [platforms, setPlatforms] = useState<PlatformCard[]>([]);
  // Free emails per route; null: a new mailbox is made for each account (Ele).
  const [options, setOptions] = useState<Record<string, number | null>>({});
  useEffect(() => { if (open) void call<PlatformCard[]>("platforms").then(setPlatforms, () => {}); }, [open]);
  useEffect(() => { if (open) void call<Record<string, number | null>>("make_options", { platform: k }).then(setOptions, () => setOptions({})); }, [open, k]);
  // A route this service does not offer: its first one.
  const routes = Object.keys(options);
  useEffect(() => {
    const rs = Object.keys(options);
    if (rs.length && !rs.includes(route)) setRoute(rs[0]);
  }, [options, route]);
  // What stops a run before it starts (a missing key, the engine, proxies):
  // the dialog's own note, as New profile says why it cannot create.
  const [blocks, setBlocks] = useState<MakeBlock[]>([]);
  useEffect(() => { if (open) void call<MakeBlock[]>("make_check", { platform: k, route, proxy: choice.proxy }).then(setBlocks, () => setBlocks([])); }, [open, k, route, choice.proxy]);
  const set = (next: Partial<Choice>) => {
    const c = { ...choice, ...next };
    setChoice(c);
    try { localStorage.setItem(KEY, JSON.stringify(c)); } catch { /* storage blocked: kept for this session */ }
  };
  const routeLabel = (r: string) => (r === "" ? ct("creator.make.routeAuto") : r.startsWith("email:") ? ct(`creator.make.route.${r.slice(6)}`) : ct("creator.make.routeVia", { name: platformLabel(r) }));
  const free = route in options ? options[route] : 0;
  const short = free !== null && count > free;
  const block = blocks[0];

  return (
    <DialogModal
      open={open}
      onClose={onClose}
      title={ct("creator.make.title")}
      confirmLabel={ct("creator.make.confirm", { n: count })}
      isDisabled={short || !!block}
      footerNote={block ? ct(`creator.make.block.${block}`) : short ? (free === 1 ? ct("creator.make.shortOne") : ct("creator.make.short", { n: free })) : undefined}
      onConfirm={() => void call("breeze_run", { count, proxy_mode: choice.proxy, route }).then(() => {
        toast.ok(ct("creator.make.queued", { n: count, name: platformLabel(k) }));
        onStarted?.();
        onClose();
      }, (e) => toast.fail({ title: ct("creator.make.failed"), detail: e }))}
    >
      <div className="flex flex-col gap-4 py-4">
        <div className="grid grid-cols-[minmax(0,1fr)_120px] gap-3">
          <SelectField
            label={ct("creator.make.service")}
            value={k}
            onChange={(v) => { set({ k: v }); setRoute(""); }}
            options={platforms.filter((p) => p.ready).map((p) => ({ value: p.key, label: p.label, icon: <PlatformMark k={p.key} /> }))}
            disabled={!!platform}
          />
          <NumField label={ct("creator.make.count")} value={count} min={1} max={MAX} onChange={setCount} />
        </div>
        <FieldGroup label={ct("creator.make.through")}>
          {(labelId) => (
            // Each choice with its help line under it, as Sync's Disconnect dialog draws its two.
            <div role="radiogroup" aria-labelledby={labelId} className="grid grid-cols-2 gap-3">
              {routes.map((r) => (
                <div key={r} className="flex flex-col gap-1">
                  <Radio name="creator-route" checked={route === r} onChange={() => setRoute(r)} label={routeLabel(r)} aria-describedby={`creator-route-${r}`} />
                  <p id={`creator-route-${r}`} className="m-0 ps-[24px] text-help text-text-sub-600">
                    {options[r] === null ? ct("creator.make.newMailbox") : ct("creator.make.free", { n: options[r] ?? 0 })}
                  </p>
                </div>
              ))}
            </div>
          )}
        </FieldGroup>
        <FieldGroup label={ct("creator.make.proxy")}>
          {(labelId) => (
            <SegmentControl
              aria-labelledby={labelId}
              size="small"
              value={choice.proxy}
              onChange={(v) => set({ proxy: v as Choice["proxy"] })}
              items={[
                { value: "rotate", label: ct("creator.make.proxyRotate") },
                { value: "least", label: ct("creator.make.proxyLeast") },
                { value: "none", label: ct("creator.make.proxyNone") },
              ]}
            />
          )}
        </FieldGroup>
        {/* IP type comes with our Proxy pool page (M7). */}
        {MOCK && choice.proxy !== "none" && (
          <SelectField
            label={ct("creator.make.kind")}
            value={choice.kind}
            onChange={(v) => set({ kind: v })}
            options={[
              { value: "" as const, label: ct("creator.make.kindAuto") },
              { value: "residential" as const, label: ct("creator.ipType.residential") },
              { value: "datacenter" as const, label: ct("creator.ipType.datacenter") },
              { value: "mobile" as const, label: ct("creator.ipType.mobile") },
            ]}
          />
        )}
      </div>
    </DialogModal>
  );
}
