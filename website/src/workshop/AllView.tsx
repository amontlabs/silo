// The All view: one line of controls (the order, Filter and its chips, the count, Display), the strip of every feature in
// the order it is built, and the list. The controls and the strip stay under the header while the list scrolls; the
// address holds all of it (all.ts).
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import type { CSSProperties, KeyboardEvent as ReactKeyboardEvent, ReactNode } from 'react';
import { AllList, PIP_WORDS } from './AllList';
import { countOf, DEFAULT_SHOW, narrowed, PRODUCT, relationsOf, rowKeyOf, STATE_WORDS, STATES, stripKept } from './all';
import type { Graph, Query, Section, Show } from './all';
import type { EntryContext } from './Entry';
import { Icon } from './Films';
import type { Plan, Status } from './plan';
import type { RelationStore } from './relation';

export interface AllViewProps {
  plan: Plan;
  graph: Graph;
  /** The list, as all.ts `allSections` makes it of the query. */
  sections: Section[];
  query: Query;
  onQuery: (next: Query) => void;
  ctx: EntryContext;
  store: RelationStore;
  folds: ReadonlyMap<string, boolean>;
  onFold: (key: string, open: boolean) => void;
  more: ReadonlySet<string>;
  onMore: (key: string) => void;
  waiting: boolean;
  /** Opens an entry where it stands, or in its own place when the list does not hold it. */
  onOpen: (key: string) => void;
}

type Sub = 'state' | 'part' | 'film';
/** `from`: the chip that opened the menu, by its key, which takes the focus back when the menu closes. */
type MenuState = { kind: 'filter'; sub: Sub | null; text: string; from?: string } | { kind: 'display' } | null;

const SUBS: [Sub, string][] = [['state', 'State'], ['part', 'Part'], ['film', 'Film']];
const SHOW_WORDS: Record<Show, string> = { feature: 'Features', decision: 'Decisions', film: 'Films' };

/** One value a filter can take: its menu's kind, its value, what it says, its mark and how many entries have it. */
interface Value {
  sub: Sub;
  value: string;
  label: string;
  mark: ReactNode;
  count: number;
}

function valuesOf(plan: Plan, graph: Graph): Value[] {
  const rows = [...graph.rows.values()];
  return [
    ...STATES.map((state): Value => ({ sub: 'state', value: state, label: STATE_WORDS[state], mark: <i className={`ws-pip ${state}`} aria-hidden="true" />, count: rows.filter((row) => row.status === state).length })),
    ...plan.parts.map((group): Value => ({ sub: 'part', value: group.part.id, label: group.part.title, mark: null, count: group.rows.length })),
    { sub: 'part', value: PRODUCT, label: 'The whole product', mark: null, count: plan.product.length },
    ...plan.films.map((film): Value => ({ sub: 'film', value: film.id, label: film.title, mark: <Icon name="film" />, count: film.rows.length })),
  ];
}

const listOf = (query: Query, sub: Sub): string[] => (sub === 'state' ? query.states : sub === 'part' ? query.parts : query.films);
function withValue(query: Query, sub: Sub, value: string, on: boolean): Query {
  const now = listOf(query, sub);
  const next = on ? [...now.filter((one) => one !== value), value] : now.filter((one) => one !== value);
  if (sub === 'state') return { ...query, states: STATES.filter((state) => next.includes(state)) as Status[] };
  if (sub === 'part') return { ...query, parts: next };
  return { ...query, films: next };
}

function isField(target: EventTarget | null): boolean {
  return target instanceof HTMLElement && (target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName));
}

