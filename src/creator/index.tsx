// Creator: everything of ours lives under src/creator/. Upstream renders these
// at its seams (App.tsx, Sidebar.tsx, settings page), so a new page is one
// entry here plus an id in sections.ts, never another upstream edit.
import { lazy, useEffect, useId, useState, type ComponentType, type ReactNode } from "react";
import { HugeiconsIcon } from "@hugeicons/react";
import { GridViewIcon, Mail01Icon, UserMultipleIcon } from "@hugeicons/core-free-icons";
import { CountRoll, ICON_STROKE } from "@proxyshard/shardx-ui-kit";
import { RouteIcon } from "../shared/icons";
import { FoldHeading } from "../shared/ui/FoldHeading";
import type { Section } from "../shared/types";
import type { Command } from "../shared/commands/types";
import { CREATOR_SECTIONS, type CreatorSection } from "./sections";
import type { NavCounts } from "./types";
import { call, MOCK } from "./api";
import { ct, useCt } from "./i18n";

export { CreatorSettingsSection, useCreatorDraft } from "./settings/CreatorSettingsSection";
export { creatorTabLabel } from "./labels";
export { CREATOR_DATA } from "./api";

/** Shard's icon recipe (shared/icons `make`): Hugeicons stroke at the app's weight. */
const icon = (svg: typeof Mail01Icon) => function CreatorIcon({ className }: { className?: string }) {
  return <HugeiconsIcon icon={svg} strokeWidth={ICON_STROKE} aria-hidden="true" focusable="false" className={className} />;
};

type PageDef = { labelKey: string; count?: keyof NavCounts; Icon: ComponentType<{ className?: string }>; Page: ComponentType };
const def = (labelKey: string, count: PageDef["count"], Icon: PageDef["Icon"], load: () => Promise<ComponentType>): PageDef =>
  ({ labelKey, count, Icon, Page: lazy(() => load().then((c) => ({ default: c }))) });

// Keyed by CreatorSection: tsc fails when an id in sections.ts has no page.
const PAGE_BY_ID: Record<CreatorSection, PageDef> = {
  "c-accounts": def("creator.nav.accounts", "accounts", icon(UserMultipleIcon), () => import("./pages/AccountsPage").then((m) => m.AccountsPage)),
  "c-emails": def("creator.nav.emails", "emails", icon(Mail01Icon), () => import("./pages/EmailsPage").then((m) => m.EmailsPage)),
  // Our sign-up proxies: Shard's Proxies page copied onto our own store (Shard's own page is untouched).
  "c-proxies": def("creator.nav.proxies", "proxies", RouteIcon, () => import("./pages/ProxiesPage").then((m) => m.ProxiesPage)),
  "c-platforms": def("creator.nav.platforms", "platforms", icon(GridViewIcon), () => import("./pages/PlatformsPage").then((m) => m.PlatformsPage)),
};
// Only Accounts runs on real data (M2, Breeze Blue); the other pages are
// mock-only until M7 wires them, so a real build leaves them out.
const PAGES = CREATOR_SECTIONS.filter((id) => MOCK || id === "c-accounts").map((id) => ({ id, ...PAGE_BY_ID[id] }));

/** Our pages as Shard's palette "Go to" commands (appCommands' nav.*), after its own (seam).
 *  `go` is Shard's own (keyboard to the page); "creator" and the English names find them in every language. */
export function creatorNavCommands(go: (s: Section) => () => void): Command[] {
  const ENGLISH: Record<string, string> = { "c-accounts": "accounts", "c-emails": "emails", "c-proxies": "proxy pool", "c-platforms": "platforms" };
  return PAGES.filter((p) => p.id in ENGLISH).map((p) => ({
    id: `nav.${p.id}`, area: "app", scope: "global", group: "navigate", icon: p.Icon,
    label: () => ct(p.labelKey), keywords: () => ["creator", ENGLISH[p.id]], surfaces: ["palette"], run: go(p.id),
  }));
}

/** The open section's page, inside Shard's LazyPage (passed in: it is private to App.tsx). */
export function CreatorPages({ section, Lazy }: { section: Section; Lazy: ComponentType<{ section: Section; children: ReactNode }> }) {
  const p = PAGES.find((x) => x.id === section);
  return p ? <Lazy section={p.id}><p.Page /></Lazy> : null;
}

type ItemProps = { id: Section; label: string; icon: ReactNode; compact: boolean; badge?: ReactNode; badgeLabel?: string };

const FOLD_KEY = "creator:sidebar.folded";
const readFolded = () => { try { return localStorage.getItem(FOLD_KEY) === "1"; } catch { return false; } };

/** Our pages as one sidebar group, built as the sidebar's own Folders group
 *  is (its private `Group`: FoldHeading with the chevron, the items under it,
 *  in the rail a hairline and the icons), each item with its count as a
 *  Folder shows its profiles. Shard's PageItem draws the items (passed in:
 *  it is private to the sidebar). Folded or not is this viewer's, as the
 *  sidebar keeps its groups'. */
export function CreatorNavGroup({ compact, Item }: { compact: boolean; Item: ComponentType<ItemProps> }) {
  const ct = useCt();
  const [counts, setCounts] = useState<NavCounts | null>(null);
  const [folded, setFolded] = useState(readFolded);
  const labelId = useId();
  const listId = useId();
  useEffect(() => { call<NavCounts>("nav_counts").then(setCounts, () => {}); }, []);
  const toggle = () => {
    setFolded(!folded);
    try { localStorage.setItem(FOLD_KEY, folded ? "0" : "1"); } catch { /* private mode: this session only */ }
  };
  const label = ct("creator.nav.group");
  const items = PAGES.map(({ id, labelKey, count, Icon }) => {
    const n = count && counts ? counts[count] : undefined;
    return (
      <Item
        key={id}
        id={id}
        label={ct(labelKey)}
        icon={<Icon className="size-3.5" />}
        compact={compact}
        badge={n !== undefined ? <span data-count className="flex"><CountRoll value={n} /></span> : undefined}
        badgeLabel={n !== undefined ? ct("creator.nav.countLabel", { n }) : undefined}
      />
    );
  });
  if (compact) {
    return (
      <div role="group" aria-labelledby={labelId} data-group="creator" className="flex flex-col gap-0.5">
        <span id={labelId} className="sr-only">{label}</span>
        <div aria-hidden="true" className="mx-2 my-1.5 border-t border-stroke-soft-200" />
        {items}
      </div>
    );
  }
  return (
    <div role="group" aria-labelledby={labelId} data-group="creator" className="flex flex-col gap-0.5">
      <div className="flex h-[24px] items-center gap-1.5 pl-2.5">
        <FoldHeading label={label} labelId={labelId} open={!folded} controls={listId} onToggle={toggle} />
        <span className="min-w-0 flex-1" />
      </div>
      {!folded && <div id={listId} className="flex flex-col gap-0.5">{items}</div>}
    </div>
  );
}
