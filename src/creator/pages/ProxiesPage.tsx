// Our sign-up proxies, on our own store: Shard's Proxies page copied, not
// extended (owner: Shard's page and data stay exactly as upstream). Same
// header, columns, bulk bar, Add proxies flow and Peek; Shard's words where
// the thing is Shard's, ours for what sign-ups add (IP type, Resting). A
// failing proxy shows as Shard's "Problems to fix" card with Retest; nothing
// moves accounts off it (Accounts' Change proxy does that by hand).
import { useState } from "react";
import { Badge, Button, EmptyState, Switch, Tooltip } from "@proxyshard/shardx-ui-kit";
import { HeaderMenu, PageBody, PageHeader } from "../../widgets/PageHeader";
import { AddIcon, CopyIcon, DeleteIcon, PauseIcon, RefreshIcon, SearchIcon } from "../../shared/icons";
import { CountryFlag } from "../../shared/ui/CountryFlag";
import { ProtocolPill } from "../../shared/ui/ProtocolPill";
import { ProxyTestPill } from "../../shared/ui/StatusPill";
import type { ListColumnDef } from "../../shared/ui/listHeader";
import { fmtTs } from "../../shared/lib/utils";
import { toast } from "../../shared/model/toast";
import { confirmModal } from "../../shared/model/confirm";
import { t as tr, useT } from "../../shared/i18n";
import { call } from "../api";
import { runPool } from "../pool";
import { ct, useCt } from "../i18n";
import type { IpType, ProxyList, ProxyRow } from "../types";
import { AddProxiesDialog } from "../widgets/AddProxiesDialog";
import { BulkBar } from "../widgets/BulkBar";
import { card, CardSection } from "../widgets/CardSection";
import { DisplayMenu } from "../widgets/Display";
import { AppliedFilters, FilterButton, type FilterField } from "../widgets/Filters";
import { ListPage } from "../widgets/ListPage";
import { ProxyPeek } from "../widgets/ProxyPeek";
import { useWindowed } from "../widgets/parts";

const KIND = { socks5: "SOCKS5", http: "HTTP", https: "HTTPS" } as const;
const IP_TYPES: IpType[] = ["residential", "datacenter", "mobile"];
/** Fixed parts beside a cell's text, px: a flag and its gap; a pill's sides. */
const FLAG = 29;
const PILL = 16;

export const testState = (p: ProxyRow) => (!p.test ? "untested" : p.test.ok ? "ok" : "failed") as "ok" | "failed" | "untested";
export const restingNow = (p: Pick<ProxyRow, "rest_until">) => !!p.rest_until && Date.parse(p.rest_until) > Date.now();
export const ipTypeWord = (p: Pick<ProxyRow, "ip_type" | "rotating">) =>
  p.ip_type ? ct(`creator.ipType.${p.ip_type}`) + (p.rotating ? `, ${ct("creator.ipType.rotating")}` : "") : "";
const cityOf = (p: ProxyRow) => p.test?.city || p.test?.country_code || p.country;

