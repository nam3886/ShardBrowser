// Edit one of our proxies: Shard's Edit proxy dialog
// (features/manage-proxies/ui/EditProxyDialog) copied onto our store, its
// fields, words and footer notes. Not copied: the change-IP link (Shard's own
// rotation feature) and its leave guard (a later pass, with the real store).
import { useState } from "react";
import { DialogModal, IconButton, Input } from "@proxyshard/shardx-ui-kit";
import { EyeIcon, EyeOffIcon } from "../../shared/icons";
import { Field } from "../../shared/ui/Field";
import { NumField } from "../../shared/ui/NumField";
import { SelectField } from "../../shared/ui/SelectField";
import { toast } from "../../shared/model/toast";
import { useT } from "../../shared/i18n";
import { useCt } from "../i18n";
import type { ProxyDetail } from "../types";
import { useCall } from "./parts";

type Draft = Pick<ProxyDetail, "name" | "kind" | "host" | "port" | "username" | "password" | "country" | "notes">;

/** Opens once the proxy (with its password) is read. */
export function EditProxyDialog({ id, onClose }: { id: number | null; onClose: () => void }) {
  if (id == null) return null;
  return <Loaded key={id} id={id} onClose={onClose} />;
}

function Loaded({ id, onClose }: { id: number; onClose: () => void }) {
  const { data } = useCall<ProxyDetail>("proxy", { id }, String(id));
  return data ? <Form proxy={data} onClose={onClose} /> : null;
}

function Form({ proxy, onClose }: { proxy: ProxyDetail; onClose: () => void }) {
  const t = useT();
  const ct = useCt();
  const [draft, setDraft] = useState<Draft>(proxy);
  const [shown, setShown] = useState(false);
  const [portBad, setPortBad] = useState(false);
  const patch = (p: Partial<Draft>) => setDraft({ ...draft, ...p });
  const hostOk = draft.host.trim() !== "";
  const dirty = (Object.keys(draft) as (keyof Draft)[]).some((k) => draft[k] !== proxy[k]);
  const note = !hostOk ? t("editProxy.hostRequired") : portBad ? t("numField.invalidRange", { min: 1, max: 65535 }) : !dirty ? t("editProxy.noChanges") : null;
  return (
    <DialogModal
      open
      onOpenChange={(o) => { if (!o) onClose(); }}
      title={t("editProxy.title", { name: proxy.name || `${proxy.host}:${proxy.port}` })}
      dirty={dirty}
      onConfirm={() => { toast.ok(ct("creator.proxies.saved")); onClose(); }}
      confirmLabel={t("editProxy.save")}
      isDisabled={!hostOk || portBad || !dirty}
      footerNote={note}
    >
      <div className="flex flex-col gap-4 py-4">
        <Input label={t("proxyEditor.nameLabel")} inputSize="xsmall" wrapperClassName="gap-[6px]" placeholder={`${draft.host}:${draft.port}`} value={draft.name} onChange={(e) => patch({ name: e.target.value })} />
        <div className="grid grid-cols-2 gap-stack-md">
          <SelectField
            label={t("proxyEditor.typeLabel")}
            value={draft.kind}
            onChange={(kind) => patch({ kind })}
            options={[{ value: "socks5" as const, label: "SOCKS5" }, { value: "http" as const, label: "HTTP" }, { value: "https" as const, label: "HTTPS" }]}
          />
          <Field label={t("proxyEditor.countryLabel")} value={draft.country} onChange={(country) => patch({ country })} />
        </div>
        <div className="grid grid-cols-[1fr_96px] gap-stack-md">
          <Input label={t("proxyEditor.hostLabel")} inputSize="xsmall" wrapperClassName="gap-[6px]" className="mono" required aria-invalid={!hostOk || undefined}
            value={draft.host} error={!hostOk ? t("editProxy.hostRequired") : undefined} onChange={(e) => patch({ host: e.target.value })} />
          <NumField label={t("proxyEditor.portLabel")} value={draft.port} min={1} max={65535} onChange={(port) => patch({ port })} onInvalid={setPortBad} />
        </div>
        <div className="grid grid-cols-2 gap-stack-md">
          <Field label={t("proxyEditor.usernameLabel")} value={draft.username} onChange={(username) => patch({ username })} />
          <Input
            label={t("proxyEditor.passwordLabel")}
            inputSize="xsmall"
            wrapperClassName="gap-[6px]"
            className="[&:has(button:focus)]:outline-hidden"
            secret
            type={shown ? "text" : "password"}
            value={draft.password}
            onChange={(e) => patch({ password: e.target.value })}
            rightIcon={<IconButton size="3xsmall" className="focus-ring-inset" label={t("proxyPeek.showPassword")} aria-pressed={shown} icon={shown ? <EyeOffIcon className="size-3.5" /> : <EyeIcon className="size-3.5" />} onClick={() => setShown((v) => !v)} />}
          />
        </div>
        <Field label={t("proxyPeek.notes")} value={draft.notes} onChange={(notes) => patch({ notes })} />
      </div>
    </DialogModal>
  );
}
