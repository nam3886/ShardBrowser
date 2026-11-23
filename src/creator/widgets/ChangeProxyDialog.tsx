// Change proxy for the selected accounts (Python's Change proxy drawer), on
// Shard's Assign proxies dialog: a DialogModal with the choice and its
// preview line in the footer. The choices are drawn as Sync's Disconnect
// dialog draws its own: a Radio with its help line under it.
import { useState } from "react";
import { DialogModal, Radio } from "@proxyshard/shardx-ui-kit";
import { fmtTs } from "../../shared/lib/utils";
import { toast } from "../../shared/model/toast";
import { useCt } from "../i18n";
import type { ProxyList } from "../types";
import { useCall } from "./parts";

export function ChangeProxyDialog({ n, open, onClose }: { n: number; open: boolean; onClose: () => void }) {
  const ct = useCt();
  const [pick, setPick] = useState("any");
  const list = useCall<ProxyList>("proxies", { status: "working", limit: 6 }, String(open));
  if (!open) return null;
  const choices = [
    { value: "any", label: ct("creator.changeProxy.any"), help: ct("creator.changeProxy.anyHelp") },
    ...(list.data?.items ?? []).map((p) => ({
      value: String(p.id),
      label: `${p.name} (${p.host}:${p.port})`,
      help: [p.test?.tcp_ms != null ? `${p.test.tcp_ms} ms` : "", p.test?.country_code ?? "", p.test ? fmtTs(p.test.at) : ""].filter(Boolean).join(", "),
    })),
  ];
  return (
    <DialogModal
      open
      onOpenChange={(o) => { if (!o) onClose(); }}
      title={n === 1 ? ct("creator.changeProxy.titleOne") : ct("creator.changeProxy.title", { n })}
      confirmLabel={ct("creator.changeProxy.confirm")}
      footerNote={ct("creator.changeProxy.note")}
      onConfirm={() => { toast.ok(ct("creator.changeProxy.done", { n })); onClose(); }}
    >
      <div role="radiogroup" aria-label={ct("creator.col.proxy")} className="flex flex-col gap-3 py-4">
        {choices.map((c) => (
          <div key={c.value} className="flex flex-col gap-1">
            <Radio name="creator-change-proxy" checked={pick === c.value} onChange={() => setPick(c.value)} label={c.label} aria-describedby={`cp-${c.value}`} />
            <p id={`cp-${c.value}`} className="m-0 ps-[24px] text-help text-text-sub-600">{c.help}</p>
          </div>
        ))}
      </div>
    </DialogModal>
  );
}