type Col = "name" | "type" | "address" | "country" | "accounts" | "test" | "ip_type" | "signups" | "rest" | "on";
const COLUMNS: readonly ListColumnDef<Col, ProxyRow>[] = [
  { id: "name", title: (t) => t("proxyTable.colName"), need: 160, min: 120, max: 360, shown: true, hideable: false, movable: false, text: (p) => p.name || `${p.host}:${p.port}` },
  {
    id: "type", title: (t) => t("proxyTable.colType"), need: 72, min: 64, max: 120, shown: true, hideable: true, movable: true,
    fit: (p, m) => PILL + m(KIND[p.kind], "medium"),
    cell: (p) => <ProtocolPill kind={p.kind} />,
  },
  { id: "address", title: (t) => t("proxyTable.colHostPort"), need: 170, min: 120, max: 320, shown: true, hideable: true, movable: true, role: "mono", text: (p) => `${p.host}:${p.port}` },
  {
    id: "country", title: (t) => t("proxyTable.colCountry"), need: 120, min: 72, max: 240, shown: true, hideable: true, movable: true,
    fit: (p, m) => (cityOf(p) ? FLAG + m(cityOf(p), "secondary") : 16),
    cell: (p) => {
      const cc = p.test?.country_code || p.country;
      return (
        <span className="flex min-w-0 items-center gap-1.5 text-secondary text-text-sub-600">
          {cc ? <><CountryFlag cc={cc} /><Tooltip content={cityOf(p)} whenTruncated warm={false}><span className="truncate">{cityOf(p)}</span></Tooltip></> : <span className="text-text-soft-400">—</span>}
        </span>
      );
    },
  },
  { id: "accounts", title: () => ct("creator.col.accounts"), need: 72, min: 56, max: 160, shown: true, hideable: true, movable: true, role: "tabular", text: (p) => String(p.accounts) },
  {
    id: "test", title: (t) => t("proxyTable.colTestResult"), need: 120, min: 96, max: 320, shown: true, hideable: true, movable: true,
    fit: (p, m) => PILL + 70 + (p.test?.ok && p.test.ip ? 6 + m(p.test.ip, "mono") : 0),
    // Shard's Test result cell: the pill, then the exit IP while there is room.
    cell: (p) => {
      const s = testState(p);
      const detail = p.test ? (p.test.error ? `${fmtTs(p.test.at)}, ${p.test.error}` : fmtTs(p.test.at)) : undefined;
      return (
        <span data-status={p.testing ? "testing" : s} className="flex min-w-0 items-center gap-1.5">
          <Tooltip content={detail}>
            {s === "ok" ? <ProxyTestPill state="ok" ms={p.test!.tcp_ms!} busy={p.testing} detail={detail} className="shrink-0" /> : <ProxyTestPill state={s} busy={p.testing} detail={detail} className="shrink-0" />}
          </Tooltip>
          {s === "ok" && !p.testing && p.test?.ip && <span className="mono min-w-0 truncate text-secondary text-text-sub-600">{p.test.ip}</span>}
        </span>
      );
    },
  },
  {
    id: "ip_type", title: () => ct("creator.proxyCol.ipType"), need: 110, min: 90, max: 200, shown: true, hideable: true, movable: true,
    fit: (p, m) => (p.ip_type ? 10 + m(ipTypeWord(p), "secondary") : 16),
    // The protocol chip's neutral look: a fact about the proxy, not a state.
    cell: (p) => (p.ip_type
      ? <Tooltip content={ipTypeWord(p)} whenTruncated><Badge color="gray" size="small" className="min-w-0 max-w-full"><span className="truncate">{ipTypeWord(p)}</span></Badge></Tooltip>
      : <span className="text-text-soft-400">—</span>),
  },
  { id: "signups", title: () => ct("creator.proxyCol.signups"), need: 72, min: 64, max: 160, shown: false, hideable: true, movable: true, role: "tabular", text: (p) => `${p.signups_ok}/${p.signups_total}` },
  {
    id: "rest", title: () => ct("creator.proxyCol.rest"), need: 120, min: 80, max: 220, shown: false, hideable: true, movable: true, role: "secondary",
    text: (p) => (restingNow(p) ? ct("creator.proxy.restingUntil", { when: fmtTs(p.rest_until!) }) : "—"),
  },
  {
    // Python's per-proxy "On": the kit Switch, as an Extensions card turns one on or off.
    id: "on", title: () => ct("creator.proxyCol.on"), need: 52, min: 52, max: 80, shown: true, hideable: true, movable: true,
    cell: (p) => (
      <span onClick={(e) => e.stopPropagation()} className="flex items-center">
        <Switch checked={p.enabled} aria-label={ct("creator.proxy.use", { name: p.name || `${p.host}:${p.port}` })} onChange={() => { toast.ok(p.enabled ? ct("creator.proxies.turnedOffOne") : ct("creator.proxies.turnedOnOne")); }} />
      </span>
    ),
  },
];

/** Deleting proxies asks first: their browsers move to a working proxy, or nothing is deleted. */
export const confirmDeleteProxies = (n: number) => confirmModal({
  title: n === 1 ? ct("creator.confirm.deleteProxyOne") : ct("creator.confirm.deleteProxies", { n }),
  message: ct("creator.confirm.deleteProxiesBody"),
  confirmLabel: ct("creator.action.delete"),
  danger: true,
});

/** Shard's bulk test exactly (useProxy.bulkTest): its words, a pool of 5, one summary. */
const testProxies = async (ids: string[]) => {
  toast.info(ids.length === 1 ? tr("useProxy.testingOne", { n: 1 }) : tr("useProxy.testingMany", { n: ids.length }));
  const got = await runPool(ids, 5, (id) => call<boolean>("test_proxy", { id }).catch(() => false));
  const ok = got.filter(Boolean).length;
  const summary = tr("useProxy.testedSummary", { n: ids.length, ok, failed: ids.length - ok });
  if (ok === ids.length) toast.ok(summary); else toast.info(summary);
};

