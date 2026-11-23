// Platforms: the catalogue as Shard's Extensions page draws its card grid
// (kit DataList grid, the card's own text, its actions in the card's menu).
// A pinned platform carries the pin a pinned profile shows by its name; its
// rule and its own options (tries, waits) are changed in an Edit dialog, as
// a proxy's are; options every platform shares are in Settings.
import { useMemo, useState } from "react";
import { Badge, Button, DataList, DialogModal, EmptyState, Switch, Tooltip } from "@proxyshard/shardx-ui-kit";
import { PageBody, PageHeader } from "../../widgets/PageHeader";
import { AddIcon, EditIcon, PinIconApp, SearchIcon } from "../../shared/icons";
import { LoadError, LoadingCards } from "../../shared/ui/ListStates";
import { NumField } from "../../shared/ui/NumField";
import { rowActionIcons } from "../../shared/ui/rowActionIcons";
import { toast } from "../../shared/model/toast";
import { useCt } from "../i18n";
import type { PlatformCard } from "../types";
import { AppliedFilters, FilterButton, type FilterField } from "../widgets/Filters";
import { MakeDialog } from "../widgets/MakeDialog";
import { PlatformMark, platformLabel, useCall } from "../widgets/parts";

export function PlatformsPage() {
  const ct = useCt();
  const { data, error, status, reload } = useCall<PlatformCard[]>("platforms", undefined, "");
  const [cat, setCat] = useState<string | null>(null);
  const [q, setQ] = useState("");
  const [making, setMaking] = useState<string | null>(null);
  const [editing, setEditing] = useState<PlatformCard | null>(null);
  const all = data ?? [];
  const shown = useMemo(() => all.filter((p) => (!cat || p.category === cat) && p.label.toLowerCase().includes(q.trim().toLowerCase())), [all, cat, q]);
  const fields: FilterField[] = [{
    id: "category", label: ct("creator.platforms.category"), current: cat, set: setCat,
    values: (["signin", "chat", "voice"] as const).map((c) => ({ value: c, label: ct(`creator.category.${c}`), count: all.filter((p) => p.category === c).length })),
  }];

  return (
    <>
      <PageHeader
        title={ct("creator.nav.platforms")}
        count={status === "ready" ? (q.trim() || cat ? { shown: shown.length, total: all.length } : all.length) : undefined}
        help={{ title: ct("creator.platforms.helpTitle"), body: ct("creator.platforms.helpBody") }}
        filter={{ value: q, onChange: setQ }}
        actions={(
          <div className="flex flex-none items-center gap-2">
            <FilterButton fields={fields} />
            <Button variant="primary" mode="filled" size="xsmall" leftIcon={<AddIcon className="size-3.5" />} onClick={() => setMaking("")}>{ct("creator.make.open")}</Button>
          </div>
        )}
      />
      <PageBody scroll={false}>
        <AppliedFilters fields={fields} />
        <DataList<PlatformCard>
          variant="grid"
          minCardWidth={280}
          aria-label={ct("creator.nav.platforms")}
          items={shown}
          getId={(p) => p.key}
          getName={(p) => p.label}
          labels={{ selectAll: ct("creator.list.selectAll"), select: (name) => ct("creator.list.select", { name }), more: ct("creator.peek.more"), actions: ct("creator.col.actions") }}
          status={status}
          loading={<LoadingCards minWidth={280} />}
          error={<LoadError what={ct("creator.platforms.what")} error={error} onRetry={reload} />}
          empty={<EmptyState icon={<SearchIcon />} title={ct("creator.platforms.noMatch", { q: q.trim() })} actions={<Button variant="neutral" mode="stroke" size="xsmall" onClick={() => setQ("")}>{ct("creator.list.clearSearch")}</Button>} />}
          renderCard={(p) => (
            <article data-platform={p.key} className="flex h-full w-full min-w-0 flex-col gap-2.5 rounded-tile px-cell py-[10px]">
              <div className="flex min-w-0 items-start gap-2.5">
                <PlatformMark k={p.key} card />
                <div className="min-w-0 flex-1 pe-5">
                  <div className="flex min-w-0 items-center gap-1.5">
                    <Tooltip content={p.label} whenTruncated><h2 className="m-0 truncate text-secondary font-medium text-text-strong-950">{p.label}</h2></Tooltip>
                    {p.starred && <span data-pin className="inline-flex shrink-0 text-icon-soft-400"><PinIconApp aria-hidden="true" className="size-3.5" /></span>}
                  </div>
                  <div className="mt-0.5 flex min-w-0 items-center gap-1.5 text-secondary text-text-soft-400">
                    <span className="truncate">{p.routes.map((r) => (r === "email" ? ct("creator.route.email") : ct("creator.make.routeVia", { name: platformLabel(r) }))).join(", ")}</span>
                    {!p.ready && <Badge size="small" color="gray" className="shrink-0">{ct("creator.platforms.later")}</Badge>}
                  </div>
                </div>
              </div>
              <p className="m-0 line-clamp-2 min-h-[2.4em] break-words text-secondary text-text-sub-600">
                {ct("creator.platforms.free", { n: p.free })}{", "}{p.account_rule === "one" ? ct("creator.platforms.oneAccount") : ct("creator.platforms.separate")}
              </p>
            </article>
          )}
          menuItems={(p) => [
            { label: ct("creator.platforms.make"), icon: <AddIcon className="size-3.5" />, disabled: !p.ready, onSelect: () => setMaking(p.key) },
            { label: ct("creator.platforms.edit"), icon: <EditIcon className="size-3.5" />, onSelect: () => setEditing(p) },
            { type: "separator" },
            p.starred
              ? { label: ct("creator.platforms.unpin"), icon: rowActionIcons.unpin({ className: "size-3.5" }), onSelect: () => {} }
              : { label: ct("creator.platforms.pin"), icon: rowActionIcons.pin({ className: "size-3.5" }), onSelect: () => {} },
          ]}
        />
      </PageBody>
      <MakeDialog open={making !== null} platform={making || undefined} onClose={() => setMaking(null)} />
      {editing && <EditPlatformDialog key={editing.key} p={editing} onClose={() => setEditing(null)} />}
    </>
  );
}

/** A platform's own settings, saved by hand, as Edit proxy saves a proxy's. */
function EditPlatformDialog({ p, onClose }: { p: PlatformCard; onClose: () => void }) {
  const ct = useCt();
  const [one, setOne] = useState(p.account_rule === "one");
  const [values, setValues] = useState(() => Object.fromEntries(p.options.map((o) => [o.key, o.value])));
  return (
    <DialogModal
      open
      onClose={onClose}
      size="sm"
      title={ct("creator.platforms.editTitle", { name: p.label })}
      confirmLabel={ct("creator.platforms.save")}
      onConfirm={() => { toast.saved(ct("creator.platforms.saved", { name: p.label })); onClose(); }}
    >
      <div className="flex flex-col gap-4 py-4">
        <Switch checked={one} disabled={p.rule_fixed} onChange={setOne} label={ct("creator.platforms.oneAccount")} />
        {p.options.map((o) => (
          <NumField key={o.key} label={ct(`creator.knob.${o.key}`)} value={values[o.key]} min={o.min} max={o.max} onChange={(v) => setValues({ ...values, [o.key]: v })} />
        ))}
      </div>
    </DialogModal>
  );
}