/** Moves the focus among a menu's buttons with the arrows. */
function menuKeys(event: ReactKeyboardEvent<HTMLDivElement>, onLeft?: () => void, onRight?: (button: HTMLElement) => void) {
  const menu = event.currentTarget;
  const buttons = [...menu.querySelectorAll<HTMLElement>(':scope > button')];
  const at = buttons.indexOf(document.activeElement as HTMLElement);
  if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
    event.preventDefault();
    const next = event.key === 'ArrowDown' ? Math.min(buttons.length - 1, at + 1) : at - 1;
    if (next < 0) menu.querySelector<HTMLElement>(':scope > input')?.focus();
    else buttons[next]?.focus();
  } else if (event.key === 'ArrowLeft' && onLeft && !isField(event.target)) {
    event.preventDefault();
    onLeft();
  } else if (event.key === 'ArrowRight' && onRight && at >= 0) {
    event.preventDefault();
    onRight(buttons[at]);
  }
}

function Check({ value, query, onQuery }: { value: Value; query: Query; onQuery: (next: Query) => void }) {
  const on = listOf(query, value.sub).includes(value.value);
  return (
    <button type="button" role="menuitemcheckbox" aria-checked={on} onClick={() => onQuery(withValue(query, value.sub, value.value, !on))}>
      {value.mark}<span>{value.label}</span><small>{value.count}</small><i className="wa-check" aria-hidden="true" />
    </button>
  );
}

