// The map of the plan, part by part, drawn from a layout (map.ts). Every part is a button; opening one folds the drawing to a strip that
// stays on screen. Each box carries both of its places, so the fold is a CSS transition of the same elements.
import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { CSSProperties } from 'react';
import { arrowPath, crossPath, FOLD_HEIGHT, MAP_HEIGHT, MAP_WIDTH, placeStyle } from './map';
import type { Glyph, MapLayout, Wire } from './map';
import { stateWords } from './plan';
import type { AreaGroup, AreaState } from './plan';

const SPOKEN: Record<AreaState, string> = { done: 'done', now: 'being built now', next: 'next', planned: 'planned', open: 'not decided yet' };

/** What a screen reader hears for a part or a tile. */
export function spoken(group: AreaGroup, toDecide: number): string {
  const count = group.rows.length - group.counts.dropped;
  const state = group.state === 'planned' ? stateWords(group) : SPOKEN[group.state];
  return `${group.area.title}, ${state}, ${count} ${count === 1 ? 'feature' : 'features'}${toDecide ? `, ${toDecide} to decide` : ''}. ${group.area.line}`;
}

/** A part's glyph, drawn from the layout's paths. */
function GlyphView({ glyph }: { glyph: Glyph }) {
  const [left, top, width] = glyph.at;
  return (
    <svg className="ws-gl" viewBox={glyph.viewBox} strokeLinecap="round" aria-hidden="true" style={{ left: `calc(${left} * var(--u))`, top: `calc(${top} * var(--u))`, width: `calc(${width} * var(--u))` }}>
      {glyph.paths.map((path, index) => <path key={index} className={path.kind} vectorEffect="non-scaling-stroke" d={path.d} />)}
    </svg>
  );
}

function Wires({ wires, height, className, states }: { wires: Wire[]; height: number; className: string; states: Map<string, AreaState> }) {
  return (
    <svg className={`ws-wires ${className}`} viewBox={`0 0 ${MAP_WIDTH} ${height}`} aria-hidden="true">
      {wires.map((wire, index) => (
        <g className={`ws-wire s-${states.get(wire.of) ?? 'open'}`} key={index}>
          <path d={wire.cross ? `${wire.d}${crossPath(wire.cross)}` : wire.d} />
          {wire.arrows?.map((arrow) => <path className="ah" key={arrow.join()} d={arrowPath(arrow)} />)}
        </g>
      ))}
    </svg>
  );
}

function focusVisible(node: Element): boolean {
  try {
    return node.matches(':focus-visible');
  } catch {
    return false;
  }
}

const style = (vars: Record<string, string | number>) => vars as CSSProperties;

export interface MapViewProps {
  layout: MapLayout;
  groups: AreaGroup[];
  folded: boolean;
  current: string | null;
  toDecide: Map<string, number>;
  onOpen: (area: string) => void;
}

