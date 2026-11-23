import { useEffect, useRef, useState } from "react";
// One mailbox beside the Emails list (the Python email drawer), on Shard's
// Peek shell as Proxy Peek fills it: details as property rows, the accounts
// on it as Proxy Peek lists "Profiles using it", its mail as Proxy Peek
// lists the IP history (a code to copy in Shard's CopyField).
import { Badge, Button, usePresence, useLatched } from "@proxyshard/shardx-ui-kit";
import { RefreshIcon } from "../../shared/icons";
import { LoadError, LoadingRows } from "../../shared/ui/ListStates";
import { PeekBody, PeekHeader, PeekPanel, PeekSection, PropertyRow, PropertySheet, PropertyValue } from "../../shared/ui/peek";
import { fmtTs } from "../../shared/lib/utils";
import { call, MOCK } from "../api";
import { useCt } from "../i18n";
import type { EmailDetail, EmailRow, MailMessage } from "../types";
import { AccountPill, CopyValue, PlatformMark, platformLabel, ProfileLink, useCall } from "./parts";

export function EmailPeek({ email, onClose }: { email: EmailRow | null; onClose: () => void }) {
  const ct = useCt();
  const presence = usePresence(email?.id ?? null, { maxExitMs: 250 });
  const m = useLatched(email);
  // Refresh reads the inbox again (the `email` call polls it first).
  const [rev, setRev] = useState(0);
  // The badge follows what the Peek's own read found (a refresh can find it gone).
  const [read, setRead] = useState<{ id: number; status: EmailRow["status"] } | null>(null);
  if (!presence.mounted || !m) return null;
  const status = read?.id === m.id ? read.status : m.status;
  return (
    <PeekPanel kind="profile" label={ct("creator.emailPeek.label", { email: m.email })} presence={presence}>
      <PeekHeader
        subject={String(m.id)}
        title={<h2 className="m-0 min-w-0 truncate text-panel-title text-text-strong-950">{m.email}</h2>}
        status={<Badge color={status === "gone" ? "error" : "gray"} size="small">{ct(`creator.emailState.${status}`)}</Badge>}
        primary={(
          <Button variant="primary" mode="lighter" size="3xsmall" leftIcon={<RefreshIcon className="size-3.5" />} onClick={() => setRev((r) => r + 1)}>
            {ct("creator.emailPeek.refresh")}
          </Button>
        )}
        menuItems={MOCK ? [{ label: ct("creator.action.export"), onSelect: () => {} }, { type: "separator" }, { label: ct("creator.action.delete"), danger: true, onSelect: () => {} }] : undefined}
        menuLabel={ct("creator.peek.more")}
        closeLabel={ct("creator.peek.close")}
        onClose={onClose}
      />
      <Body key={`${m.id}:${rev}`} id={m.id} rev={rev} onRead={setRead} />
    </PeekPanel>
  );
}

