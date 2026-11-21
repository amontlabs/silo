// The plan as a sheet of tiles in build order: the Order view on wide screens, all a phone gets.
// With a part open, the tiles fold into a strip of names above it.
import type { CSSProperties } from 'react';
import { spoken } from './MapView';
import { pipClass } from './Entry';
import { stateWords } from './plan';
import type { PartGroup } from './plan';

export interface TilesProps {
  groups: PartGroup[];
  /** The open part. */
  current: string | null;
  onOpen: (part: string) => void;
}

const number = (n: number) => String(n).padStart(2, '0');

function Tile({ group, on, onOpen }: { group: PartGroup; on: boolean; onOpen: (part: string) => void }) {
  const { counts, state } = group;
  const partBuilt = state !== 'done' && counts.built > 0;
  const share = Math.round((100 * counts.built) / Math.max(1, counts.built + counts.planned));
  return (
    <div className={`ws-tile s-${state}${partBuilt ? ' part-built' : ''}${on ? ' on' : ''}`} data-part={group.part.id} style={{ '--p': share } as CSSProperties}>
      <button type="button" className="ws-tile-hit" aria-label={spoken(group)} aria-current={on ? 'true' : undefined} onClick={() => onOpen(group.part.id)}>
        <span className="ws-tile-top">
          <span className="ws-num">{number(group.number)}</span>
          <span className="ws-lab">{stateWords(group)}</span>
        </span>
        <span className="ws-title"><i className="ws-mark" />{group.part.title}</span>
        <span className="ws-tline">{group.part.line}</span>
        <span className="ws-pips" aria-hidden="true">
          {group.rows.filter((row) => row.of === 'feature' && row.status !== 'dropped').map((row) => <i className={pipClass(row)} key={row.key} />)}
        </span>
      </button>
    </div>
  );
}

export function Tiles({ groups, current, onOpen }: TilesProps) {
  if (!groups.length) return null;
  const first = groups.filter((group) => group.first);
  const after = groups.filter((group) => !group.first);
  const tile = (group: PartGroup) => <Tile key={group.part.id} group={group} on={current === group.part.id} onOpen={onOpen} />;
  return (
    <nav className="ws-board" aria-label="Parts, in build order">
      {first.length ? <div className="ws-band ws-band-first">{first.map(tile)}</div> : null}
      {after.length && first.length ? <p className="ws-cut">After that</p> : null}
      {after.length ? <div className="ws-band ws-band-after">{after.map(tile)}</div> : null}
    </nav>
  );
}
