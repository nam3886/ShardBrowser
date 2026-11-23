// Accounts, laid out as Shard's All profiles page: the header with Filters,
// More and the primary; over the list, the overview's folding card sections
// ("Running now" with the job's usage bar, "Problems to fix" for a halted
// queue or a missing setup); the applied filters as chips; the list; Peek
// beside it. Sign-ups are fully automatic: nothing waits for the person.
import { useState } from "react";
import { Badge, Button, EmptyState, Tooltip } from "@proxyshard/shardx-ui-kit";
import { HeaderMenu, PageBody, PageHeader } from "../../widgets/PageHeader";
import { AddIcon, CopyIcon, DeleteIcon, DownloadIcon, FilterIcon, LockedIcon, PauseIcon, RefreshIcon, SearchIcon, StopIcon, UploadIcon } from "../../shared/icons";
import { CountryFlag } from "../../shared/ui/CountryFlag";
import type { ListColumnDef } from "../../shared/ui/listHeader";
import { toast } from "../../shared/model/toast";
import { useNav } from "../../shared/model/navigation";
import { confirmModal } from "../../shared/model/confirm";
import { ct, useCt } from "../i18n";
import type { AccountList, AccountRow, RunState } from "../types";
import { AccountPeek } from "../widgets/AccountPeek";
import { BulkBar } from "../widgets/BulkBar";
import { card, CardSection } from "../widgets/CardSection";
import { AppliedFilters, FilterButton, type FilterField } from "../widgets/Filters";
import { DisplayMenu } from "../widgets/Display";
import { ListPage } from "../widgets/ListPage";
import { MakeDialog } from "../widgets/MakeDialog";
import { ChangeProxyDialog } from "../widgets/ChangeProxyDialog";
import { AccountPill, confirmDelete, failText, groupOf, PlatformMark, platformLabel, useCall, useWindowed } from "../widgets/parts";

// Fixed parts of a cell beside its text, px: the platform mark and its gap,
// a status pill's dot and padding, a flag and its gap, a small Badge's sides.
const MARK = 20; // the 14 px mark and its 6 px gap
const PILL = 27;
const FLAG = 29;
const BADGE = 10;

type Col = "email" | "service" | "status" | "route" | "proxy" | "flags";
const COLUMNS: readonly ListColumnDef<Col, AccountRow>[] = [
  { id: "email", title: () => ct("creator.col.email"), need: 220, min: 160, max: 480, shown: true, hideable: false, movable: false, text: (a) => a.email },
  {
    id: "service", title: () => ct("creator.col.service"), need: 130, min: 110, max: 220, shown: true, hideable: true, movable: true,
    fit: (a, m) => MARK + m(platformLabel(a.platform), "ui"),
    cell: (a) => <span className="flex min-w-0 items-center gap-1.5"><PlatformMark k={a.platform} /><span className="truncate">{platformLabel(a.platform)}</span></span>,
  },
  {
    id: "status", title: () => ct("creator.col.status"), need: 120, min: 110, max: 360, shown: true, hideable: false, movable: true,
    fit: (a, m) => PILL + m(ct(`creator.status.${groupOf(a)}`), "medium") + (a.step || a.fail ? 6 + m(a.step ?? failText(a.fail!.code), "secondary") : 0),
    // The state, then what a live account is doing or why it failed, as a
    // Proxies row puts the test's IP beside its pill.
    cell: (a) => (
      <span className="flex min-w-0 items-center gap-1.5">
        <AccountPill a={a} />
        {(a.step || a.fail) && (
          <Tooltip content={a.step ?? a.error ?? ""} whenTruncated>
            <span className="min-w-0 truncate text-secondary text-text-sub-600">{a.step ?? (a.fail ? failText(a.fail.code) : "")}</span>
          </Tooltip>
        )}
      </span>
    ),
  },
  { id: "route", title: () => ct("creator.col.route"), need: 110, min: 90, max: 200, shown: false, hideable: true, movable: true, role: "secondary", text: (a) => (a.route === "email" ? ct("creator.route.email") : platformLabel(a.route)) },
  {
    id: "proxy", title: () => ct("creator.col.proxy"), need: 170, min: 120, max: 260, shown: true, hideable: true, movable: true,
    fit: (a, m) => (a.proxy ? FLAG + m(a.proxy, "mono") : m(ct("creator.proxy.direct"), "secondary")),
    cell: (a) => (a.proxy
      ? <span className="flex min-w-0 items-center gap-1.5">{a.proxy_cc && <CountryFlag cc={a.proxy_cc} />}<span className="truncate text-mono text-text-sub-600">{a.proxy}</span></span>
      : <span className="text-secondary text-text-soft-400">{ct("creator.proxy.direct")}</span>),
  },
  {
    // As the Trash list's countdown: small gray Badges.
    id: "flags", title: () => ct("creator.col.flags"), need: 90, min: 80, max: 220, shown: true, hideable: true, movable: true,
    fit: (a, m) => (a.frozen ? BADGE + m(ct("creator.flag.frozen"), "medium") : 0) + (a.relay_id ? 4 + BADGE + m(ct("creator.flag.pushed"), "medium") : 0),
    cell: (a) => (
      <span className="flex min-w-0 items-center gap-1">
        {a.frozen && <Badge color="gray" size="small">{ct("creator.flag.frozen")}</Badge>}
        {a.relay_id && <Badge color="gray" size="small">{ct("creator.flag.pushed")}</Badge>}
      </span>
    ),
  },
];