export function MapView({ layout, groups, folded, current, toDecide, onOpen }: MapViewProps) {
  const states = new Map(groups.map((group) => [group.area.id, group.state]));
  const byId = new Map(groups.map((group) => [group.area.id, group]));
  const [tip, setTip] = useState<{ text: string; x: number; y: number } | null>(null);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  const tipRef = useRef<HTMLDivElement>(null);
  // Centred on its part, and kept 8 px inside the window.
  useLayoutEffect(() => {
    const node = tipRef.current;
    if (!node || !tip) return;
    const width = node.offsetWidth;
    const left = Math.max(8, Math.min(innerWidth - width - 8, tip.x - width / 2));
    node.style.transform = `translate(${Math.round(left)}px, ${Math.round(tip.y)}px)`;
  }, [tip]);
  useEffect(() => () => clearTimeout(timer.current), []);

  const showTip = (target: HTMLElement, group: AreaGroup) => {
    clearTimeout(timer.current);
    timer.current = setTimeout(() => {
      const box = target.getBoundingClientRect();
      const text = folded ? `${group.area.title}. ${group.area.line}` : group.area.line;
      const below = box.height < 200 && box.bottom + 40 < innerHeight;
      setTip({ text, x: box.left + box.width / 2, y: below ? box.bottom + 8 : box.top - 34 });
    }, 240);
  };
  const hideTip = () => {
    clearTimeout(timer.current);
    setTip(null);
  };

  return (
    <section className="ws-mapsec" aria-label="Silo, part by part">
      <div className={['ws-stage', folded ? 'fold' : '', groups.length ? '' : 'ws-stage-empty'].filter(Boolean).join(' ')}>
        {groups.length ? null : <p className="ws-quiet">No part yet.</p>}
        {layout.zones.map((zone) => (
          <div className={`ws-geo ws-zone${zone.rounded ? ' rounded' : ''}`} key={zone.label} style={style(placeStyle(zone.full, zone.fold))} aria-hidden="true">
            <span className="ws-zlabel">{zone.label}</span>
            {zone.corners ? <><i className="ws-dia tl" /><i className="ws-dia tr" /><i className="ws-dia bl" /><i className="ws-dia br" /></> : null}
          </div>
        ))}
        <Wires wires={layout.wires.full} height={MAP_HEIGHT} className="w-full" states={states} />
        <Wires wires={layout.wires.fold} height={FOLD_HEIGHT} className="w-fold" states={states} />
        {layout.wires.full.filter((wire) => wire.label).map((wire, index) => {
          const [x, y, text, anchor] = wire.label!;
          return (
            <span className={`ws-wl s-${states.get(wire.of) ?? 'open'}${anchor === 'middle' ? ' mid' : ''}`} key={index} aria-hidden="true" style={style({ '--lx': x, '--ly': y })}>{text}</span>
          );
        })}
        {layout.notes.map((note) => (
          <div className="ws-geo ws-node" key={note.title} style={style(placeStyle(note.full, note.fold))} aria-hidden="true">
            <b>{note.title}</b><span>{note.line}</span>
          </div>
        ))}
        {layout.ghosts.map((ghost, index) => (
          <i className={`ws-geo ws-ghost s-${states.get(ghost.of) ?? 'open'}`} key={index} style={style(placeStyle(ghost.full, ghost.fold))} aria-hidden="true" />
        ))}
        {Object.entries(layout.places).map(([id, place]) => {
          const group = byId.get(id);
          if (!group) return null;
          const todo = toDecide.get(id) ?? 0;
          const on = current === id;
          const classes = ['ws-geo', 'ws-part', `s-${group.state}`, place.inner ? 'inner' : '', place.tall ? 'tall' : '', place.wide ? 'wide' : '', on ? 'on' : ''].filter(Boolean).join(' ');
          return (
            <button
              type="button" key={id} data-part={id} className={classes} style={style(placeStyle(place.full, place.fold, place))}
              aria-label={spoken(group, todo)} aria-current={on ? 'true' : undefined}
              onClick={() => { hideTip(); onOpen(id); }}
              onMouseEnter={(event) => showTip(event.currentTarget, group)} onMouseLeave={hideTip}
              onFocus={(event) => { if (focusVisible(event.currentTarget)) showTip(event.currentTarget, group); }} onBlur={hideTip}
            >
              <span className="ws-p-ttl">{group.area.title}</span>
              <span className="ws-p-st"><i className="ws-mk" /><span>{stateWords(group)}</span></span>
              {place.glyph ? <GlyphView glyph={place.glyph} /> : null}
              {todo ? <b className="ws-todo ws-late-o">{todo}</b> : null}
            </button>
          );
        })}
      </div>
      {tip ? <div className="ws-tip" ref={tipRef} aria-hidden="true" style={{ transform: `translate(${Math.round(tip.x)}px, ${Math.round(tip.y)}px) translateX(-50%)` }}>{tip.text}</div> : null}
    </section>
  );
}
