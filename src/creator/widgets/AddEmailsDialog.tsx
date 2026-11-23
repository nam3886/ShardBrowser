// Add emails: paste Outlook combos (email|password|refresh_token|client_id|
// recovery) or ask Ele for new temp mailboxes. A DialogModal with a Textarea
// and the parse result line, as Shard's proxy import reads what was pasted.
import { useEffect, useState } from "react";
import { DialogModal, SegmentControl, Textarea } from "@proxyshard/shardx-ui-kit";
import { FieldGroup } from "../../shared/ui/FieldGroup";
import { NumField } from "../../shared/ui/NumField";
import { toast } from "../../shared/model/toast";
import { call } from "../api";
import { useCt } from "../i18n";

const SAMPLE = "lan.pham@outlook.example|pass1|M.C5_BAY.0.U.-refresh|9e5f94bc-client|lan.r@recovery.example\nhai.do@outlook.example|pass2|M.C5_BAY.0.U.-refresh|9e5f94bc-client|hai.r@recovery.example\nnot a combo line";

export function AddEmailsDialog({ open, onClose }: { open: boolean; onClose: () => void }) {
  const ct = useCt();
  const [kind, setKind] = useState<"outlook" | "ele">("outlook");
  const [text, setText] = useState(SAMPLE);
  const [count, setCount] = useState(10);
  const [parsed, setParsed] = useState<{ valid: number; invalid: number } | null>(null);
  useEffect(() => {
    if (kind !== "outlook") return;
    void call<{ valid: number; invalid: number }>("parse_outlook", { text }).then(setParsed, () => setParsed(null));
  }, [kind, text]);
  const n = kind === "outlook" ? parsed?.valid ?? 0 : count;
  return (
    <DialogModal
      open={open}
      onClose={onClose}
      size="lg"
      title={ct("creator.addEmails.title")}
      confirmLabel={ct("creator.addEmails.confirm", { n })}
      isDisabled={n === 0}
      onConfirm={() => { toast.ok(ct("creator.addEmails.added", { n })); onClose(); }}
    >
      <div className="flex flex-col gap-4 py-4">
        <FieldGroup label={ct("creator.addEmails.kind")}>
          {(labelId) => (
            <SegmentControl
              aria-labelledby={labelId}
              size="small"
              value={kind}
              onChange={(v) => setKind(v as "outlook" | "ele")}
              items={[{ value: "outlook", label: ct("creator.mailbox.outlook") }, { value: "ele", label: ct("creator.mailbox.ele") }]}
            />
          )}
        </FieldGroup>
        {kind === "outlook" ? (
          <>
            <Textarea
              label={ct("creator.addEmails.combos")}
              hint={ct("creator.addEmails.combosHint")}
              technical
              rows={7}
              value={text}
              onChange={(e) => setText(e.target.value)}
              className="mono"
            />
            {parsed && (
              <p data-parse-result className="m-0 text-secondary text-text-sub-600">
                {ct("creator.addEmails.parsed", { valid: parsed.valid })}
                {parsed.invalid > 0 && <span className="text-error-text">{", "}{ct("creator.addEmails.invalid", { n: parsed.invalid })}</span>}
              </p>
            )}
          </>
        ) : (
          <div className="w-[160px]">
            <NumField label={ct("creator.addEmails.count")} value={count} min={1} max={100} onChange={setCount} />
          </div>
        )}
      </div>
    </DialogModal>
  );
}
