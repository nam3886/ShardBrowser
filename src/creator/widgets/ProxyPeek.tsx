// One of our proxies beside the list: Shard's Proxy Peek copied onto our
// store. Shard's Details is data-agnostic (a ProxyEntry in, rows out) and is
// used as is; the header, Connection (last test + IP history) and the list
// of what uses it are ProxyPeek's markup with our data; "Sign-ups" is ours.
import { useState, type ReactNode } from "react";
import { Badge, Button, Label, usePresence, useLatched } from "@proxyshard/shardx-ui-kit";
import { BuildingIcon, ClockIcon, EditIcon, GlobeIcon, RefreshIcon } from "../../shared/icons";
import { CountryFlag } from "../../shared/ui/CountryFlag";
import { ProxyTestPill } from "../../shared/ui/StatusPill";
import { LoadError, LoadingRows } from "../../shared/ui/ListStates";
import { PeekBody, PeekHeader, PeekPanel, PeekSection, PropertyRow, PropertySheet, PropertyValue } from "../../shared/ui/peek";
import { fmtTs } from "../../shared/lib/utils";
import { toast } from "../../shared/model/toast";
import { useT } from "../../shared/i18n";
import { Details } from "../../widgets/ProxyPeek/Details";
import { ipTypeWord, restingNow, testState } from "../pages/ProxiesPage";
import { useCt } from "../i18n";
import type { ProxyDetail, ProxyRow } from "../types";
import { AccountPill, CopyValue, PlatformMark, useCall } from "./parts";
import { EditProxyDialog } from "./EditProxyDialog";

export function ProxyPeek({ proxy, onClose }: { proxy: ProxyRow | null; onClose: () => void }) {
  const t = useT();
  const presence = usePresence(proxy?.id ?? null, { maxExitMs: 250 });
  const p = useLatched(proxy);
  const [editing, setEditing] = useState<number | null>(null);
  if (!presence.mounted || !p) return null;
  const name = p.name || `${p.host}:${p.port}`;
  const s = testState(p);
  return (
    <PeekPanel kind="proxy" label={t("proxyPeek.label", { name })} presence={presence}>
      <PeekHeader
        subject={String(p.id)}
        title={<h2 className="m-0 min-w-0 truncate text-panel-title text-text-strong-950">{name}</h2>}
        status={s === "ok" ? <ProxyTestPill data-peek-test-status state="ok" ms={p.test!.tcp_ms!} busy={p.testing} /> : <ProxyTestPill data-peek-test-status state={s} busy={p.testing} />}
        extra={<Button variant="neutral" mode="stroke" size="3xsmall" data-peek-edit leftIcon={<EditIcon className="size-3.5" />} onClick={() => setEditing(p.id)}>{t("peek.edit")}</Button>}
        primary={(
          <Button variant="primary" mode="lighter" size="3xsmall" leftIcon={<RefreshIcon className="size-3.5" />} isLoading={p.testing} onClick={() => toast.info(t("useProxy.testingOne", { n: 1 }))}>
            {t("proxyPeek.testNow")}
          </Button>
        )}
        menuItems={[{ label: t("commands.copyAddress"), onSelect: () => {} }, { type: "separator" }, { label: t("commands.delete"), danger: true, onSelect: () => {} }]}
        menuLabel={t("pageHeader.more")}
        closeLabel={t("peek.close")}
        onClose={onClose}
      />
      <Body key={p.id} id={p.id} />
      <EditProxyDialog id={editing} onClose={() => setEditing(null)} />
    </PeekPanel>
  );
}

