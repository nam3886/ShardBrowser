// Emails: every mailbox, narrowed with Shard's Filters (type, state, free
// for a platform), and one mailbox with its accounts and mail in Peek.
import { useState } from "react";
import { Badge, Button, EmptyState } from "@proxyshard/shardx-ui-kit";
import { HeaderMenu, PageBody, PageHeader } from "../../widgets/PageHeader";
import { AddIcon, DeleteIcon, DownloadIcon, RefreshIcon, SearchIcon } from "../../shared/icons";
import type { ListColumnDef } from "../../shared/ui/listHeader";
import { fmtTs } from "../../shared/lib/utils";
import { toast } from "../../shared/model/toast";
import { confirmModal } from "../../shared/model/confirm";
import { call } from "../api";
import { runPool } from "../pool";
import { ct, useCt } from "../i18n";
import type { EmailList, EmailRow, PlatformCard } from "../types";
import { AddEmailsDialog } from "../widgets/AddEmailsDialog";
import { BulkBar } from "../widgets/BulkBar";
import { EmailPeek } from "../widgets/EmailPeek";
import { AppliedFilters, FilterButton, type FilterField } from "../widgets/Filters";
import { DisplayMenu } from "../widgets/Display";
import { ListPage } from "../widgets/ListPage";
import { MakeDialog } from "../widgets/MakeDialog";
import { PlatformMark, platformLabel, useCall, useWindowed } from "../widgets/parts";

type Col = "email" | "type" | "accounts" | "mails" | "status" | "created";
const COLUMNS: readonly ListColumnDef<Col, EmailRow>[] = [
  { id: "email", title: () => ct("creator.col.email"), need: 240, min: 160, max: 480, shown: true, hideable: false, movable: false, text: (m) => m.email },
  {
    id: "type", title: () => ct("creator.col.type"), need: 120, min: 100, max: 200, shown: true, hideable: true, movable: true,
    fit: (m, w) => 20 + w(ct(`creator.mailbox.${m.provider}`), "ui"),
    cell: (m) => <span className="flex min-w-0 items-center gap-1.5"><PlatformMark k={m.provider} /><span className="truncate">{ct(`creator.mailbox.${m.provider}`)}</span></span>,
  },
  // The platforms with an account on the email, as text (their states are in Peek).
  { id: "accounts", title: () => ct("creator.col.accounts"), need: 160, min: 100, max: 360, shown: true, hideable: true, movable: true, role: "secondary", text: (m) => m.accounts.map((a) => platformLabel(a.platform)).join(", ") || "—" },
  { id: "mails", title: () => ct("creator.col.mails"), need: 56, min: 56, max: 120, shown: true, hideable: true, movable: true, role: "tabular", text: (m) => String(m.mails) },
  {
    id: "status", title: () => ct("creator.col.status"), need: 90, min: 80, max: 160, shown: true, hideable: true, movable: true,
    cell: (m) => <Badge color={m.status === "gone" ? "error" : "gray"} size="small">{ct(`creator.emailState.${m.status}`)}</Badge>,
  },
  { id: "created", title: () => ct("creator.col.created"), need: 120, min: 100, max: 200, shown: true, hideable: true, movable: true, role: "tabular", text: (m) => fmtTs(m.created_at) },
];

/** Test mailboxes as Shard tests proxies (useProxy.bulkTest): a "Testing"
 *  toast, the same continuous pool of 5, then one summary. */
const testEmails = async (ids: string[]) => {
  toast.info(ids.length === 1 ? ct("creator.emails.testingOne") : ct("creator.emails.testing", { n: ids.length }));
  const got = await runPool(ids, 5, (id) => call<"active" | "gone" | "failed">("test_mailbox", { id }).catch(() => "failed" as const));
  const r = { n: ids.length, active: got.filter((x) => x === "active").length, gone: got.filter((x) => x === "gone").length, failed: got.filter((x) => x === "failed").length };
  (r.active === r.n ? toast.ok : toast.info)(ct("creator.emails.testedSummary", r));
};

const confirmDeleteEmails = (n: number) => confirmModal({
  title: n === 1 ? ct("creator.confirm.deleteEmailOne") : ct("creator.confirm.deleteEmails", { n }),
  message: ct("creator.confirm.deleteEmailsBody"),
  confirmLabel: ct("creator.action.delete"),
  danger: true,
});

