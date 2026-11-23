// One account beside the list, on Shard's Peek shell (ProxyPeek's recipe):
// the header carries the state and the next action, the body its error, its
// details with the secrets to copy, its browser and its log.
import { useEffect, useState } from "react";
import { Button, Modal, usePresence, useLatched } from "@proxyshard/shardx-ui-kit";
import { NavBrowsersIcon, RefreshIcon } from "../../shared/icons";
import { CountryFlag } from "../../shared/ui/CountryFlag";
import { PeekBody, PeekHeader, PeekPanel, PeekSection, PropertyRow, PropertySheet, PropertyValue } from "../../shared/ui/peek";
import { fmtTs } from "../../shared/lib/utils";
import { toast } from "../../shared/model/toast";
import { confirmModal } from "../../shared/model/confirm";
import { call, MOCK } from "../api";
import { useCt } from "../i18n";
import type { AccountRow, PlatformCard } from "../types";
import { EventList } from "./EventList";
import { AccountPill, confirmDelete, CopyValue, failText, PlatformMark, platformLabel, ProfileLink } from "./parts";

export function AccountPeek({ account, onClose }: { account: AccountRow | null; onClose: () => void }) {
  const ct = useCt();
  const presence = usePresence(account?.id ?? null, { maxExitMs: 250 });
  const a = useLatched(account);
  // Whether the platform keeps an API key (the key itself is read only on Copy).
  const [hasKey, setHasKey] = useState(false);
  useEffect(() => {
    setHasKey(false);
    if (account?.status === "linked") void call<PlatformCard[]>("platforms").then((ps) => setHasKey(!!ps.find((p) => p.key === account.platform)?.secrets.includes("api_key")), () => {});
  }, [account]);
  if (!presence.mounted || !a) return null;
  const retryable = a.status === "failed" && (a.fail?.retryable ?? true);
  const row = (id: string, label: string, value: React.ReactNode, empty = false, o: { copy?: React.ReactNode } = {}) => (
    <PropertyRow id={id} label={label}>{() => <><PropertyValue empty={empty}>{value}</PropertyValue>{o.copy}</>}</PropertyRow>
  );
  const secret = (key: "password" | "api_key") => () => call<string>("account_secret", { id: a.id, key });
  const dots = <span aria-hidden="true" className="truncate">••••••••</span>;

  const events = (
    <PeekSection id="events" title={ct("creator.peek.events")}>
      <EventList key={a.id} accountId={a.id} live={a.busy} />
    </PeekSection>
  );

  return (
    <PeekPanel kind="profile" label={ct("creator.peek.label", { email: a.email })} presence={presence}>
      <PeekHeader
        subject={String(a.id)}
        title={<h2 className="m-0 min-w-0 truncate text-panel-title text-text-strong-950">{a.email}</h2>}
        status={<AccountPill a={a} />}
        // Open browser, Retry, Log in again, Check and the More menu are mock-only until their backend comes (M5-M7).
        extra={MOCK && a.browser_id != null && (
          <Button variant="neutral" mode="stroke" size="3xsmall" leftIcon={<NavBrowsersIcon className="size-3.5" />} onClick={() => toast.info(ct("creator.peek.opening"))}>
            {ct("creator.peek.openBrowser")}
          </Button>
        )}
        primary={!MOCK ? undefined : a.status === "linked" && (a.verify_status === "signed_out" || a.verify_status === "expired") ? (
          <Button variant="primary" mode="lighter" size="3xsmall" leftIcon={<RefreshIcon className="size-3.5" />} onClick={() => toast.ok(ct("creator.action.loggingIn"))}>
            {ct("creator.action.logIn")}
          </Button>
        ) : retryable ? (
          <Button variant="primary" mode="lighter" size="3xsmall" leftIcon={<RefreshIcon className="size-3.5" />} onClick={() => toast.ok(ct("creator.bulk.retried", { n: 1 }))}>
            {ct("creator.action.retry")}
          </Button>
        ) : a.status === "linked" ? (
          <Button variant="primary" mode="lighter" size="3xsmall" leftIcon={<RefreshIcon className="size-3.5" />} onClick={() => toast.ok(ct("creator.bulk.checking", { n: 1 }))}>
            {ct("creator.action.verify")}
          </Button>
        ) : undefined}
        menuItems={!MOCK ? undefined : [
          {
            label: a.frozen ? ct("creator.action.unfreeze") : ct("creator.action.freeze"),
            // A pushed account in use here may sign out the relay's copy: Unfreeze asks first.
            onSelect: () => (a.frozen && a.relay_id
              ? void confirmModal({ title: ct("creator.confirm.unfreezeTitle"), message: ct("creator.confirm.unfreezeBody"), confirmLabel: ct("creator.action.unfreeze"), danger: true })
                .then((yes) => { if (yes) toast.ok(ct("creator.bulk.unfrozen", { n: 1 })); })
              : toast.ok(a.frozen ? ct("creator.bulk.unfrozen", { n: 1 }) : ct("creator.bulk.frozen", { n: 1 }))),
          },
          { label: ct("creator.action.push"), disabled: a.status !== "linked", onSelect: () => {} },
          // Python's email drawer: cookies and proxy to paste into another browser.
          { label: ct("creator.action.copyCookies"), disabled: a.status !== "linked", onSelect: () => toast.ok(ct("creator.action.copiedCookies", { n: 14 })) },
          { type: "separator" },
          { label: ct("creator.action.delete"), danger: true, onSelect: () => void confirmDelete(1) },
        ]}
        menuLabel={ct("creator.peek.more")}
        closeLabel={ct("creator.peek.close")}
        onClose={onClose}
      />
      <PeekBody>
        {a.status === "failed" && (
          <PeekSection id="error" title={ct("creator.peek.error")}>
            {/* As Proxy Peek says why a test failed: the word in the error
                text, the backend's own words under it, selectable. */}
            <p className="m-0 text-secondary text-error-text">
              {a.fail ? failText(a.fail.code) : ct("creator.peek.errorUnknown")}
              <span className="block text-secondary text-text-soft-400" data-selectable>{a.error}{a.fail ? ` (${a.fail.code})` : ""}</span>
            </p>
          </PeekSection>
        )}
        {/* The page as the sign-up last saw it, in Shard's live-view frame (LiveView); a click shows it large in the kit's Modal. */}
        {a.status === "failed" && <Screenshot key={a.id} id={a.id} />}
        {/* A failed account's log first: it says what went wrong. */}
        {a.status === "failed" && events}
        <PeekSection id="details" title={ct("creator.peek.details")}>
          <PropertySheet>
            <div className="flex flex-col gap-[2px]">
              {row("service", ct("creator.col.service"), <><PlatformMark k={a.platform} /><span className="truncate">{platformLabel(a.platform)}</span></>)}
              {row("route", ct("creator.col.route"), a.route === "email" ? ct("creator.route.email") : ct("creator.make.routeVia", { name: platformLabel(a.route) }))}
              {row("mailbox", ct("creator.peek.mailbox"), <><PlatformMark k={a.mailbox_type} /><span className="truncate">{ct(`creator.mailbox.${a.mailbox_type}`)}</span></>)}
              {row("created", ct("creator.peek.created"), fmtTs(a.created_at))}
              {a.step && row("step", ct("creator.peek.step"), a.step)}
              {row("id", ct("creator.peek.accountId"), <span data-selectable className="mono truncate">{a.id}</span>, false, { copy: <CopyValue label={ct("creator.peek.accountId")} value={String(a.id)} /> })}
              {/* Login, password and key: Shard's copy button; secrets are read only when Copy is pressed. */}
              {a.status === "linked" && row("login", ct("creator.peek.login"), <span data-selectable className="truncate">{a.email}</span>, false, { copy: <CopyValue label={ct("creator.peek.login")} value={a.email} /> })}
              {a.status === "linked" && row("password", ct("creator.peek.password"), dots, false, { copy: <CopyValue label={ct("creator.peek.password")} value={secret("password")} secret /> })}
              {a.status === "linked" && hasKey && row("api-key", ct("creator.peek.apiKey"), dots, false, { copy: <CopyValue label={ct("creator.peek.apiKey")} value={secret("api_key")} secret /> })}
            </div>
          </PropertySheet>
        </PeekSection>
        <PeekSection id="browser" title={ct("creator.peek.browser")}>
          <PropertySheet>
            <div className="flex flex-col gap-[2px]">
              {row("proxy", ct("creator.col.proxy"), a.proxy ? <>{a.proxy_cc && <CountryFlag cc={a.proxy_cc} height={14} />}<span data-selectable className="mono truncate">{a.proxy}</span></> : ct("creator.proxy.direct"), !a.proxy, {
                copy: a.proxy ? <CopyValue label={ct("creator.col.proxy")} value={a.proxy} /> : undefined })}
              {a.profile_id
                ? <PropertyRow id="profile" label={ct("creator.peek.profile")}>{() => <ProfileLink id={a.profile_id!} name={a.email} />}</PropertyRow>
                : row("profile", ct("creator.peek.profile"), ct("creator.peek.noProfile"), true)}
            </div>
          </PropertySheet>
        </PeekSection>
        {a.status !== "failed" && events}
      </PeekBody>
    </PeekPanel>
  );
}