function Body({ id }: { id: number }) {
  const t = useT();
  const ct = useCt();
  const { data: d, error, status, reload } = useCall<ProxyDetail>("proxy", { id }, String(id));
  if (status === "loading") return <PeekBody><div className="p-inset"><LoadingRows rows={5} /></div></PeekBody>;
  if (status === "error" || !d) return <PeekBody><LoadError what={ct("creator.proxies.what")} error={error} onRetry={reload} headingLevel={3} /></PeekBody>;
  const row = (rid: string, label: string, value: ReactNode, empty = false, o: { copy?: ReactNode } = {}) => (
    <PropertyRow id={rid} label={label}>{() => <><PropertyValue empty={empty}>{value}</PropertyValue>{o.copy}</>}</PropertyRow>
  );
  const resting = restingNow(d);
  return (
    <PeekBody>
      <PeekSection id="details" title={t("proxyPeek.details")}>
        {/* Shard's Details as is. Country: the one given, else the last test's.
            Its ProxyShard signature row calls ProxyShard only for notes that
            name a ProxyShard order ("ProxyShard order 123"); ours never carry
            one (the backend clears such text from our notes). */}
        <Details proxy={{ id: String(d.id), name: d.name, kind: d.kind, host: d.host, port: d.port, username: d.username, password: d.password, country: d.country || d.test?.country_code || "", notes: d.notes }} />
      </PeekSection>
      <PeekSection id="test" title={t("proxyPeek.test")}>
        {d.test ? <Result d={d} /> : <p className="m-0 text-secondary text-text-soft-400">{t("proxyPeek.notTested")}</p>}
        <Label as="div" className="mt-1">{t("proxyPeek.history")}</Label>
        {d.history.length === 0 ? (
          <p className="m-0 text-secondary text-text-soft-400">{t("proxyInfoPopover.historyEmpty")}</p>
        ) : (
          <ul className="m-0 flex list-none flex-col p-0">
            {d.history.map((h, i) => (
              <li key={`${h.ip}-${i}`} data-history-item data-selectable className="py-2">
                <div className="flex items-center gap-2 text-body text-text-strong-950">
                  <span className="mono">{h.ip}</span>
                  {h.country_code && <CountryFlag cc={h.country_code} />}
                  {h.city && <span className="text-secondary text-text-soft-400">{h.city}</span>}
                </div>
                <div className="mt-0.5 text-secondary text-text-soft-400">
                  {h.first_seen === h.last_seen ? fmtTs(h.first_seen) : t("proxyPeek.historySpan", { from: fmtTs(h.first_seen), to: fmtTs(h.last_seen) })}
                </div>
              </li>
            ))}
          </ul>
        )}
      </PeekSection>
      <PeekSection id="creator" title={ct("creator.proxy.section")}>
        <PropertySheet>
          <div className="flex flex-col gap-[2px]">
            {row("address", ct("creator.proxy.address"), <span data-selectable className="mono truncate">{`${d.host}:${d.port}`}</span>, false, { copy: <CopyValue label={ct("creator.proxy.address")} value={`${d.host}:${d.port}`} /> })}
            {row("ip-type", ct("creator.proxyCol.ipType"), d.ip_type ? ipTypeWord(d) : ct("creator.ipType.unknown"), !d.ip_type)}
            {d.classified_by && row("found", ct("creator.proxy.foundBy"), ct(`creator.proxy.foundBy.${d.classified_by}`))}
            {row("signups", ct("creator.proxyCol.signups"), `${d.signups_ok}/${d.signups_total}`)}
            {row("streak", ct("creator.proxy.failStreak"), String(d.fail_streak))}
            {row("rest", ct("creator.proxyCol.rest"), resting ? ct("creator.proxy.restingUntil", { when: fmtTs(d.rest_until!) }) : ct("creator.proxy.notResting"), !resting)}
          </div>
        </PropertySheet>
      </PeekSection>
      <PeekSection id="accounts" title={ct("creator.proxy.using")}>
        {d.using.length === 0 ? (
          <p className="m-0 text-secondary text-text-soft-400">{ct("creator.proxy.noAccounts")}</p>
        ) : (
          <ul className="m-0 flex list-none flex-col gap-1 p-0">
            {d.using.map((a) => (
              <li key={a.id} className="flex min-w-0 items-center gap-2">
                <Button variant="neutral" mode="ghost" size="3xsmall" className="min-w-0" leftIcon={<PlatformMark k={a.platform} />} onClick={() => {}}>
                  <span className="truncate">{a.email}</span>
                </Button>
                <AccountPill a={a} />
              </li>
            ))}
          </ul>
        )}
      </PeekSection>
    </PeekBody>
  );
}

/** ProxyPeek's Result: the last test's facts, 14 px apart. */
function Result({ d }: { d: ProxyDetail }) {
  const t = useT();
  const ct = useCt();
  const snap = d.test!;
  const place = [snap.region, snap.city].filter(Boolean).join(", ") || snap.country_code;
  return (
    <div data-test-result className="flex flex-col gap-[14px] text-ui text-text-strong-950">
      {snap.ip && <Line icon={<GlobeIcon className="size-3.5" />} copy={<CopyValue label={ct("creator.proxy.exitIp")} value={snap.ip} />}><span className="mono">{snap.ip}</span></Line>}
      {place && <Line icon={snap.country_code ? <CountryFlag cc={snap.country_code} height={14} /> : <GlobeIcon className="size-3.5" />}>{place}</Line>}
      {snap.timezone && <Line icon={<ClockIcon className="size-3.5" />}>{snap.timezone}</Line>}
      {snap.isp && <Line icon={<BuildingIcon className="size-3.5" />}>{snap.isp}</Line>}
      <div className="flex flex-wrap items-center gap-2">
        <Badge color={snap.ok ? "success" : "error"} size="small">{snap.tcp_ms != null ? t("proxyPeek.tcpOk", { ms: snap.tcp_ms }) : t("proxyPeek.tcpFailed")}</Badge>
        <span className="text-secondary text-text-soft-400">{fmtTs(snap.at)}</span>
      </div>
      {snap.error && <p className="m-0 text-secondary text-error-text"><span className="block text-secondary text-text-soft-400" data-selectable>{snap.error}</span></p>}
    </div>
  );
}

function Line({ icon, children, copy }: { icon: ReactNode; children: ReactNode; copy?: ReactNode }) {
  return (
    <div className="flex items-center gap-2.5">
      <span className="inline-grid w-3.5 place-items-center text-icon-soft-400">{icon}</span>
      <span data-selectable className="min-w-0">{children}</span>
      {copy}
    </div>
  );
}