export function AllView(props: AllViewProps) {
  const { plan, graph, sections, query, onQuery, store } = props;
  const [menu, setMenu] = useState<MenuState>(null);
  const bar = useRef<HTMLDivElement>(null);
  const filterButton = useRef<HTMLButtonElement>(null);
  const displayButton = useRef<HTMLButtonElement>(null);
  const values = useMemo(() => valuesOf(plan, graph), [plan, graph]);
  const count = useMemo(() => countOf(graph, query), [graph, query]);
  const kept = useMemo(() => stripKept(sections, query), [sections, query]);
  const chips = SUBS.flatMap(([sub]) => listOf(query, sub).map((value) => values.find((one) => one.sub === sub && one.value === value)).filter((one): one is Value => one !== undefined));

  // A move inside the Filter menu (typing, another submenu) keeps what opened it, until it closes.
  const within = (patch: { sub: Sub | null; text: string }) => setMenu((now) => ({ kind: 'filter', ...(now?.kind === 'filter' && now.from ? { from: now.from } : {}), ...patch }));
  const chipButton = (key: string) => bar.current?.querySelector<HTMLElement>(`.wa-chip[data-chip="${CSS.escape(key)}"] > button:first-child`);
  const close = (focus: 'filter' | 'display' | null = null) => {
    const from = menu?.kind === 'filter' ? menu.from : undefined;
    setMenu(null);
    if (focus === 'filter') (from && chipButton(from) || filterButton.current)?.focus();
    if (focus === 'display') displayButton.current?.focus();
  };
  // Where the focus goes once the chips are drawn again: the chip now at that place, else Filter.
  const chipFocus = useRef<number | null>(null);
  const chipKeys = chips.map((chip) => `${chip.sub}-${chip.value}`).join('|');
  useEffect(() => {
    const at = chipFocus.current;
    if (at === null) return;
    chipFocus.current = null;
    const buttons = bar.current?.querySelectorAll<HTMLElement>('.wa-chip > button:first-child');
    (buttons?.[Math.min(at, buttons.length - 1)] ?? filterButton.current)?.focus();
  }, [chipKeys]);
  // A menu closes on a press outside it, and on Escape; F opens Filter.
  useEffect(() => {
    const onDown = (event: PointerEvent) => {
      if (!menu || !bar.current) return;
      if (!(event.target as Element).closest?.('.wa-menu, .wa-bar [aria-haspopup]')) setMenu(null);
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape' && menu) {
        event.stopPropagation();
        close(menu.kind);
      } else if ((event.key === 'f' || event.key === 'F') && !event.metaKey && !event.ctrlKey && !event.altKey && !isField(event.target) && !menu) {
        event.preventDefault();
        setMenu({ kind: 'filter', sub: null, text: '' });
      }
    };
    document.addEventListener('pointerdown', onDown);
    document.addEventListener('keydown', onKey, true);
    return () => {
      document.removeEventListener('pointerdown', onDown);
      document.removeEventListener('keydown', onKey, true);
    };
  });
  // The field of an opened Filter takes the focus.
  const opened = menu?.kind;
  useEffect(() => {
    if (opened === 'filter') bar.current?.querySelector<HTMLElement>('.wa-menu > input')?.focus();
    if (opened === 'display') bar.current?.querySelector<HTMLElement>('.wa-menu > button')?.focus();
  }, [opened]);

  let filterMenu: ReactNode = null;
  if (menu?.kind === 'filter') {
    const text = menu.text.trim().toLowerCase();
    const matches = text ? values.filter((value) => value.label.toLowerCase().includes(text)) : [];
    const subIndex = SUBS.findIndex(([sub]) => sub === menu.sub);
    filterMenu = (
      <>
        <div className="wa-menu wa-menu-filter" role="menu" aria-label="Filter" onKeyDown={(event) => menuKeys(event, undefined, (button) => button.click())}>
          <input type="text" placeholder="Filter" aria-label="Filter" value={menu.text} onChange={(event) => within({ sub: null, text: event.target.value })} />
          {text ? matches.map((value) => <Check key={`${value.sub}-${value.value}`} value={value} query={query} onQuery={onQuery} />) : SUBS.map(([sub, label]) => (
            <button
              type="button" role="menuitem" key={sub} aria-haspopup="menu" aria-expanded={menu.sub === sub}
              onClick={() => {
                within({ sub, text: '' });
                requestAnimationFrame(() => bar.current?.querySelector<HTMLElement>('.wa-menu-sub > button')?.focus());
              }}
              onPointerEnter={() => within({ sub, text: '' })}
            >
              <span>{label}</span><Icon name="chev" />
            </button>
          ))}
        </div>
        {menu.sub ? (
          <div
            className="wa-menu wa-menu-sub" role="menu" aria-label={SUBS[subIndex][1]} style={{ top: `${36 + 33 + subIndex * 28}px` }}
            onKeyDown={(event) => menuKeys(event, () => {
              within({ sub: null, text: '' });
              requestAnimationFrame(() => bar.current?.querySelectorAll<HTMLElement>('.wa-menu-filter > button')[subIndex]?.focus());
            })}
          >
            {values.filter((value) => value.sub === menu.sub).map((value) => <Check key={value.value} value={value} query={query} onQuery={onQuery} />)}
          </div>
        ) : null}
      </>
    );
  }

  const showChanged = query.show.join() !== DEFAULT_SHOW.join();
  const clear = () => onQuery({ ...query, states: [], parts: [], films: [], q: '' });
  return (
    <section className="wa" aria-label="All">
      <div className={chips.length ? 'wa-bar has-chips' : 'wa-bar'} role="toolbar" aria-label="What the list shows" ref={bar}>
        <div className="ws-seg" role="group" aria-label="Order">
          <button type="button" aria-pressed={query.order === 'build'} onClick={() => onQuery({ ...query, order: 'build' })}>Build order</button>
          <button type="button" aria-pressed={query.order === 'latest'} onClick={() => onQuery({ ...query, order: 'latest' })}>Latest</button>
        </div>
        <button
          type="button" className="wa-ctl" ref={filterButton} aria-haspopup="menu" aria-expanded={menu?.kind === 'filter'} aria-label="Filter"
          onClick={() => (menu?.kind === 'filter' ? close() : setMenu({ kind: 'filter', sub: null, text: '' }))}
        >
          <Icon name="filter" /><span>Filter</span>
        </button>
        {chips.length ? (
          <div className="wa-chips">
            {chips.map((chip, index) => (
              <span className="wa-chip" key={`${chip.sub}-${chip.value}`} data-chip={`${chip.sub}-${chip.value}`}>
                <button type="button" onClick={() => setMenu({ kind: 'filter', sub: chip.sub, text: '', from: `${chip.sub}-${chip.value}` })}>{chip.mark}<span>{chip.label}</span></button>
                <button type="button" className="wa-x" aria-label={`Remove ${chip.label}`} onClick={() => { chipFocus.current = index; onQuery(withValue(query, chip.sub, chip.value, false)); }}><Icon name="x" /></button>
              </span>
            ))}
          </div>
        ) : null}
        {chips.length > 1 ? <button type="button" className="wa-clear" onClick={() => { onQuery({ ...query, states: [], parts: [], films: [] }); filterButton.current?.focus(); }}>Clear</button> : null}
        <span className="wa-gap" />
        {narrowed(query) ? <span className="wa-count">{count.shown} of {count.total}</span> : null}
        <button
          type="button" className="wa-ctl" ref={displayButton} aria-haspopup="menu" aria-expanded={menu?.kind === 'display'} aria-label="Display"
          onClick={() => (menu?.kind === 'display' ? close() : setMenu({ kind: 'display' }))}
        >
          <Icon name="display" /><span>Display</span>{showChanged ? <span className="wa-dot" aria-hidden="true" /> : null}
        </button>
        {filterMenu}
        {menu?.kind === 'display' ? (
          <div className="wa-menu wa-menu-display" role="menu" aria-label="Display" onKeyDown={(event) => menuKeys(event)}>
            {(['feature', 'decision', 'film'] as Show[]).map((show) => {
              const on = query.show.includes(show);
              return (
                <button type="button" role="menuitemcheckbox" aria-checked={on} key={show} onClick={() => onQuery({ ...query, show: (['feature', 'decision', 'film'] as Show[]).filter((one) => (one === show ? !on : query.show.includes(one))) })}>
                  <span>{SHOW_WORDS[show]}</span><i className="wa-check" aria-hidden="true" />
                </button>
              );
            })}
          </div>
        ) : null}
      </div>
      <Strip graph={graph} kept={kept} store={store} onOpen={props.onOpen} />
      <AllList
        sections={sections} graph={graph} plan={plan} ctx={props.ctx} store={store} latest={query.order === 'latest'} flat={Boolean(query.q.trim())}
        folds={props.folds} onFold={props.onFold} more={props.more} onMore={props.onMore} waiting={props.waiting}
        empty={<p className="wa-none">Nothing matches.<button type="button" className="wa-ctl" onClick={clear}>Clear</button></p>}
      />
    </section>
  );
}