export function ProxiesPage() {
  const t = useT();
  const ct = useCt();
  const [status, setStatus] = useState<string | null>(null);
  const [kind, setKind] = useState<string | null>(null);
  const [ipType, setIpType] = useState<string | null>(null);
  const [resting, setResting] = useState<string | null>(null);
  const [rotation, setRotation] = useState<string | null>(null);
  const [hidden, setHidden] = useState<number[]>([]);
  const [q, setQ] = useState("");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [openId, setOpenId] = useState<number | null>(null);
  const [adding, setAdding] = useState(false);
  const [order, setOrder] = useState("added");
  const [layoutRev, setLayoutRev] = useState(0);
  const list = useWindowed<ProxyList>("proxies", { status, kind, ip_type: ipType, resting, rotation, q, order });
  const data = list.data;
  const c = data?.counts;
  const filtered = !!(status || kind || ipType || resting || rotation);
  const nothing = list.status === "ready" && c?.all === 0 && !q.trim() && !filtered;
  // Select all means every match, not only the rows loaded so far (Shard's
  // lists hold every row; ours load in windows). ponytail: the mock counts
  // the matches; the real select-all asks the backend for their ids (M7).
  const n = selected.size > 0 && selected.size === list.items.length && data ? data.total : selected.size;
  const pick = (set: (v: string | null) => void) => (v: string | null) => { set(v); setSelected(new Set()); };
  const fields: FilterField[] = [
    { id: "status", label: t("profileFilterBar.statusTitle"), current: status, set: pick(setStatus), values: [
      { value: "working", label: t("proxyFilters.statusWorking"), count: c?.working },
      { value: "failed", label: t("proxyFilters.statusFailed"), count: c?.failed },
      { value: "untested", label: t("proxyFilters.statusUntested"), count: c?.untested },
      { value: "off", label: ct("creator.proxy.off"), count: c?.off },
    ] },
    { id: "kind", label: t("proxyFilters.kindTitle"), current: kind, set: pick(setKind), values: (["socks5", "http", "https"] as const).map((k) => ({ value: k, label: KIND[k] })) },
    { id: "ip_type", label: ct("creator.proxyCol.ipType"), current: ipType, set: pick(setIpType), values: IP_TYPES.map((v) => ({ value: v, label: ct(`creator.ipType.${v}`), count: c?.[v] })) },
    { id: "rotation", label: ct("creator.proxy.rotation"), current: rotation, set: pick(setRotation), values: [
      { value: "static", label: ct("creator.proxy.static"), count: c?.static },
      { value: "rotating", label: ct("creator.proxy.rotatingValue"), count: c?.rotating },
    ] },
    { id: "rest", label: ct("creator.proxyCol.rest"), current: resting, set: pick(setResting), values: [
      { value: "resting", label: ct("creator.proxy.resting"), count: c?.resting },
      { value: "in_use", label: ct("creator.proxy.notResting"), count: c ? c.all - c.resting : undefined },
    ] },
  ];
  const failing = (data?.failing ?? []).filter((x) => !hidden.includes(x.id));
  const failingCards = failing.length === 0 ? [] : [card("proxy-failing", {
    label: t("proxyFilters.statusFailed"),
    pill: { color: "error", text: failing[0].accounts === 1 ? ct("creator.needs.accountsOne") : ct("creator.needs.accounts", { n: failing[0].accounts }) },
    what: t("overview.failedProxy", { proxy: failing[0].name, why: failing[0].error }),
    why: ct("creator.proxies.failingWhy"),
    detail: fmtTs(failing[0].tested_at),
    fix: { id: "retest", label: failing.length === 1 ? t("overview.retestProxy") : t("overview.retestProxies"), run: () => testProxies(failing.map((x) => String(x.id))) },
    more: failing.length > 1 ? t("overview.more", { n: failing.length - 1 }) : null,
    show: () => pick(setStatus)("failed"),
    hide: () => setHidden((h) => [...h, ...failing.map((x) => x.id)]),
  })];
  const empty = q.trim()
    ? <EmptyState icon={<SearchIcon />} title={t("proxyTable.noMatchTitle", { q: q.trim() })} actions={<Button variant="neutral" mode="stroke" size="xsmall" onClick={() => setQ("")}>{ct("creator.list.clearSearch")}</Button>} />
    : filtered
      ? <EmptyState icon={<SearchIcon />} title={t("proxyTable.filtersNoMatchTitle")} actions={<Button variant="neutral" mode="stroke" size="xsmall" onClick={() => { setStatus(null); setKind(null); setIpType(null); setResting(null); setRotation(null); }}>{ct("creator.list.clearFilters")}</Button>} />
      : <EmptyState icon={<AddIcon />} title={t("proxyTable.emptyTitle")} hint={ct("creator.proxies.emptyHint")} action={{ label: t("proxyTable.emptyAction"), icon: <AddIcon />, onClick: () => setAdding(true) }} />;

  return (
    <>
      <PageHeader
        title={ct("creator.nav.proxies")}
        count={data && !nothing ? (q.trim() || filtered ? { shown: data.total, total: c!.all } : c!.all) : undefined}
        help={{ title: ct("creator.nav.proxies"), body: ct("creator.proxies.helpBody") }}
        filter={nothing ? undefined : { value: q, onChange: (v) => { setQ(v); setSelected(new Set()); } }}
        actions={(
          <div className="flex flex-none items-center gap-2">
            {!nothing && <FilterButton fields={fields} />}
            {!nothing && (
              <DisplayMenu
                storageKey="creator:proxies.layout"
                defs={COLUMNS}
                onChanged={() => setLayoutRev((x) => x + 1)}
                order={{ value: order, initial: "added", onChange: setOrder, options: [
                  { value: "added", label: t("profileFilterBar.orderAdded") },
                  { value: "name", label: t("proxyDisplay.orderName") },
                  { value: "latency", label: t("proxyDisplay.orderLatency") },
                  { value: "accounts", label: t("proxyDisplay.orderProfiles") },
                ] }}
              />
            )}
            <HeaderMenu items={[
              { label: ct("creator.proxies.testAll"), icon: <RefreshIcon className="size-3.5" />, onSelect: () => void testProxies(list.items.map((p) => String(p.id))) },
              { label: ct("creator.proxies.copyAll"), icon: <CopyIcon className="size-3.5" />, onSelect: () => {} },
            ]} />
            <Button variant="primary" mode="filled" size="xsmall" leftIcon={<AddIcon className="size-3.5" />} onClick={() => setAdding(true)}>
              {t("proxyTable.emptyAction")}
            </Button>
          </div>
        )}
      />
      <PageBody scroll={false} className="relative">
        <ListPage<Col, ProxyRow>
          key={layoutRev}
          label={ct("creator.nav.proxies")}
          above={(
            <>
              {/* Failing proxies, as Shard's overview card for one (its words, Retest, Show, dismiss). */}
              <CardSection id="failing" label={ct("creator.cards.needs")} cards={failingCards} />
              <AppliedFilters fields={fields} />
            </>
          )}
          layoutKey="creator:proxies.layout"
          defs={COLUMNS}
          items={list.items}
          onEnd={list.more}
          status={list.status}
          error={list.error}
          onRetry={list.reload}
          what={ct("creator.proxies.what")}
          name={(p) => p.name || `${p.host}:${p.port}`}
          empty={empty}
          selected={selected}
          onSelected={setSelected}
          onOpen={(p) => setOpenId(p.id)}
          openId={openId}
          tail={40}
          bar={(
            <BulkBar
              n={n}
              onClear={() => setSelected(new Set())}
              actions={[
                { id: "test", label: t("commands.test"), icon: <RefreshIcon className="size-3.5" />, onClick: () => void testProxies([...selected]) },
                { id: "off", label: ct("creator.proxies.turnOff"), icon: <PauseIcon className="size-3.5" />, onClick: () => toast.ok(ct("creator.proxies.turnedOff", { n })) },
                { id: "copy", label: t("commands.export"), icon: <CopyIcon className="size-3.5" />, onClick: () => toast.ok(ct("creator.proxies.copied", { n })) },
                { id: "delete", label: t("commands.delete"), icon: <DeleteIcon className="size-3.5" />, danger: true, onClick: () => void confirmDeleteProxies(n) },
              ]}
            />
          )}
          peek={<ProxyPeek proxy={list.items.find((p) => p.id === openId) ?? null} onClose={() => setOpenId(null)} />}
        />
      </PageBody>
      <AddProxiesDialog open={adding} onClose={() => setAdding(false)} />
    </>
  );
}
