// Add emails: paste Outlook combos (email|password|refresh_token|client_id|
// recovery) or ask Ele for new temp mailboxes. A DialogModal with a Textarea
// and the parse result line, as Shard's proxy import reads what was pasted.
// After Add the dialog stays open and that line reports what was done.
import { useEffect, useState } from "react";
import { DialogModal, SegmentControl, Textarea } from "@proxyshard/shardx-ui-kit";
import { FieldGroup } from "../../shared/ui/FieldGroup";
import { NumField } from "../../shared/ui/NumField";
import { toast } from "../../shared/model/toast";
import { call, MOCK } from "../api";
import { useCt } from "../i18n";

const SAMPLE = MOCK
  ? "lan.pham@outlook.example|pass1|M.C5_BAY.0.U.-refresh|9e5f94bc-client|lan.r@recovery.example\nhai.do@outlook.example|pass2|M.C5_BAY.0.U.-refresh|9e5f94bc-client|hai.r@recovery.example\nnot a combo line"
  : "";

/** Most new Ele mailboxes one Add makes (the backend's `add_ele` limit). */
const ELE_MAX = 100;
/** Unread line numbers shown; the rest are counted. */
const LINES_SHOWN = 20;

type Report = { added: number; duplicates: number[]; invalid: [number, string][]; error?: string };

export function AddEmailsDialog({ open, onClose, onAdded }: { open: boolean; onClose: () => void; onAdded?: () => void }) {
  const ct = useCt();
  const [kind, setKind] = useState<"outlook" | "ele">("outlook");
  const [text, setText] = useState(SAMPLE);
  const [count, setCount] = useState(10);
  const [parsed, setParsed] = useState<{ valid: number; invalid: number } | null>(null);
  const [report, setReport] = useState<Report | null>(null);
  const [busy, setBusy] = useState(false);
  useEffect(() => {
    if (kind !== "outlook") return;
    void call<{ valid: number; invalid: number }>("parse_outlook", { text }).then(setParsed, () => setParsed(null));
  }, [kind, text]);
  // A new paste or type starts over.
  useEffect(() => setReport(null), [kind, text, count]);
  const n = kind === "outlook" ? parsed?.valid ?? 0 : count;

  const add = () => {
    setBusy(true);
    const done = kind === "outlook"
      ? call<Report>("add_outlook", { text })
      : call<{ created: number; error?: string }>("add_ele", { count }).then((r) => ({ added: r.created, duplicates: [], invalid: [], error: r.error }));
    void done.then((r) => {
      setReport(r);
      if (r.error) toast.fail({ title: ct("creator.addEmails.title"), detail: r.error });
      onAdded?.();
    }, (e) => toast.fail({ title: ct("creator.addEmails.title"), detail: e })).finally(() => setBusy(false));
  };

  const lines = (r: Report) => {
    const shown = r.invalid.slice(0, LINES_SHOWN).map(([line]) => line).join(", ");
    const rest = r.invalid.length - LINES_SHOWN;
    return rest > 0 ? `${shown}, ${ct("creator.addEmails.more", { n: rest })}` : shown;
  };

  return (
    <DialogModal
      open={open}
      onClose={onClose}
      size="lg"
      title={ct("creator.addEmails.title")}
      confirmLabel={n === 1 ? ct("creator.addEmails.confirmOne") : ct("creator.addEmails.confirm", { n })}
      isDisabled={n === 0 || !!report}
      isLoading={busy}
      onConfirm={add}
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
          <Textarea
            label={ct("creator.addEmails.combos")}
            hint={ct("creator.addEmails.combosHint")}
            technical
            rows={7}
            value={text}
            onChange={(e) => setText(e.target.value)}
            className="mono"
          />
        ) : (
          <div className="w-[160px]">
            <NumField label={ct("creator.addEmails.count")} value={count} min={1} max={ELE_MAX} onChange={setCount} />
          </div>
        )}
        {report ? (
          <p data-add-report className="m-0 text-secondary text-text-sub-600">
            {report.added === 1 ? ct("creator.addEmails.addedOne") : ct("creator.addEmails.added", { n: report.added })}
            {report.duplicates.length > 0 && <>{", "}{report.duplicates.length === 1 ? ct("creator.addEmails.duplicatesOne") : ct("creator.addEmails.duplicates", { n: report.duplicates.length })}</>}
            {report.invalid.length > 0 && (
              <span className="text-error-text">
                {", "}{report.invalid.length === 1 ? ct("creator.addEmails.invalidOne") : ct("creator.addEmails.invalid", { n: report.invalid.length })}{": "}{lines(report)}
              </span>
            )}
          </p>
        ) : kind === "outlook" && parsed && (
          <p data-parse-result className="m-0 text-secondary text-text-sub-600">
            {ct("creator.addEmails.parsed", { valid: parsed.valid })}
            {parsed.invalid > 0 && <span className="text-error-text">{", "}{parsed.invalid === 1 ? ct("creator.addEmails.invalidOne") : ct("creator.addEmails.invalid", { n: parsed.invalid })}</span>}
          </p>
        )}
      </div>
    </DialogModal>
  );
}
