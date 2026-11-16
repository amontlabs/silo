// The drawing of Silo, part by part. Every part is a button; opening one folds the drawing to a strip that
// stays on screen. Each box carries both of its places, so the fold is a CSS transition of the same elements.
import { useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { CSSProperties, ReactNode } from 'react';
import { arrowPath, crossPath, FOLD_HEIGHT, GHOSTS, LAYOUT, MAP_HEIGHT, MAP_WIDTH, OUTSIDE, placeStyle, WIRES_FOLD, WIRES_FULL, ZONES } from './map';
import type { Wire } from './map';
import { stateWords } from './plan';
import type { AreaGroup, AreaState } from './plan';

const SPOKEN: Record<AreaState, string> = { done: 'done', now: 'being built now', next: 'next', planned: 'planned', open: 'not decided yet' };

/** What a screen reader hears for a part or a tile. */
export function spoken(group: AreaGroup, toDecide: number): string {
  const features = group.rows.length - group.counts.dropped;
  const state = group.state === 'planned' ? stateWords(group) : SPOKEN[group.state];
  return `${group.area.title}, ${state}, ${features} ${features === 1 ? 'feature' : 'features'}${toDecide ? `, ${toDecide} to decide` : ''}. ${group.area.line}`;
}

const GLYPHS: Record<string, ReactNode> = {
  app: (
    <svg className="ws-gl" viewBox="0 0 148 50" aria-hidden="true" style={{ left: 'calc(15 * var(--u))', top: 'calc(70 * var(--u))', width: 'calc(148 * var(--u))' }}>
      <rect x=".5" y=".5" width="68" height="48" rx="2" /><path d="M.5 11.5h68" />
      <circle className="solid" cx="7.5" cy="6" r="1.2" /><circle className="solid" cx="12.5" cy="6" r="1.2" /><circle className="solid" cx="17.5" cy="6" r="1.2" />
      <path d="M8.5 21h4m6 0h34M8.5 30h4m6 0h26M8.5 39h4m6 0h30" />
      <rect x="79.5" y=".5" width="68" height="48" rx="2" /><text x="87.5" y="20.5">$ silo</text>
      <rect className="solid" x="126.5" y="12.5" width="5" height="10" /><path d="M87.5 32h30M87.5 40h18" />
    </svg>
  ),
  engine: (
    <svg className="ws-gl" viewBox="21.5 21.5 85 85" strokeLinecap="round" aria-hidden="true" style={{ left: 'calc(30 * var(--u))', top: 'calc(160 * var(--u))', width: 'calc(52 * var(--u))' }}>
      <path vectorEffect="non-scaling-stroke" d="M60.386 102.832A39 39 0 1 1 98.435 82.309" />
      <path vectorEffect="non-scaling-stroke" d="M88.625 72.344A26 26 0 1 1 75.804 40.834" />
      <path className="core" vectorEffect="non-scaling-stroke" d="M72.591 54.243A13 13 0 1 1 52.316 58.301" />
    </svg>
  ),
  devices: (
    <svg className="ws-gl" viewBox="0 0 81 51" aria-hidden="true" style={{ left: 'calc(15 * var(--u))', top: 'calc(62 * var(--u))', width: 'calc(81 * var(--u))' }}>
      <rect x=".5" y=".5" width="13" height="50" rx="1.5" /><rect x="30.5" y=".5" width="50" height="21" rx="1.5" />
      <rect x="30.5" y="29.5" width="50" height="21" rx="1.5" /><path d="M13.5 11h17M13.5 40h17" />
    </svg>
  ),
};

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
  groups: AreaGroup[];
  folded: boolean;
  current: string | null;
  toDecide: Map<string, number>;
  onOpen: (area: string) => void;
}

export function MapView({ groups, folded, current, toDecide, onOpen }: MapViewProps) {
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
      <div className={folded ? 'ws-stage fold' : 'ws-stage'}>
        {ZONES.map((zone) => (
          <div className={`ws-geo ws-zone${zone.rounded ? ' rounded' : ''}`} key={zone.label} style={style(placeStyle(zone.full, zone.fold))} aria-hidden="true">
            <span className="ws-zlabel">{zone.label}</span>
            {zone.corners ? <><i className="ws-dia tl" /><i className="ws-dia tr" /><i className="ws-dia bl" /><i className="ws-dia br" /></> : null}
          </div>
        ))}
        <Wires wires={WIRES_FULL} height={MAP_HEIGHT} className="w-full" states={states} />
        <Wires wires={WIRES_FOLD} height={FOLD_HEIGHT} className="w-fold" states={states} />
        {WIRES_FULL.filter((wire) => wire.label).map((wire, index) => {
          const [x, y, text, anchor] = wire.label!;
          return (
            <span className={`ws-wl s-${states.get(wire.of) ?? 'open'}${anchor === 'middle' ? ' mid' : ''}`} key={index} aria-hidden="true" style={style({ '--lx': x, '--ly': y })}>{text}</span>
          );
        })}
        <div className="ws-geo ws-node" style={style(placeStyle(OUTSIDE.full, OUTSIDE.fold))} aria-hidden="true">
          <b>{OUTSIDE.title}</b><span>{OUTSIDE.line}</span>
        </div>
        {GHOSTS.map((ghost, index) => (
          <i className={`ws-geo ws-ghost s-${states.get(ghost.of) ?? 'open'}`} key={index} style={style(placeStyle(ghost.full, ghost.fold))} aria-hidden="true" />
        ))}
        {Object.entries(LAYOUT).map(([id, place]) => {
          const group = byId.get(id);
          if (!group) return null;
          const todo = toDecide.get(id) ?? 0;
          const on = current === id;
          const classes = ['ws-geo', 'ws-part', `s-${group.state}`, place.inner ? 'inner' : '', place.tall ? 'tall' : '', on ? 'on' : ''].filter(Boolean).join(' ');
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
              {GLYPHS[id] ?? null}
              {todo ? <b className="ws-todo">{todo}</b> : null}
            </button>
          );
        })}
      </div>
      {tip ? <div className="ws-tip" ref={tipRef} aria-hidden="true" style={{ transform: `translate(${Math.round(tip.x)}px, ${Math.round(tip.y)}px) translateX(-50%)` }}>{tip.text}</div> : null}
    </section>
  );
}
