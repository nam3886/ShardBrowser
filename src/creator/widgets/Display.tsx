// Shard's Display button and popover (ProxyDisplayMenu): the list's order,
// then its columns as checkboxes, then "Reset to default" once something
// changed. The columns are the ones the list's header saves (useListLayout
// keeps them under `storageKey`); a change here is written there and the
// list reads it again (`onChanged` remounts it).
import { useId, useRef, useState } from "react";
import { Button, Checkbox, Popover, Tooltip } from "@proxyshard/shardx-ui-kit";
import { DisplayIcon } from "../../shared/icons";
import { SelectField } from "../../shared/ui/SelectField";
import { columnChoices, defaultSet, fromStored, isDefaultSet, setShown, toStored, type ColumnDef, type ColumnSet } from "../../shared/lib/tableColumns";
import { useT } from "../../shared/i18n";
import { useCt } from "../i18n";

type Order = { value: string; options: { value: string; label: string }[]; onChange: (v: string) => void; initial: string };

export function DisplayMenu<Id extends string>({ storageKey, defs, order, onChanged }: { storageKey: string; defs: readonly ColumnDef<Id>[]; order: Order; onChanged: () => void }) {
  const t = useT();
  const ct = useCt();
  const noteId = useId();
  const [open, setOpen] = useState(false);
  const trigger = useRef<HTMLButtonElement>(null);
  const read = () => {
    let raw: unknown = null;
    try { raw = JSON.parse(localStorage.getItem(storageKey) ?? "null"); } catch { /* a broken value: the defaults */ }
    return { raw, got: fromStored(defs, raw) ?? { set: defaultSet(defs), widths: {} } };
  };
  const { got } = read();
  const write = (set: ColumnSet<Id>) => {
    const { raw, got: now } = read();
    const sort = raw && typeof raw === "object" ? (raw as { sort?: unknown }).sort ?? null : null;
    try { localStorage.setItem(storageKey, JSON.stringify({ ...toStored(set, now.widths), sort })); } catch { /* private mode */ }
    onChanged();
  };
  // `excluded` is Shard's word for a column its page leaves out by context
  // (Tags with no tag, Folder inside a folder); our lists leave none out.
  // Short of room Shard narrows or moves a column, and its Display says nothing.
  const choices = columnChoices(defs, got.set, { excluded: [] }, t);
  const changed = order.value !== order.initial || !isDefaultSet(defs, got.set);
  const label = ct("creator.display.title");
  return (
    <>
      <Tooltip content={label} whenTruncated>
        <Button ref={trigger} variant="neutral" mode="stroke" size="xsmall" leftIcon={<DisplayIcon className="size-3.5" />} aria-haspopup="dialog" aria-expanded={open} onClick={() => setOpen((v) => !v)}>
          {label}
        </Button>
      </Tooltip>
      <Popover open={open} onOpenChange={setOpen} anchorRef={trigger} align="end" aria-label={label} className="flex w-[240px] flex-col gap-3 p-3">
        <SelectField label={ct("creator.display.order")} value={order.value} onChange={order.onChange} options={order.options} />
        <fieldset className="m-0 flex flex-col gap-2 border-0 p-0">
          <legend className="mb-2 p-0 text-section-title text-text-strong-950">{ct("creator.display.columns")}</legend>
          {choices.map((c) => (
            <div key={c.id} className="flex flex-wrap items-center gap-x-2 gap-y-0.5">
              <Checkbox
                wrapperClassName="min-w-0 grow"
                label={c.title}
                checked={c.checked}
                disabled={c.disabled}
                aria-describedby={c.note ? `${noteId}-${c.id}` : undefined}
                onChange={(e) => write(setShown(defs, got.set, c.id, e.target.checked))}
              />
              {c.note && <span id={`${noteId}-${c.id}`} className="ms-auto min-w-0 text-end text-secondary text-text-soft-400">{c.note}</span>}
            </div>
          ))}
        </fieldset>
        {changed && (
          <Button variant="neutral" mode="ghost" size="2xsmall" className="self-start" onClick={() => { order.onChange(order.initial); write(defaultSet(defs)); }}>
            {ct("creator.display.reset")}
          </Button>
        )}
      </Popover>
    </>
  );
}