export function EmailsPage() {
  const ct = useCt();
  const [kind, setKind] = useState<string | null>(null);
  const [state, setState] = useState<string | null>(null);
  const [freeFor, setFreeFor] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [openId, setOpenId] = useState<number | null>(null);
  const [adding, setAdding] = useState(false);
  const [makeFor, setMakeFor] = useState<string | null>(null);
  const [order, setOrder] = useState("newest");
  const [layoutRev, setLayoutRev] = useState(0);
  const list = useWindowed<EmailList>("emails", { kind, status: state, free_for: freeFor, q, order });
  const plats = useCall<PlatformCard[]>("platforms", undefined, "");
  const data = list.data;
  const freeCount = plats.data?.find((p) => p.key === freeFor)?.free ?? 0;
  const filtered = !!(kind || state || freeFor);
  const nothing = list.status === "ready" && data?.counts.all === 0 && !q.trim() && !filtered;
  const n = selected.size;
  const pick = (set: (v: string | null) => void) => (v: string | null) => { set(v); setSelected(new Set()); };
  const fields: FilterField[] = [
    { id: "type", label: ct("creator.col.type"), current: kind, set: pick(setKind), values: (["outlook", "ele"] as const).map((k) => ({ value: k, label: ct(`creator.mailbox.${k}`), count: data?.counts[k], icon: <PlatformMark k={k} /> })) },
    { id: "state", label: ct("creator.col.status"), current: state, set: pick(setState), values: (["active", "gone"] as const).map((v) => ({ value: v, label: ct(`creator.emailState.${v}`) })) },
    { id: "free", label: ct("creator.emails.freeFor"), current: freeFor, set: pick(setFreeFor), values: (plats.data ?? []).map((p) => ({ value: p.key, label: p.label, count: p.free, icon: <PlatformMark k={p.key} /> })) },
  ];

  return (
    <>
      <PageHeader
        title={ct("creator.nav.emails")}
        count={data && !nothing ? (q.trim() || filtered ? { shown: data.total, total: data.counts.all } : data.counts.all) : undefined}
        help={{ title: ct("creator.emails.helpTitle"), body: ct("creator.emails.helpBody") }}
        filter={nothing ? undefined : { value: q, onChange: (v) => { setQ(v); setSelected(new Set()); } }}
        actions={(
          <div className="flex flex-none items-center gap-2">
            {!nothing && <FilterButton fields={fields} />}
            {!nothing && (
              <DisplayMenu
                storageKey="creator:emails.layout"
                defs={COLUMNS}
                onChanged={() => setLayoutRev((n) => n + 1)}
                order={{ value: order, initial: "newest", onChange: setOrder, options: [
                  { value: "newest", label: ct("creator.display.newest") },
                  { value: "oldest", label: ct("creator.display.oldest") },
                  { value: "email", label: ct("creator.display.email") },
                ] }}
              />
            )}
            <HeaderMenu items={[
              { label: ct("creator.emails.checkAll"), icon: <RefreshIcon className="size-3.5" />, onSelect: () => void testEmails(list.items.map((m) => String(m.id))) },
              { label: ct("creator.emails.exportAll"), icon: <DownloadIcon className="size-3.5" />, onSelect: () => {} },
            ]} />
            <Button variant="primary" mode="filled" size="xsmall" leftIcon={<AddIcon className="size-3.5" />} onClick={() => setAdding(true)}>
              {ct("creator.addEmails.open")}
            </Button>
          </div>
        )}
      />
      <PageBody scroll={false} className="relative">
        <ListPage<Col, EmailRow>
          key={layoutRev}
          label={ct("creator.nav.emails")}
          above={(
            // Python's "Make N" from "Free for": Make accounts with the platform and count filled in.
            <AppliedFilters fields={fields} after={freeFor && freeCount > 0 ? (
              <Button variant="neutral" mode="stroke" size="3xsmall" leftIcon={<AddIcon className="size-3.5" />} onClick={() => setMakeFor(freeFor)}>
                {freeCount === 1 ? ct("creator.emails.makeOne", { name: platformLabel(freeFor) }) : ct("creator.emails.make", { n: Math.min(freeCount, 500), name: platformLabel(freeFor) })}
              </Button>
            ) : undefined} />
          )}
          layoutKey="creator:emails.layout"
          defs={COLUMNS}
          items={list.items}
          onEnd={list.more}
          status={list.status}
          error={list.error}
          onRetry={list.reload}
          what={ct("creator.emails.what")}
          name={(m) => m.email}
          empty={q.trim() ? (
            <EmptyState icon={<SearchIcon />} title={ct("creator.emails.noMatch", { q: q.trim() })} actions={<Button variant="neutral" mode="stroke" size="xsmall" onClick={() => setQ("")}>{ct("creator.list.clearSearch")}</Button>} />
          ) : (
            <EmptyState icon={<AddIcon />} title={ct("creator.emails.emptyTitle")} hint={ct("creator.emails.emptyHint")} action={{ label: ct("creator.addEmails.open"), icon: <AddIcon />, onClick: () => setAdding(true) }} />
          )}
          selected={selected}
          onSelected={setSelected}
          onOpen={(m) => setOpenId(m.id)}
          openId={openId}
          tail={40}
          bar={(
            <BulkBar
              n={n}
              onClear={() => setSelected(new Set())}
              actions={[
                { id: "check", label: ct("creator.emails.check"), icon: <RefreshIcon className="size-3.5" />, onClick: () => void testEmails([...selected]) },
                { id: "export", label: ct("creator.action.export"), icon: <DownloadIcon className="size-3.5" />, onClick: () => {} },
              ]}
              more={[{ label: ct("creator.action.delete"), icon: <DeleteIcon className="size-3.5" />, danger: true, onSelect: () => void confirmDeleteEmails(n) }]}
            />
          )}
          peek={<EmailPeek email={list.items.find((m) => m.id === openId) ?? null} onClose={() => setOpenId(null)} />}
        />
      </PageBody>
      <AddEmailsDialog open={adding} onClose={() => setAdding(false)} />
      <MakeDialog open={!!makeFor} platform={makeFor ?? undefined} count={Math.min(freeCount, 500)} onClose={() => setMakeFor(null)} />
    </>
  );
}