const GROUPS = ["linked", "signed_out", "making", "failed"] as const;
const FLAGS = ["frozen", "pushed", "not_pushed"] as const;

export function AccountsPage() {
  const ct = useCt();
  const [tab, setTab] = useState<string | null>(null);
  const [status, setStatus] = useState<string | null>(null);
  const [flag, setFlag] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [openId, setOpenId] = useState<number | null>(null);
  const [making, setMaking] = useState(false);
  const [changing, setChanging] = useState(false);
  const [order, setOrder] = useState("newest");
  const [layoutRev, setLayoutRev] = useState(0);
  const list = useWindowed<AccountList>("accounts", { tab, status, flag, q, order });
  const run = useCall<RunState>("run_state", undefined, "");
  const data = list.data;
  const filtered = !!(tab || status || flag);
  const nothing = list.status === "ready" && data?.counts.all === 0 && !q.trim() && !filtered;
  const open = list.items.find((a) => a.id === openId) ?? null;
  const n = selected.size;
  const pick = (set: (v: string | null) => void) => (v: string | null) => { set(v); setSelected(new Set()); };

  const fields: FilterField[] = [
    {
      id: "service", label: ct("creator.col.service"), current: tab, set: pick(setTab),
      values: (data?.tabs ?? []).map((t) => ({ value: t.platform, label: platformLabel(t.platform), count: t.count, icon: <PlatformMark k={t.platform} /> })),
    },
    { id: "status", label: ct("creator.col.status"), current: status, set: pick(setStatus), values: GROUPS.map((g) => ({ value: g, label: ct(`creator.status.${g}`), count: data?.counts[g] })) },
    { id: "flag", label: ct("creator.col.flags"), current: flag, set: pick(setFlag), values: FLAGS.map((f) => ({ value: f, label: ct(`creator.flag.${f}`) })) },
  ];

  const r = run.data;
  const running = (r?.jobs ?? []).map((j) => card(`job-${j.job_id}`, {
    label: ct("creator.job.label"),
    pill: { color: "information", text: platformLabel(j.platform) },
    what: ct("creator.job.making", { n: j.total, name: platformLabel(j.platform) }),
    why: r?.setup.engine_busy ? ct("creator.job.engineWait") : j.step ?? "",
    detail: "",
    usage: { share: (j.linked + j.failed) / j.total, used: ct("creator.job.done", { done: j.linked + j.failed, total: j.total }), ends: ct("creator.job.failed", { n: j.failed }) },
    fix: { id: "stop", label: ct("creator.job.stop"), run: () => {} },
    more: null,
  }));
  // What a run needs that is missing (first run, a lost key): Shard's
  // "Problems to fix" cards, each with the one fix. The engine's download,
  // update and swap are Shard's own screens (FirstRunGate, EngineUpdateBanner).
  const nav = useNav.getState();
  const setup = r?.setup;
  const setupCards = !setup ? [] : [
    ...(!setup.ele ? [card("setup-ele", {
      label: ct("creator.setup.ele"), pill: { color: "error", text: ct("creator.setup.notSet") },
      what: ct("creator.setup.eleWhat"), why: ct("creator.setup.eleWhy"), detail: "",
      fix: { id: "settings", label: ct("creator.setup.openSettings"), run: () => void nav.openSettings("creator") }, more: null,
    })] : []),
    ...(setup.proxies === 0 ? [card("setup-proxies", {
      label: ct("creator.nav.proxies"), pill: { color: "warning", text: ct("creator.setup.none") },
      what: ct("creator.setup.proxiesWhat"), why: ct("creator.setup.proxiesWhy"), detail: "",
      fix: { id: "add", label: ct("creator.setup.addProxies"), run: () => void nav.setSection("c-proxies") }, more: null,
    })] : []),
  ];
  const needs = [
    ...setupCards,
    ...(r?.halted ?? []).map((h) => card(`halted-${h.platform}`, {
      label: ct("creator.needs.halted"),
      pill: { color: "error", text: ct("creator.needs.failures", { n: h.n }) },
      what: ct("creator.accounts.halted", { name: platformLabel(h.platform), n: h.n }),
      why: ct("creator.needs.haltedWhy", { name: platformLabel(h.platform) }),
      detail: `${h.error} (${h.code})`,
      fix: { id: "resume", label: ct("creator.accounts.resume"), run: () => toast.ok(ct("creator.accounts.resumed", { name: platformLabel(h.platform) })) },
      more: null,
    })),
  ];

  return (
    <>
      <PageHeader
        title={ct("creator.nav.accounts")}
        count={data && !nothing ? (q.trim() || filtered ? { shown: data.total, total: data.counts.all } : data.counts.all) : undefined}
        help={{ title: ct("creator.accounts.helpTitle"), body: ct("creator.accounts.helpBody") }}
        filter={nothing ? undefined : { value: q, onChange: (v) => { setQ(v); setSelected(new Set()); } }}
        actions={(
          <div className="flex flex-none items-center gap-2">
            {!nothing && <FilterButton fields={fields} />}
            {!nothing && (
              <DisplayMenu
                storageKey="creator:accounts.layout"
                defs={COLUMNS}
                onChanged={() => setLayoutRev((n) => n + 1)}
                order={{ value: order, initial: "newest", onChange: setOrder, options: [
                  { value: "newest", label: ct("creator.display.newest") },
                  { value: "oldest", label: ct("creator.display.oldest") },
                  { value: "email", label: ct("creator.display.email") },
                  { value: "status", label: ct("creator.display.status") },
                ] }}
              />
            )}
            <HeaderMenu items={[
              { label: ct("creator.accounts.pauseAll"), icon: <PauseIcon className="size-3.5" />, onSelect: () => {} },
              { label: ct("creator.accounts.closeBrowsers"), icon: <StopIcon className="size-3.5" />, onSelect: () => void confirmModal({
                title: ct("creator.confirm.closeAllTitle"), message: ct("creator.confirm.closeAllBody"), confirmLabel: ct("creator.accounts.closeBrowsers"), danger: true,
              }) },
            ]} />
            <Button variant="primary" mode="filled" size="xsmall" leftIcon={<AddIcon className="size-3.5" />} onClick={() => setMaking(true)}>
              {ct("creator.make.open")}
            </Button>
          </div>
        )}
      />
      <PageBody scroll={false} className="relative">
        <ListPage<Col, AccountRow>
          key={layoutRev}
          label={ct("creator.nav.accounts")}
          above={(
            <>
              <CardSection id="running" label={ct("creator.cards.running")} cards={running} />
              <CardSection id="needs" label={ct("creator.cards.needs")} cards={needs} />
              <AppliedFilters fields={fields} />
            </>
          )}
          layoutKey="creator:accounts.layout"
          defs={COLUMNS}
          items={list.items}
          onEnd={list.more}
          status={list.status}
          error={list.error}
          onRetry={list.reload}
          what={ct("creator.accounts.what")}
          name={(a) => a.email}
          empty={q.trim() ? (
            <EmptyState icon={<SearchIcon />} title={ct("creator.accounts.noMatch", { q: q.trim() })} actions={<Button variant="neutral" mode="stroke" size="xsmall" onClick={() => setQ("")}>{ct("creator.list.clearSearch")}</Button>} />
          ) : nothing ? (
            <EmptyState icon={<AddIcon />} title={ct("creator.accounts.emptyTitle")} hint={ct("creator.accounts.emptyHint")} action={{ label: ct("creator.make.open"), icon: <AddIcon />, onClick: () => setMaking(true) }} />
          ) : (
            <EmptyState icon={<FilterIcon />} title={ct("creator.accounts.filtersNoMatch")} actions={<Button variant="neutral" mode="stroke" size="xsmall" onClick={() => fields.forEach((f) => f.set(null))}>{ct("creator.list.clearFilters")}</Button>} />
          )}
          selected={selected}
          onSelected={setSelected}
          onOpen={(a) => setOpenId(a.id)}
          openId={openId}
          tail={96}
          // The row's one hover action, per state, as Python's row button: Retry, Log in again, Copy login.
          rowActions={(a) => {
            const act = (label: string, icon: React.ReactNode, done: () => void) => (
              <Button variant="neutral" mode="ghost" size="3xsmall" leftIcon={icon} onClick={(e) => { e.stopPropagation(); done(); }}>{label}</Button>
            );
            const g = groupOf(a);
            if (a.status === "failed" && (a.fail?.retryable ?? true)) return act(ct("creator.action.retry"), <RefreshIcon className="size-3.5" />, () => toast.ok(ct("creator.bulk.retried", { n: 1 })));
            if (g === "signed_out") return act(ct("creator.action.logIn"), <RefreshIcon className="size-3.5" />, () => toast.ok(ct("creator.action.loggingIn")));
            if (a.status === "linked") return act(ct("creator.action.copyLogin"), <CopyIcon className="size-3.5" />, () => toast.ok(ct("creator.action.copiedLogin")));
            return null;
          }}
          bar={(
            <BulkBar
              n={n}
              onClear={() => setSelected(new Set())}
              actions={[
                { id: "retry", label: ct("creator.action.retry"), icon: <RefreshIcon className="size-3.5" />, onClick: () => toast.ok(ct("creator.bulk.retried", { n })) },
                { id: "verify", label: ct("creator.action.verify"), icon: <SearchIcon className="size-3.5" />, onClick: () => toast.ok(ct("creator.bulk.checking", { n })) },
                { id: "freeze", label: ct("creator.action.freeze"), icon: <LockedIcon className="size-3.5" />, onClick: () => {} },
                { id: "push", label: ct("creator.action.push"), icon: <UploadIcon className="size-3.5" />, onClick: () => {} },
              ]}
              more={[
                { label: ct("creator.action.logIn"), onSelect: () => toast.ok(ct("creator.bulk.loggingIn", { n })) },
                { label: ct("creator.action.changeProxy"), onSelect: () => setChanging(true) },
                { type: "separator" },
                // Python's three exports, each saying what it skipped.
                { label: ct("creator.export.logins"), icon: <DownloadIcon className="size-3.5" />, onSelect: () => toast.ok(ct("creator.export.doneLogins", { n: n - 1, skipped: 1 })) },
                { label: ct("creator.export.cookies"), icon: <DownloadIcon className="size-3.5" />, onSelect: () => toast.ok(ct("creator.export.doneCookies", { n: n - 1, skipped: 1 })) },
                { label: ct("creator.export.keys"), icon: <DownloadIcon className="size-3.5" />, onSelect: () => toast.ok(ct("creator.export.doneKeys", { n: 1, skipped: n - 1 })) },
                { type: "separator" },
                { label: ct("creator.action.delete"), icon: <DeleteIcon className="size-3.5" />, danger: true, onSelect: () => void confirmDelete(n) },
              ]}
            />
          )}
          peek={<AccountPeek account={open} onClose={() => setOpenId(null)} />}
        />
      </PageBody>
      <MakeDialog open={making} onClose={() => setMaking(false)} />
      <ChangeProxyDialog n={n} open={changing} onClose={() => setChanging(false)} />
    </>
  );
}