function Screenshot({ id }: { id: number }) {
  const ct = useCt();
  const [src, setSrc] = useState<string | null | undefined>(undefined);
  const [big, setBig] = useState(false);
  useEffect(() => { void call<string | null>("account_screenshot", { id }).then(setSrc, () => setSrc(null)); }, [id]);
  return (
    <PeekSection id="screenshot" title={ct("creator.peek.screenshot")}>
      {src === undefined ? null : !src ? (
        <p className="m-0 text-secondary text-text-soft-400">{ct("creator.peek.noScreenshot")}</p>
      ) : (
        <>
          <Button variant="neutral" mode="ghost" aria-label={ct("creator.peek.screenshotOpen")} onClick={() => setBig(true)}
            className="block h-auto w-full cursor-zoom-in overflow-hidden rounded-control bg-bg-weak-50 p-0 ring-1 ring-inset ring-stroke-soft-200">
            <img src={src} alt="" className="block aspect-[16/10] w-full object-contain" />
          </Button>
          <Modal open={big} onClose={() => setBig(false)} title={ct("creator.peek.screenshot")} size="lg">
            <div className="overflow-hidden rounded-control bg-bg-weak-50 ring-1 ring-inset ring-stroke-soft-200">
              <img src={src} alt={ct("creator.peek.screenshot")} className="block w-full object-contain" />
            </div>
          </Modal>
        </>
      )}
    </PeekSection>
  );
}
