// The owner's decisions: open questions first, then the decided log. Nobody else is shown this.
import type { ReactNode } from 'react';
import { Block, Rows } from './AreaView';
import type { EntryContext } from './Entry';

export function DecisionsPlace({ ctx, onBack, notes }: { ctx: EntryContext; onBack: () => void; notes?: ReactNode }) {
  const { plan } = ctx.w;
  return (
    <Block
      title="Decisions" line="Only you see this." onBack={onBack} notes={notes}
      list={(
        <>
          <section aria-label="Open">
            <h3 className="ws-grp">To decide</h3>
            {plan.openDecisions.length ? <Rows rows={plan.openDecisions} ctx={ctx} /> : <p className="ws-empty">Nothing to decide.</p>}
          </section>
          <section aria-label="Decided">
            <h3 className="ws-grp">Decided</h3>
            <Rows rows={plan.decided} ctx={ctx} />
          </section>
        </>
      )}
    >
      <p className="ws-tally">{plan.openDecisions.length ? <i>{plan.openDecisions.length} to decide</i> : null}{plan.openDecisions.length ? ' · ' : ''}{plan.decided.length} decided</p>
    </Block>
  );
}
