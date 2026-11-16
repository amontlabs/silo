// An opened area: its title, line, one dot per entry and the counts on the left, its entries on the right.
// The same layout holds the suggestions, the entries not carried over and the owner's decisions.
import type { ReactNode } from 'react';
import { Entry, pipClass } from './Entry';
import type { EntryContext } from './Entry';
import { countRows } from './plan';
import type { AreaGroup, Counts, Row } from './plan';

export interface BlockProps {
  num?: string;
  title: string;
  line?: string;
  onBack: () => void;
  children?: ReactNode;
  list: ReactNode;
  /** Quiet lines under the counts: the API unavailable, sign-in failed. */
  notes?: ReactNode;
}

/** The two columns every opened place uses. */
export function Block({ num, title, line, onBack, children, list, notes }: BlockProps) {
  return (
    <>
      <div className="ws-block">
        <button type="button" className="ws-back" onClick={onBack}>
          <span className="lbl-map">← Full map</span><span className="lbl-order">← All areas</span>
        </button>
        <span className="ws-num">{num ?? ''}</span>
        <h2 id="ws-area-title" tabIndex={-1}>{title}</h2>
        {line ? <p className="ws-aline">{line}</p> : null}
        {children}
        {notes ? <div className="ws-notes wide">{notes}</div> : null}
      </div>
      <div className="ws-list">{list}{notes ? <div className="ws-notes narrow">{notes}</div> : null}</div>
    </>
  );
}

export function Rows({ rows, openKey, ctx }: { rows: Row[]; openKey: string | null; ctx: EntryContext }) {
  return <ul className="ws-rows">{rows.map((row) => <Entry key={row.key} row={row} open={openKey === row.key} ctx={ctx} />)}</ul>;
}

function tally(counts: Counts, toDecide: number): ReactNode[] {
  const { built, building, planned, undecided, later, dropped } = counts;
  const parts: ReactNode[] = [];
  // Each count stays on one line; lines break after a separator.
  const one = (n: number, words: string) => `${n} ${words}`.replace(/ /g, '\u00a0');
  if (built) parts.push(one(built, 'built'));
  if (building) parts.push(<i key="b">{one(building, 'being built')}</i>);
  if (planned) parts.push(one(planned, 'planned'));
  if (toDecide) parts.push(<i key="d">{one(toDecide, 'to decide')}</i>);
  if (undecided - toDecide > 0) parts.push(one(undecided - toDecide, 'not decided'));
  if (later) parts.push(one(later, 'later'));
  if (dropped) parts.push(one(dropped, 'dropped'));
  return parts.flatMap((part, index) => (index ? ['\u00a0· ', part] : [part]));
}

export function AreaPlace({ group, toDecide, openKey, ctx, onBack, notes }: { group: AreaGroup; toDecide: number; openKey: string | null; ctx: EntryContext; onBack: () => void; notes?: ReactNode }) {
  return (
    <Block
      num={String(group.number).padStart(2, '0')} title={group.area.title} line={group.area.line} onBack={onBack} notes={notes}
      list={<Rows rows={group.rows} openKey={openKey} ctx={ctx} />}
    >
      <div className="ws-pips" aria-hidden="true">
        {group.rows.filter((row) => row.status !== 'dropped').map((row) => <i className={pipClass(row)} key={row.key} />)}
      </div>
      <p className="ws-tally">{tally(group.counts, toDecide)}</p>
    </Block>
  );
}

export function LeftPlace({ rows, openKey, ctx, onBack, notes }: { rows: Row[]; openKey: string | null; ctx: EntryContext; onBack: () => void; notes?: ReactNode }) {
  return (
    <Block
      title="Not carried over" line="Left out of the rebuild, or dropped." onBack={onBack} notes={notes}
      list={<Rows rows={rows} openKey={openKey} ctx={ctx} />}
    >
      <p className="ws-tally">{tally(countRows(rows), 0)}</p>
    </Block>
  );
}
