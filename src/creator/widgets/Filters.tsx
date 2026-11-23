import type { ReactNode } from "react";
// Shard's list filters, as the Proxies page wires them (ProxyFilterBar and
// ProxyFilterChips): the Filters button and menu in the header, the applied
// filters as chips under it.
import { useState } from "react";
import { FilterChips, FilterMenu } from "../../shared/ui/filters";
import type { FilterField } from "../../shared/ui/filters/types";
import { useCt } from "../i18n";

export type { FilterField };

export function FilterButton({ fields }: { fields: FilterField[] }) {
  const ct = useCt();
  const [open, setOpen] = useState(false);
  return <FilterMenu open={open} onOpenChange={setOpen} fields={fields} activeCount={fields.filter((f) => f.current !== null).length} label={ct("creator.filters.label")} />;
}

/** `after`: Shard's FilterChips slot after the chips (one action that the filters make possible). */
export function AppliedFilters({ fields, after }: { fields: FilterField[]; after?: ReactNode }) {
  const ct = useCt();
  const chips = fields.filter((f) => f.current !== null).map((f) => ({
    id: f.id,
    label: ct("creator.filters.chip", { field: f.label, value: f.values.find((v) => v.value === f.current)?.label ?? f.current! }),
    onRemove: () => f.set(null),
  }));
  return <FilterChips chips={chips} onClear={() => fields.forEach((f) => f.set(null))} after={after} />;
}
