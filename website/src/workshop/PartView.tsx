// An opened part: its title, line, one dot per feature and the counts on the left, its decisions and features on the
// right. The same layout holds the decisions of Product, the suggestions, the dropped entries and the latest changes.
import type { ReactNode } from 'react';
import { Entry, pipClass } from './Entry';
import type { EntryContext } from './Entry';
import { countDecisions, countRows } from './plan';
import type { Counts, DecisionCounts, PartGroup, Row } from './plan';

export interface BlockProps {
  num?: string;
  title: string;
  line?: string;
  onBack: () => void;
  children?: ReactNode;
  list: ReactNode;
  /** Quiet lines under the counts: the API unavailable, sign-in failed. */
  notes?: ReactNode;
  /** While the list loads, nothing sits below it in its column. */
  loading?: boolean;
}

/** The two columns every opened place uses. */
export function Block({ num, title, line, onBack, children, list, notes, loading = false }: BlockProps) {
  return (
    <>
      <div className="ws-block">
        <button type="button" className="ws-back" onClick={onBack}>
          <span className="lbl-map">← Full map</span><span className="lbl-order">← All parts</span>
        </button>
        <span className="ws-num">{num ?? ''}</span>
        <h2 id="ws-part-title" tabIndex={-1}>{title}</h2>
        {line ? <p className="ws-aline">{line}</p> : null}
        {children}
        {notes ? <div className="ws-notes wide">{notes}</div> : null}
      </div>
      <div className={loading ? 'ws-list ws-list-loading' : 'ws-list'}>{list}{notes && !loading ? <div className="ws-notes narrow">{notes}</div> : null}</div>
    </>
  );
}

export function Rows({ rows, ctx }: { rows: Row[]; ctx: EntryContext }) {
  return <ul className="ws-rows">{rows.map((row) => <Entry key={row.key} row={row} open={ctx.open.has(row.key)} ctx={ctx} />)}</ul>;
}

/** Each count stays on one line; lines break after a separator. */
const one = (n: number, words: string) => `${n} ${words}`.replace(/ /g, '\u00a0');
const joined = (items: ReactNode[]) => items.flatMap((item, index) => (index ? ['\u00a0· ', item] : [item]));

/** The counts of a part's features. */
function tally(counts: Counts): ReactNode[] {
  const { built, planned, undecided, dropped } = counts;
  const items: ReactNode[] = [];
  if (built) items.push(one(built, 'built'));
  if (planned) items.push(one(planned, 'planned'));
  if (undecided) items.push(one(undecided, 'not decided'));
  if (dropped) items.push(one(dropped, 'dropped'));
  return joined(items);
}

/** The counts of a part's decisions, on a line of their own after "Decisions:". */
function decisionTally(counts: DecisionCounts): ReactNode[] {
  const items: ReactNode[] = [];
  if (counts.decided) items.push(one(counts.decided, 'decided'));
  if (counts.undecided) items.push(one(counts.undecided, 'not decided'));
  if (counts.replaced) items.push(one(counts.replaced, 'replaced'));
  if (counts.dropped) items.push(one(counts.dropped, 'dropped'));
  return joined(items);
}

/** The tallies of a list of rows: its features', then its decisions'. */
function Tallies({ rows }: { rows: Row[] }) {
  const features = tally(countRows(rows.filter((row) => row.of === 'feature')));
  const decisions = decisionTally(countDecisions(rows.filter((row) => row.of === 'decision')));
  return (
    <>
      {features.length ? <p className="ws-tally">{features}</p> : null}
      {decisions.length ? <p className="ws-tally">Decisions: {decisions}</p> : null}
    </>
  );
}

export function PartPlace({ group, ctx, onBack, notes }: { group: PartGroup; ctx: EntryContext; onBack: () => void; notes?: ReactNode }) {
  return (
    <Block
      num={String(group.number).padStart(2, '0')} title={group.part.title} line={group.part.line} onBack={onBack} notes={notes}
      list={<Rows rows={group.rows} ctx={ctx} />}
    >
      <div className="ws-pips" aria-hidden="true">
        {group.rows.filter((row) => row.of === 'feature' && row.status !== 'dropped').map((row) => <i className={pipClass(row)} key={row.key} />)}
      </div>
      <Tallies rows={group.rows} />
    </Block>
  );
}

/** Product: the decisions that are not about one part, from plan/decisions/. */
export function ProductPlace({ rows, ctx, onBack, notes }: { rows: Row[]; ctx: EntryContext; onBack: () => void; notes?: ReactNode }) {
  return (
    <Block
      title="Product" line="The decisions that are not about one part." onBack={onBack} notes={notes}
      list={<Rows rows={rows} ctx={ctx} />}
    >
      <Tallies rows={rows} />
    </Block>
  );
}

export function LeftPlace({ rows, ctx, onBack, notes }: { rows: Row[]; ctx: EntryContext; onBack: () => void; notes?: ReactNode }) {
  return (
    <Block
      title="Left out" onBack={onBack} notes={notes}
      list={<Rows rows={rows} ctx={ctx} />}
    >
      <Tallies rows={rows} />
    </Block>
  );
}
