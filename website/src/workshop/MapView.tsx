// The map of the plan, part by part, drawn from a layout (map.ts). Every part is a button; opening one folds the drawing to a strip that
// stays on screen. Each box carries both of its places, so the fold is a CSS transition of the same elements.
import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { CSSProperties } from 'react';
import { arrowPath, crossPath, FOLD_HEIGHT, MAP_HEIGHT, MAP_WIDTH, placeStyle } from './map';
import type { Drawing, Glyph, PageLayout } from './map';
import { stateWords } from './plan';
import type { PartGroup, PartState } from './plan';

const SPOKEN: Record<PartState, string> = { building: 'being built', built: 'built', next: 'next', decided: 'decided', open: 'not decided' };

/** The titles of some parts, in the order given, joined by commas. */
const titlesOf = (ids: string[], titles: Map<string, string>) => ids.map((id) => titles.get(id) ?? id).join(', ');

/** What a screen reader hears for a part or a tile; with `titles` (part titles by id), also the parts it depends on, which it cannot see marked. */
export function spoken(group: PartGroup, titles?: Map<string, string>): string {
  const features = group.counts.built + group.counts.building + group.counts.decided + group.counts.undecided;
  const decisions = group.decisions.decided + group.decisions.undecided;
  const state = group.state === 'decided' ? stateWords(group) : SPOKEN[group.state];
  const counted = [`${features} ${features === 1 ? 'feature' : 'features'}`, ...(decisions ? [`${decisions} ${decisions === 1 ? 'decision' : 'decisions'}`] : [])];
  const depends = titles && group.part.dependsOn.length ? ` Depends on ${titlesOf(group.part.dependsOn, titles)}.` : '';
  return `${group.part.title}, ${state}, ${counted.join(', ')}. ${group.part.line}${depends}`;
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
function Key({ at: [x, y], marked }: { at: [number, number]; marked: boolean }) {
  const line = (undecided: boolean) => (
    <svg className={marks('ws-wire', { undecided })} viewBox="0 0 24 2"><path d="M0 1H24" /></svg>
  );
  return (
    <div className="ws-mapkey" aria-hidden="true" style={style({ '--lx': x, '--ly': y })}>
      <span>{line(false)}decided</span>
      <span>{line(true)}not decided</span>
      {marked ? <span><i className="ws-litkey" />it depends on</span> : null}
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
  /** Parts marked from outside the map (a film's), on the full map and the folded one; every other part fades. */
  marked?: ReadonlySet<string> | null;
  onOpen: (part: string) => void;
}

export function MapView({ layout, groups, folded, current, marked = null, onOpen }: MapViewProps) {
  const states = new Map(groups.map((group) => [group.part.id, group.state]));
  const byId = new Map(groups.map((group) => [group.part.id, group]));
  const titles = new Map(groups.map((group) => [group.part.id, group.part.title]));
  // The part under the pointer and the one holding the keyboard's focus. While either is on a part, that part is the active one,
  // the pointer's winning when they differ: the parts it depends on are marked, and every other part fades.
  // Only on the full map; the strip marks and fades nothing. The marks go when neither is on a part.
  const [pointer, setPointer] = useState<string | null>(null);
  const [focus, setFocus] = useState<string | null>(null);
  const active = pointer ?? focus;
  const related = active && !folded ? byId.get(active)?.part : undefined;
  const [tip, setTip] = useState<{ head: string; x: number; top: number; bottom: number } | null>(null);
  const stageRef = useRef<HTMLDivElement>(null);
  const tipRef = useRef<HTMLDivElement>(null);
  // Centred on its part and kept 8 px inside the window. It never covers its part while it fits below it, or else above it;
  // it is as tall as its lines, and is pulled inside the window only when it fits in neither place.
  useLayoutEffect(() => {
    const node = tipRef.current;
    if (!node || !tip) return;
    const { offsetWidth: width, offsetHeight: height } = node;
    const left = Math.max(8, Math.min(innerWidth - width - 8, tip.x - width / 2));
    const lowest = innerHeight - height - 8;
    const below = tip.bottom + 8;
    const above = tip.top - height - 8;
    const top = below <= lowest ? below : above >= 8 ? above : Math.max(8, Math.min(lowest, above));
    node.style.transform = `translate(${Math.round(left)}px, ${Math.round(top)}px)`;
  }, [tip]);
  // The tip of the active part shows after 240 ms, and goes when the active part changes or none is left.
  useEffect(() => {
    setTip(null);
    if (!active) return;
    const timer = setTimeout(() => {
      const group = byId.get(active);
      const target = [...(stageRef.current?.querySelectorAll<HTMLElement>('.ws-part') ?? [])].find((node) => node.dataset.part === active);
      if (!group || !target) return;
      const box = target.getBoundingClientRect();
      const head = folded ? `${group.part.title}. ${group.part.line}` : group.part.line;
      setTip({ head, x: box.left + box.width / 2, top: box.top, bottom: box.bottom });
    }, 240);
    return () => clearTimeout(timer);
  }, [active, folded]);

  return (
    <section className="ws-mapsec" aria-label="Silo, part by part">
      <div ref={stageRef} className={['ws-stage', folded ? 'fold' : '', related || marked ? 'relating' : '', groups.length ? '' : 'ws-stage-empty'].filter(Boolean).join(' ')}>
        {groups.length ? null : <p className="ws-quiet">No part yet.</p>}
        {layout.zones.map((zone) => (
          // A zone with no folded place stays where it is and fades out as the map folds.
          <div className={marks(`ws-geo ws-zone${zone.rounded ? ' rounded' : ''}${zone.fold ? '' : ' off-fold'}`, zone)} key={zone.label} style={style(placeStyle(zone.full, zone.fold ?? zone.full))} aria-hidden="true">
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
        {layout.key ? <Key at={layout.key} marked={Boolean(related?.dependsOn.length)} /> : null}
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
          const classes = ['ws-geo', 'ws-part', `s-${group.state}`, place.inner ? 'inner' : '', place.tall ? 'tall' : '', place.wide ? 'wide' : '', on ? 'on' : '',
            ...(marked && !related ? [marked.has(id) ? 'marked' : 'faint'] : [related && related.dependsOn.includes(id) ? 'marked' : '', related && id !== related.id && !related.dependsOn.includes(id) ? 'faint' : ''])].filter(Boolean).join(' ');
          return (
            <button
              type="button" key={id} data-part={id} className={classes} style={style(placeStyle(place.full, place.fold, place))}
              aria-label={spoken(group, titles)} aria-current={on ? 'true' : undefined}
              onClick={() => { setPointer(null); setFocus(null); onOpen(id); }}
              onMouseEnter={() => setPointer(id)} onMouseLeave={() => setPointer((now) => (now === id ? null : now))}
              onFocus={(event) => { if (focusVisible(event.currentTarget)) setFocus(id); }} onBlur={() => setFocus((now) => (now === id ? null : now))}
            >
              <span className="ws-p-ttl">{group.part.title}</span>
              <span className="ws-p-st"><i className="ws-mk" /><span>{stateWords(group)}</span></span>
              {place.glyph ? <GlyphView glyph={place.glyph} /> : null}
            </button>
          );
        })}
      </div>
      {tip ? (
        <div className="ws-tip" ref={tipRef} aria-hidden="true" style={{ transform: `translate(${Math.round(tip.x)}px, ${Math.round(tip.top)}px) translateX(-50%)` }}>
          {tip.head}
        </div>
      ) : null}
    </section>
  );
}