function Body({ id, rev, onRead }: { id: number; rev: number; onRead: (r: { id: number; status: EmailRow["status"] }) => void }) {
  const ct = useCt();
  const { data, error, status, reload } = useCall<EmailDetail>("email", { id }, `${id}:${rev}`);
  useEffect(() => { if (data) onRead({ id: data.id, status: data.status }); }, [data, onRead]);
  if (status === "loading") return <PeekBody><div className="p-inset"><LoadingRows rows={5} /></div></PeekBody>;
  if (status === "error" || !data) return <PeekBody><LoadError what={ct("creator.emails.what")} error={error} onRetry={reload} headingLevel={3} /></PeekBody>;
  const row = (rid: string, label: string, value: React.ReactNode, empty = false, o: { copy?: React.ReactNode } = {}) => (
    <PropertyRow id={rid} label={label}>{() => <><PropertyValue empty={empty}>{value}</PropertyValue>{o.copy}</>}</PropertyRow>
  );
  return (
    <PeekBody>
      <PeekSection id="details" title={ct("creator.peek.details")}>
        <PropertySheet>
          <div className="flex flex-col gap-[2px]">
            {row("email", ct("creator.col.email"), <span data-selectable className="truncate">{data.email}</span>, false, { copy: <CopyValue label={ct("creator.col.email")} value={data.email} /> })}
            {row("type", ct("creator.col.type"), <><PlatformMark k={data.provider} /><span className="truncate">{ct(`creator.mailbox.${data.provider}`)}</span></>)}
            {row("created", ct("creator.peek.created"), fmtTs(data.created_at))}
            {row("proxy", ct("creator.col.proxy"), <span data-selectable className="mono truncate">{data.technical.proxy ?? ct("creator.proxy.direct")}</span>, !data.technical.proxy, {
              copy: data.technical.proxy ? <CopyValue label={ct("creator.col.proxy")} value={data.technical.proxy} /> : undefined })}
            {data.technical.profile
              ? <PropertyRow id="profile" label={ct("creator.peek.profile")}>{() => <ProfileLink id={data.technical.profile!} name={data.email} />}</PropertyRow>
              : row("profile", ct("creator.peek.profile"), ct("creator.peek.noProfile"), true)}
          </div>
        </PropertySheet>
      </PeekSection>
      <PeekSection id="accounts" title={ct("creator.emailPeek.accounts")}>
        {data.accounts.length === 0 ? (
          <p className="m-0 text-secondary text-text-soft-400">{ct("creator.emailPeek.noAccounts")}</p>
        ) : (
          <ul className="m-0 flex list-none flex-col gap-1 p-0">
            {data.accounts.map((a) => (
              <li key={a.platform} className="flex items-center gap-2">
                <Button variant="neutral" mode="ghost" size="3xsmall" leftIcon={<PlatformMark k={a.platform} />} onClick={() => {}}>{platformLabel(a.platform)}</Button>
                <AccountPill a={a} />
              </li>
            ))}
          </ul>
        )}
      </PeekSection>
      <PeekSection id="mail" title={ct("creator.emailPeek.mail", { n: data.items.length })}>
        {/* A gone mailbox says why no new mail can come, as Proxy Peek says why a test failed. */}
        {data.status === "gone" && (
          <p data-mailbox-gone className="m-0 mb-2 text-secondary text-error-text">
            {ct("creator.emailPeek.goneTitle")}
            <span className="block text-secondary text-text-soft-400">{ct(`creator.emailPeek.gone.${data.provider}`)}</span>
          </p>
        )}
        {data.items.length === 0 ? (
          <p className="m-0 text-secondary text-text-soft-400">{ct("creator.emailPeek.noMail")}</p>
        ) : (
          <MailList id={data.id} first={data.items} more={data.more} />
        )}
      </PeekSection>
    </PeekBody>
  );
}

/** The mail, newest first; older mail loads as its end scrolls into view
 *  (Shard's lists: no pages, no "Load older" button), 50 at a time, with
 *  Shard's placeholder rows while it comes. */
function MailList({ id, first, more: firstMore }: { id: number; first: MailMessage[]; more: boolean }) {
  const ct = useCt();
  const [items, setItems] = useState(first);
  const [more, setMore] = useState(firstMore);
  const [busy, setBusy] = useState(false);
  const end = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const el = end.current;
    if (!el || !more) return;
    const io = new IntersectionObserver(([e]) => {
      if (!e.isIntersecting || busy) return;
      setBusy(true);
      // Older than the last one shown (a cursor: new mail on top never shifts it).
      void call<{ items: MailMessage[]; more: boolean }>("email_mail", { id, before_id: items[items.length - 1]?.id })
        .then((r) => { setItems((x) => [...x, ...r.items]); setMore(r.more); }, () => setMore(false))
        .finally(() => setBusy(false));
    });
    io.observe(el);
    return () => io.disconnect();
  }, [id, items.length, more, busy]);
  return (
    <>
          <ul data-mail-list className="m-0 flex list-none flex-col p-0">
            {items.map((mail) => (
              <li key={mail.id} data-mail={mail.id} data-selectable className="py-2">
                <div className="text-body text-text-strong-950">{mail.subject}</div>
                <div className="mt-0.5 truncate text-secondary text-text-soft-400">{fmtTs(mail.received_at)}, {mail.sender}</div>
                {mail.code && (
                  <div className="mt-1.5 flex items-center gap-2">
                    <span data-selectable className="mono text-body text-text-strong-950">{mail.code}</span>
                    <CopyValue label={ct("creator.emailPeek.code")} value={mail.code} />
                  </div>
                )}
              </li>
            ))}
          </ul>
          {more && <div ref={end}>{busy && <LoadingRows rows={2} />}</div>}
    </>
  );
}
