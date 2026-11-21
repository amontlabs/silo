// The map of the plan, part by part, drawn from a layout (map.ts). Every part is a button; opening one folds the drawing to a strip that
// stays on screen. Each box carries both of its places, so the fold is a CSS transition of the same elements.
import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { CSSProperties } from 'react';
import { arrowPath, crossPath, FOLD_HEIGHT, MAP_HEIGHT, MAP_WIDTH, placeStyle } from './map';
import type { Drawing, Glyph, PageLayout } from './map';
import { stateWords } from './plan';
import type { PartGroup, PartState } from './plan';

const SPOKEN: Record<PartState, string> = { done: 'done', next: 'next', planned: 'planned', open: 'not decided yet' };

/** What a screen reader hears for a part or a tile. */
export function spoken(group: PartGroup): string {
  const features = group.counts.built + group.counts.planned + group.counts.undecided;
  const decisions = group.decisions.decided + group.decisions.undecided;
  const state = group.state === 'planned' ? stateWords(group) : SPOKEN[group.state];
  const counted = [`${features} ${features === 1 ? 'feature' : 'features'}`, ...(decisions ? [`${decisions} ${decisions === 1 ? 'decision' : 'decisions'}`] : [])];
  return `${group.part.title}, ${state}, ${counted.join(', ')}. ${group.part.line}`;
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

/** A wire as one map draws it: its lines there, the part whose state colours it, and whether it is not decided. */
interface Drawn {
  of: string;
  drawing: Drawing;
  undecided?: boolean;
}

/** The classes of a wire, a wire's word, a zone or a text: its part's state when it has one, and `undecided` when its `rests` say it is not decided. */
const marks = (base: string, thing: { undecided?: boolean }, state?: PartState) => [base, state ? `s-${state}` : '', thing.undecided ? 'undecided' : ''].filter(Boolean).join(' ');

function Wires({ wires, height, className, states }: { wires: Drawn[]; height: number; className: string; states: Map<string, PartState> }) {
  return (
    <svg className={`ws-wires ${className}`} viewBox={`0 0 ${MAP_WIDTH} ${height}`} aria-hidden="true">
      {wires.map(({ of, drawing, undecided }, index) => (
        <g className={marks('ws-wire', { undecided }, states.get(of) ?? 'open')} key={index}>
          <path d={drawing.cross ? `${drawing.d}${crossPath(drawing.cross)}` : drawing.d} />
          {drawing.arrows?.map((arrow) => <path className="ah" key={arrow.join()} d={arrowPath(arrow)} />)}
        </g>
      ))}
    </svg>
  );
}

/** What a solid and a dashed line mean, drawn as the wires draw them. */
function Key({ at: [x, y] }: { at: [number, number] }) {
  const line = (undecided: boolean) => (
    <svg className={marks('ws-wire', { undecided })} viewBox="0 0 24 2"><path d="M0 1H24" /></svg>
  );
  return (
    <div className="ws-mapkey" aria-hidden="true" style={style({ '--lx': x, '--ly': y })}>
      <span>{line(false)}decided</span>
      <span>{line(true)}not decided</span>
    </div>
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
  layout: PageLayout;
  groups: PartGroup[];
  folded: boolean;
  current: string | null;
  onOpen: (part: string) => void;
}

export function MapView({ layout, groups, folded, current, onOpen }: MapViewProps) {
  const states = new Map(groups.map((group) => [group.part.id, group.state]));
  const byId = new Map(groups.map((group) => [group.part.id, group]));
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

  const showTip = (target: HTMLElement, group: PartGroup) => {
    clearTimeout(timer.current);
    timer.current = setTimeout(() => {
      const box = target.getBoundingClientRect();
      const text = folded ? `${group.part.title}. ${group.part.line}` : group.part.line;
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
          <div className={marks(`ws-geo ws-zone${zone.rounded ? ' rounded' : ''}`, zone)} key={zone.label} style={style(placeStyle(zone.full, zone.fold))} aria-hidden="true">
            <span className="ws-zlabel">{zone.label}</span>
            {zone.corners ? <><i className="ws-dia tl" /><i className="ws-dia tr" /><i className="ws-dia bl" /><i className="ws-dia br" /></> : null}
          </div>
        ))}
        <Wires wires={layout.wires.map((wire) => ({ of: wire.of, drawing: wire, undecided: wire.undecided }))} height={MAP_HEIGHT} className="w-full" states={states} />
        <Wires wires={layout.wires.flatMap((wire) => (wire.fold ? [{ of: wire.of, drawing: wire.fold, undecided: wire.undecided }] : []))} height={FOLD_HEIGHT} className="w-fold" states={states} />
        {layout.wires.filter((wire) => wire.label).map((wire, index) => {
          const [x, y, text, anchor] = wire.label!;
          return (
            <span className={marks(`ws-wl${anchor === 'middle' ? ' mid' : ''}`, wire, states.get(wire.of) ?? 'open')} key={index} aria-hidden="true" style={style({ '--lx': x, '--ly': y })}>{text}</span>
          );
        })}
        {layout.key ? <Key at={layout.key} /> : null}
        {layout.notes.map((note) => (
          // A text with no folded place stays where it is and fades out as the map folds.
          <div className={marks(`ws-geo ws-node${note.fold ? '' : ' off-fold'}`, note)} key={note.title} style={style(placeStyle(note.full, note.fold ?? note.full))} aria-hidden="true">
            <b>{note.title}</b><span>{note.line}</span>
          </div>
        ))}
        {layout.ghosts.map((ghost, index) => (
          <i className={`ws-geo ws-ghost s-${states.get(ghost.of) ?? 'open'}`} key={index} style={style(placeStyle(ghost.full, ghost.fold))} aria-hidden="true" />
        ))}
        {Object.entries(layout.places).map(([id, place]) => {
          const group = byId.get(id);
          if (!group) return null;
          const on = current === id;
          const classes = ['ws-geo', 'ws-part', `s-${group.state}`, place.inner ? 'inner' : '', place.tall ? 'tall' : '', place.wide ? 'wide' : '', on ? 'on' : ''].filter(Boolean).join(' ');
          return (
            <button
              type="button" key={id} data-part={id} className={classes} style={style(placeStyle(place.full, place.fold, place))}
              aria-label={spoken(group)} aria-current={on ? 'true' : undefined}
              onClick={() => { hideTip(); onOpen(id); }}
              onMouseEnter={(event) => showTip(event.currentTarget, group)} onMouseLeave={hideTip}
              onFocus={(event) => { if (focusVisible(event.currentTarget)) showTip(event.currentTarget, group); }} onBlur={hideTip}
            >
              <span className="ws-p-ttl">{group.part.title}</span>
              <span className="ws-p-st"><i className="ws-mk" /><span>{stateWords(group)}</span></span>
              {place.glyph ? <GlyphView glyph={place.glyph} /> : null}
            </button>
          );
        })}
      </div>
      {tip ? <div className="ws-tip" ref={tipRef} aria-hidden="true" style={{ transform: `translate(${Math.round(tip.x)}px, ${Math.round(tip.y)}px) translateX(-50%)` }}>{tip.text}</div> : null}
    </section>
  );
}
