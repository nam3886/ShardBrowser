// A folding section of cards over a list, as the Profiles page's overview
// draws "Problems to fix": Shard's FoldHeading and its ProblemCard (title
// with a pill, what is wrong, why it matters, one detail line or a usage bar,
// the fix and a quiet "N more"), at most three to a row.
import { useId, useState } from "react";
import { FoldHeading } from "../../shared/ui/FoldHeading";
import { ProblemCard } from "../../widgets/Overview/ProblemCard";
import type { CardModel } from "../../widgets/Overview/useProblemCards";

export type { CardModel };

type CardFields = Omit<CardModel, "kind" | "usage" | "risk" | "show" | "hide">;
type CardExtras = Partial<Pick<CardModel, "usage" | "show" | "hide">>;
/** A card as ProblemCard takes it; our kinds are not Shard's signals. */
export const card = (kind: string, m: CardFields & CardExtras): CardModel =>
  ({ usage: null, risk: false, show: () => {}, hide: null, ...m, kind: kind as CardModel["kind"] });

export function CardSection({ id, label, cards }: { id: string; label: string; cards: CardModel[] }) {
  const listId = useId();
  const labelId = useId();
  const [folded, setFolded] = useState(false);
  if (cards.length === 0) return null;
  return (
    <div data-creator-cards={id} className="flex shrink-0 flex-col">
      <div role="group" aria-labelledby={labelId} className="flex h-[24px] items-center box-content">
        <FoldHeading label={label} labelId={labelId} open={!folded} controls={listId} onToggle={() => setFolded(!folded)}
          after={folded ? <>{" "}<span className="tabular-nums">{cards.length}</span></> : undefined} />
      </div>
      {!folded && (
        <div id={listId} className="grid gap-2.5 pt-1 pb-2" style={{ gridTemplateColumns: `repeat(${Math.min(3, cards.length)}, minmax(0, 1fr))` }}>
          {cards.map((m, i) => <ProblemCard key={m.kind} model={m} fresh={false} slot={i} onLeave={() => {}} />)}
        </div>
      )}
    </div>
  );
}
