// The owner's decisions, from plan/decisions.json, newest first. Nobody else is shown this.
import type { ReactNode } from 'react';
import { Block, Rows } from './AreaView';
import type { EntryContext } from './Entry';

export function DecisionsPlace({ ctx, onBack, notes }: { ctx: EntryContext; onBack: () => void; notes?: ReactNode }) {
  const { plan } = ctx.w;
  return (
    <Block
      title="Decisions" line="Only you see this." onBack={onBack} notes={notes}
      list={plan.decided.length ? <Rows rows={plan.decided} ctx={ctx} /> : <p className="ws-empty">Nothing decided yet.</p>}
    >
      <p className="ws-tally">{plan.decided.length} decided</p>
    </Block>
  );
}