/**
 * One pip per feature, in the order they are built, always all of them; those a filter leaves out go faint. A pip
 * pressed brings that step's row into view and gives it the focus. The strip is one stop for the keyboard: the arrows
 * move along it. While a row is pointed at, focused or open, its pip is ringed, the pips it needs and those that wait
 * for it keep their ink, and a comb under them joins the three.
 */
function Strip({ graph, kept, store, onOpen }: { graph: Graph; kept: ReadonlySet<string> | null; store: RelationStore; onOpen: (key: string) => void }) {
  const [stop, setStop] = useState(0);
  const strip = useRef<HTMLElement>(null);
  const comb = useRef<SVGSVGElement>(null);
  useLayoutEffect(() => {
    const node = strip.current;
    const svg = comb.current;
    const ol = node?.querySelector('ol');
    if (!node || !svg || !ol) return;
    let marked: Element[] = [];
    const draw = () => {
      for (const one of marked) {
        one.classList.remove('rel', 'cur');
        if (one instanceof HTMLElement && one.dataset.label) one.setAttribute('aria-label', one.dataset.label);
      }
      marked = [];
      node.classList.remove('relating');
      svg.replaceChildren();
      const active = store.active();
      if (!active) return;
      const key = rowKeyOf(active);
      const { needs, waits } = relationsOf(graph, key);
      const pipOf = (one: string) => ol.querySelector<HTMLElement>(`.wa-sp[data-row="${CSS.escape(one)}"]`);
      const me = pipOf(key);
      const needed = needs.map(pipOf).filter((one): one is HTMLElement => one !== null);
      const waiting = waits.map(pipOf).filter((one): one is HTMLElement => one !== null);
      if (!me && !needed.length && !waiting.length) return;
      node.classList.add('relating');
      if (me) me.classList.add('cur');
      for (const one of [...needed, ...waiting]) one.classList.add('rel');
      // What a screen reader hears of a dot that is part of the relation drawn: one word for its side of it.
      for (const one of needed) one.setAttribute('aria-label', `${one.dataset.label}, needed`);
      for (const one of waiting) one.setAttribute('aria-label', `${one.dataset.label}, waiting`);
      marked = [...(me ? [me] : []), ...needed, ...waiting];
      const box = ol.getBoundingClientRect();
      const x = (one: Element) => {
        const r = one.getBoundingClientRect();
        return Math.round(r.left - box.left + r.width / 2) + 0.5;
      };
      svg.setAttribute('width', String(Math.round(box.width)));
      const add = (tag: string, attrs: Record<string, string | number>) => {
        const el = document.createElementNS('http://www.w3.org/2000/svg', tag);
        for (const [name, value] of Object.entries(attrs)) el.setAttribute(name, String(value));
        svg.append(el);
      };
      const xs = [...(me ? [me] : []), ...needed, ...waiting].map(x);
      const BASE = 22.5;
      if (xs.length > 1) add('path', { d: `M${Math.min(...xs)} ${BASE}H${Math.max(...xs)}` });
      for (const one of needed) {
        const cx = x(one);
        add('path', { d: `M${cx} ${BASE}V17` });
        add('circle', { class: 'solid', cx, cy: 16.5, r: 1.5 });
      }
      for (const one of waiting) {
        const cx = x(one);
        add('path', { d: `M${cx} ${BASE}V17` });
        add('path', { class: 'solid', d: `M${cx - 2.5} 18.5L${cx} 15L${cx + 2.5} 18.5z` });
      }
      if (me) add('path', { d: `M${x(me)} ${BASE}V16` });
    };
    draw();
    return store.subscribe(draw);
  });

  // A dot whose feature the list does not hold opens it where it lives, as a link to it does.
  const go = (key: string) => {
    const item = document.getElementById(key);
    if (!item) {
      onOpen(key);
      return;
    }
    item.scrollIntoView?.({ block: 'center' });
    item.querySelector<HTMLElement>(':scope > .wa-row > .wa-hit')?.focus({ preventScroll: true });
  };
  const onKey = (event: ReactKeyboardEvent<HTMLOListElement>) => {
    const next = event.key === 'ArrowRight' ? stop + 1 : event.key === 'ArrowLeft' ? stop - 1 : event.key === 'Home' ? 0 : event.key === 'End' ? graph.steps.length - 1 : null;
    if (next === null || next < 0 || next >= graph.steps.length) return;
    event.preventDefault();
    setStop(next);
    strip.current?.querySelectorAll<HTMLElement>('.wa-sp')[next]?.focus();
  };
  return (
    <nav className="wa-strip" aria-label="The features, in the order they are built" ref={strip}>
      <ol style={{ '--n': graph.steps.length } as CSSProperties} onKeyDown={onKey}>
        {graph.steps.map((row, index) => {
          const step = index + 1;
          const out = kept !== null && !kept.has(row.key);
          return (
            <li key={row.key}>
              <button
                type="button" className={`wa-sp ${row.status}${out ? ' out' : ''}`} data-row={row.key} tabIndex={index === stop ? 0 : -1}
                aria-label={`${step} ${row.name}, ${PIP_WORDS[row.status]}`} data-label={`${step} ${row.name}, ${PIP_WORDS[row.status]}`} data-tip={`${step}  ${row.name}`}
                onFocus={() => { setStop(index); store.focus(row.key); }} onBlur={() => store.focus(null)}
                onPointerEnter={() => store.point(row.key)} onPointerLeave={() => store.point(null)}
                onClick={() => go(row.key)}
              />
            </li>
          );
        })}
      </ol>
      <svg className="wa-comb" ref={comb} height="28" aria-hidden="true" />
    </nav>
  );
}
