// A list's selection bar on Shard's BulkBarFrame: the count, the actions
// (icons only when the column is narrow, as Shard's bars do), More, and X.
import type { ReactNode } from "react";
import { Button, IconButton, useContextMenu, useLatched, type MenuItem } from "@proxyshard/shardx-ui-kit";
import { CloseIcon, MoreIcon } from "../../shared/icons";
import { BulkBarFrame } from "../../shared/ui/BulkBarFrame";
import { useCt } from "../i18n";

/** `danger`: Shard's bar draws it in the error variant and never as an icon alone (CommandButton). */
export type BulkAction = { id: string; label: string; icon: ReactNode; onClick: () => void; danger?: boolean };

export function BulkBar({ n, actions, more = [], onClear }: { n: number; actions: BulkAction[]; more?: MenuItem[]; onClear: () => void }) {
  const ct = useCt();
  const menu = useContextMenu();
  const label = useLatched(n > 0 ? ct("creator.bulk.selected", { n }) : null) ?? "";
  return (
    <BulkBarFrame open={n > 0} label={label}>
      {(narrow) => (
        <>
          <span className="mr-1.5 text-secondary font-medium text-text-strong-950 tabular-nums">{label}</span>
          {actions.map((a) => (narrow && !a.danger
            ? <IconButton key={a.id} label={a.label} icon={a.icon} size="3xsmall" onClick={a.onClick} />
            : <Button key={a.id} variant={a.danger ? "error" : "neutral"} mode="ghost" size="3xsmall" leftIcon={a.icon} onClick={a.onClick}>{a.label}</Button>))}
          {more.length > 0 && (
            <IconButton
              label={ct("creator.peek.more")}
              icon={<MoreIcon className="size-3.5" />}
              size="3xsmall"
              aria-haspopup="menu"
              aria-expanded="false"
              onClick={(e) => menu.openFrom(e.currentTarget, more)}
            />
          )}
          <IconButton label={ct("creator.bulk.clear")} icon={<CloseIcon className="size-3.5" />} size="3xsmall" onClick={onClear} />
          {menu.node}
        </>
      )}
    </BulkBarFrame>
  );
}
