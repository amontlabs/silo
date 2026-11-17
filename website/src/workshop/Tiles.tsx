// The plan as a sheet of tiles in build order: the Order view on wide screens, all a phone gets.
// With an area open, the tiles fold into a strip of names above it.
import type { CSSProperties } from 'react';
import { spoken } from './MapView';
import { pipClass } from './Entry';
import { stateWords } from './plan';
import type { AreaGroup } from './plan';

export interface TilesProps {
  groups: AreaGroup[];
  /** The open area, or 'suggested'. */
  current: string | null;
  toDecide: Map<string, number>;
  suggested: number;
  onOpen: (area: string) => void;
  onSuggest: () => void;
}

const number = (n: number) => String(n).padStart(2, '0');

function Tile({ group, on, todo, onOpen }: { group: AreaGroup; on: boolean; todo: number; onOpen: (area: string) => void }) {
  const { counts, state } = group;
  const partBuilt = state !== 'done' && state !== 'now' && counts.built > 0;
  const share = Math.round((100 * counts.built) / Math.max(1, counts.built + counts.planned));
  return (
    <div className={`ws-tile s-${state}${partBuilt ? ' part-built' : ''}${on ? ' on' : ''}`} data-area={group.area.id} style={{ '--p': share } as CSSProperties}>
      <button type="button" className="ws-tile-hit" aria-label={spoken(group, todo)} aria-current={on ? 'true' : undefined} onClick={() => onOpen(group.area.id)}>
        <span className="ws-tile-top">
          <span className="ws-num">{number(group.number)}</span>
          <span className="ws-lab">
            {todo ? <b className="ws-late-o">{todo} to decide</b> : <>{state === 'now' ? <i className="ws-live" /> : null}{stateWords(group)}</>}
          </span>
        </span>
        <span className="ws-title"><i className="ws-mark" />{group.area.title}</span>
        <span className="ws-tline">{group.area.line}</span>
        <span className="ws-pips" aria-hidden="true">
          {group.rows.filter((row) => row.status !== 'dropped').map((row) => <i className={pipClass(row)} key={row.key} />)}
        </span>
      </button>
    </div>
  );
}

export function Tiles({ groups, current, toDecide, suggested, onOpen, onSuggest }: TilesProps) {
  const first = groups.filter((group) => group.first);
  const after = groups.filter((group) => !group.first);
  const tile = (group: AreaGroup) => <Tile key={group.area.id} group={group} on={current === group.area.id} todo={toDecide.get(group.area.id) ?? 0} onOpen={onOpen} />;
  const on = current === 'suggested';
  return (
    <nav className="ws-board" aria-label="Areas, in build order">
      <div className="ws-band ws-band-first">{first.map(tile)}</div>
      {after.length && first.length ? <p className="ws-cut">After that</p> : null}
      <div className="ws-band ws-band-after">
        {after.map(tile)}
        <div className={`ws-tile ws-suggest-tile${on ? ' on' : ''}`} data-area="suggested">
          <button type="button" className="ws-tile-hit" aria-current={on ? 'true' : undefined} onClick={onSuggest}>
            <span className="ws-tile-top"><span className="ws-num">+</span></span>
            <span className="ws-title">Suggest a feature</span>
            <span className="ws-tline">{suggested ? `${suggested} from people.` : 'Something Silo should do.'}</span>
          </button>
        </div>
      </div>
    </nav>
  );
}
