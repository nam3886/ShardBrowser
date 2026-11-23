// The list part of our list pages, on Shard's Trash recipe: the kit DataList
// with a saved column layout (useListLayout), its loading, error and empty
// states, the selection bar in the list's column and Peek beside it (the
// Proxies page's row: list column, then Peek). One scrolling list as every
// Shard list; the next window loads as its end comes into view.
import { useEffect, type ReactNode } from "react";
import { DataList, type MenuItem } from "@proxyshard/shardx-ui-kit";
import { LoadError, LoadingRows } from "../../shared/ui/ListStates";
import { ListColumn, useListEndRoom } from "../../shared/ui/ListColumn";
import { useListLayout, type ListColumnDef } from "../../shared/ui/listHeader";
import { useCt } from "../i18n";

/** How near the end, px, the next window is asked for. */
const NEAR_END = 400;

export function ListPage<Id extends string, Row extends { id: number }>({
  label, above, layoutKey, defs, items, onEnd, status, error, onRetry, what, empty, selected, onSelected, onOpen, openId, rowActions, menuItems, tail, bar, peek, name,
}: {
  label: string;
  /** In the list's column over the rows, as the Profiles page puts its
   *  overview cards and filter chips: Peek starts under the page header. */
  above?: ReactNode;
  layoutKey: string;
  defs: readonly ListColumnDef<Id, Row>[];
  items: Row[];
  /** The list's end is near: load the next window. */
  onEnd: () => void;
  status: "loading" | "error" | "ready";
  error: string | null;
  onRetry: () => void;
  /** What failed to load ("accounts"). */
  what: string;
  empty: ReactNode;
  selected: Set<string>;
  onSelected: (s: Set<string>) => void;
  onOpen?: (row: Row) => void;
  openId: number | null;
  rowActions?: (row: Row) => ReactNode;
  menuItems?: (row: Row) => MenuItem[];
  tail: number;
  bar: ReactNode;
  peek: ReactNode;
  name: (row: Row) => string;
}) {
  const ct = useCt();
  const list = useListLayout({ storageKey: layoutKey, defs, items, selectable: true, tail, actionsLabel: ct("creator.col.actions") });
  const endRoom = useListEndRoom(selected.size > 0);
  const scrollRef = list.layout.scrollRef;
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const check = () => { if (el.scrollHeight - el.scrollTop - el.clientHeight < NEAR_END) onEnd(); };
    el.addEventListener("scroll", check, { passive: true });
    return () => el.removeEventListener("scroll", check);
  }, [scrollRef, onEnd]);
  return (
    <div className="relative flex min-h-0 flex-1 gap-panel">
      <ListColumn>
        {above}
        <DataList<Row>
          aria-label={label}
          items={list.items}
          columns={list.columns}
          layout={list.layout}
          getId={(r) => String(r.id)}
          getName={name}
          labels={{
            selectAll: ct("creator.list.selectAll"),
            select: (n) => ct("creator.list.select", { name: n }),
            more: ct("creator.peek.more"),
            actions: ct("creator.col.actions"),
            selectedCount: (n) => ct("creator.bulk.selected", { n }),
          }}
          status={status}
          loading={<LoadingRows rows={6} inGrid />}
          error={<LoadError what={what} error={error} onRetry={onRetry} />}
          empty={empty}
          selected={selected}
          onSelectedChange={onSelected}
          openOnClick={!!onOpen}
          onOpen={onOpen}
          activeId={openId != null ? String(openId) : null}
          rowActions={rowActions}
          menuItems={menuItems}
          bottomInset={endRoom}
        />
        {bar}
      </ListColumn>
      {peek}
    </div>
  );
}
