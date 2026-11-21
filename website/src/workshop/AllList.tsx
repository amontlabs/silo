// All's rows: one line per entry, keys first; a feature's decisions fold under it. The same rows draw All, a part's
// place, Product and a film's place. Pointing at a row, focusing it or opening it draws its relations in the rail at
// the list's left: a dot on each row it needs, an arrowhead on each row that waits for it, one line joining them.
import { memo, useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { CSSProperties, KeyboardEvent, MouseEvent, ReactNode } from 'react';
import { KIDS_SHOWN, partTitle, relationsOf, rowKeyOf, underKey } from './all';
import type { FeatureItem, Graph, Item, Section } from './all';
import { Panel, pipClass } from './Entry';
import type { EntryContext } from './Entry';
import { BEHIND_TIP, FILM_TIP, FilmBox, Icon } from './Films';
import type { FilmItem, Plan, Row } from './plan';
import type { RelationStore } from './relation';

/** A pip's tooltip and accessible name. */
export const PIP_WORDS: Record<Row['status'], string> = { undecided: 'not decided', decided: 'decided', building: 'being built', built: 'built', dropped: 'dropped', replaced: 'replaced' };

export interface ListProps {
  sections: Section[];
  graph: Graph;
  plan: Plan;
  ctx: EntryContext;
  store: RelationStore;
  /** The part whose place this is: its features leave the part column empty. */
  inPart?: string | null;
  /** Every row has its line (Latest, a search, a film's place). */
  flat?: boolean;
  latest?: boolean;
  /** Whether a feature or a part's bundle is unfolded, by key; one not named follows the filter. */
  folds: ReadonlyMap<string, boolean>;
  onFold: (key: string, open: boolean) => void;
  /** The folds whose "N more" was pressed. */
  more: ReadonlySet<string>;
  onMore: (key: string) => void;
  /** Whether the votes are still to come. */
  waiting: boolean;
  /** What the list shows when it has no row. */
  empty?: ReactNode;
  className?: string;
}

const isUnfolded = (item: { key: string; passing: Row[] | null }, folds: ReadonlyMap<string, boolean>) => folds.get(item.key) ?? Boolean(item.passing?.length);

/** The mark after the name of an entry a film shows; its frame is dashed when a film is behind on it. */
function FilmMark({ row }: { row: Row }) {
  if (!row.films.length) return null;
  const tip = row.filmBehind ? BEHIND_TIP : FILM_TIP;
  return <span className={row.filmBehind ? 'wa-film behind' : 'wa-film'} role="img" aria-label={tip} data-tip={tip}><Icon name="film" /></span>;
}

function Meter({ decided, total }: { decided: number; total: number }) {
  if (!total) return <span className="wa-meter" />;
  const says = `${decided} of ${total} decisions decided`;
  return (
    <span className="wa-meter" role="img" aria-label={says} data-tip={says}>
      <i style={{ '--p': (decided / total).toFixed(3) } as CSSProperties} /><span>{decided}/{total}</span>
    </span>
  );
}

function Side({ row, waiting }: { row: Row; waiting: boolean }) {
  if (waiting) return <span className="wa-side" aria-busy="true"><span className="wa-ph wa-ph-side" aria-hidden="true" /></span>;
  return <span className="wa-side">{row.up ? <span className="ws-late-s">👍 {row.up}</span> : null}</span>;
}

/** What a row says to a screen reader: its name first, then its step and its state. */
const spokenRow = (row: Row, step?: number) => [row.name, step ? `step ${step}` : '', PIP_WORDS[row.status]].filter(Boolean).join(', ');

export function AllList(props: ListProps) {
  const { sections, graph, store, flat = false, latest = false, className = '', empty = null } = props;
  const list = useRef<HTMLDivElement>(null);
  const rail = useRef<SVGSVGElement>(null);

  // The relations of the active row: the rows it needs and the rows that wait for it light up, and the rail joins them.
  useLayoutEffect(() => {
    const node = list.current;
    const svg = rail.current;
    if (!node || !svg) return;
    let lit: Element[] = [];
    const draw = () => {
      for (const one of lit) one.classList.remove('lit');
      lit = [];
      svg.replaceChildren();
      const active = store.active();
      const item = active ? document.getElementById(active) : null;
      const current = item && node.contains(item) ? item.querySelector<HTMLElement>(':scope > .wa-row') : null;
      if (!current || !active) return;
      const { needs, waits } = relationsOf(graph, rowKeyOf(active));
      // Every drawing of a related entry is lit: a decision several unfolded features share shows under each.
      const rowsOf = (key: string) => [...node.querySelectorAll<HTMLElement>(`.wa-row[data-row="${CSS.escape(key)}"]`)];
      const up = needs.flatMap(rowsOf);
      const down = waits.flatMap(rowsOf);
      const upShown = needs.filter((key) => rowsOf(key).length).length;
      const downShown = waits.filter((key) => rowsOf(key).length).length;
      if (!up.length && !down.length && !needs.length && !waits.length) return;
      for (const one of [...up, ...down]) one.classList.add('lit');
      lit = [...up, ...down];
      const box = node.getBoundingClientRect();
      const mid = (one: Element) => {
        const r = one.getBoundingClientRect();
        return Math.round(r.top - box.top + Math.min(r.height, 36) / 2) + 0.5;
      };
      svg.setAttribute('height', String(Math.round(box.height)));
      const X = 11.5;
      const y0 = mid(current);
      const ys = [...up, ...down].map(mid);
      const top = Math.min(y0, ...ys);
      const bottom = Math.max(y0, ...ys);
      const add = (tag: string, attrs: Record<string, string | number>) => {
        const el = document.createElementNS('http://www.w3.org/2000/svg', tag);
        for (const [name, value] of Object.entries(attrs)) el.setAttribute(name, String(value));
        svg.append(el);
      };
      add('path', { d: `M${X} ${top}V${bottom}` });
      // More out of sight (folded, filtered, or not in this list): the line goes on, dotted, to the list's edge.
      if (needs.length > upShown) add('path', { class: 'far', d: `M${X} ${top}V0` });
      if (waits.length > downShown) add('path', { class: 'far', d: `M${X} ${bottom}V${Math.round(box.height)}` });
      for (const one of up) {
        const y = mid(one);
        add('path', { d: `M${X} ${y}H22` });
        add('circle', { class: 'solid', cx: X, cy: y, r: 2.5 });
      }
      for (const one of down) {
        const y = mid(one);
        add('path', { d: `M${X} ${y}H18` });
        add('path', { class: 'solid', d: `M17 ${y - 3.5}L22 ${y}L17 ${y + 3.5}z` });
      }
      add('path', { d: `M${X} ${y0}H22` });
      add('circle', { class: 'node', cx: X, cy: y0, r: 3.5 });
    };
    draw();
    const off = store.subscribe(draw);
    // Rows open, close and fold under the marks: they follow.
    const observer = typeof ResizeObserver === 'function' ? new ResizeObserver(() => draw()) : null;
    observer?.observe(node);
    return () => {
      off();
      observer?.disconnect();
      for (const one of lit) one.classList.remove('lit');
    };
  });

  // The pointer on a row draws its marks after a rest; leaving the list takes them away.
  const onOver = (event: { target: EventTarget }) => {
    const row = (event.target as Element).closest?.('.wa-row[data-row]');
    const item = row?.parentElement;
    store.point(item?.id ?? null);
  };
  useEffect(() => () => store.point(null), [store]);

  // Up and down move between rows, right unfolds, left folds (or goes to the feature that holds the row).
  const onKey = (event: KeyboardEvent<HTMLDivElement>) => {
    const target = event.target as HTMLElement;
    if (!target.matches('.wa-hit, .wa-caret, .wa-row.more')) return;
    const node = list.current;
    if (!node) return;
    const item = target.closest<HTMLElement>('.wa-item');
    if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      const all = [...node.querySelectorAll<HTMLElement>('.wa-hit, .wa-row.more')];
      const at = all.indexOf(target.matches('.wa-caret') ? item?.querySelector<HTMLElement>('.wa-hit') ?? target : target);
      const next = all[at + (event.key === 'ArrowDown' ? 1 : -1)];
      if (next) {
        event.preventDefault();
        next.focus();
        next.scrollIntoView?.({ block: 'nearest' });
      }
    } else if (event.key === 'ArrowRight' || event.key === 'ArrowLeft') {
      const caret = item?.querySelector<HTMLButtonElement>(':scope > .wa-row > .wa-caret');
      const unfolded = caret?.getAttribute('aria-expanded') === 'true';
      if (event.key === 'ArrowRight' && caret && !unfolded) {
        event.preventDefault();
        caret.click();
      } else if (event.key === 'ArrowLeft') {
        if (caret && unfolded) {
          event.preventDefault();
          caret.click();
        } else {
          const holder = item?.parentElement?.closest<HTMLElement>('.wa-item')?.querySelector<HTMLElement>(':scope > .wa-row > .wa-hit');
          if (holder) {
            event.preventDefault();
            holder.focus();
          }
        }
      }
    }
  };

  const classes = ['wa-list', flat || latest ? 'flat' : '', latest ? 'latest' : '', props.inPart ? 'in-part' : '', className].filter(Boolean).join(' ');
  const hasRows = sections.some((section) => section.items.length);
  return (
    <div
      className={classes} ref={list} onKeyDown={onKey}
      onPointerOver={onOver} onPointerLeave={() => store.point(null)}
      onFocus={(event) => { if ((event.target as Element).matches('.wa-hit')) store.focus((event.target as Element).closest('.wa-item')?.id ?? null); }}
      onBlur={() => store.focus(null)}
    >
      <svg className="wa-rail" ref={rail} width="32" height="0" aria-hidden="true" />
      {hasRows ? sections.map((section) => (
        <section key={section.key} aria-label={section.title || undefined}>
          {section.title ? <h3 className="wa-grp">{section.title}</h3> : null}
          {section.items.map((item) => <ItemView key={item.key} item={item} props={props} />)}
        </section>
      )) : empty}
    </div>
  );
}

const keysOf = (rows: Row[] | null) => (rows ? rows.map((row) => row.key).join() : null);

/**
 * Whether an item draws the same as before: the same entry and state, closed, folded, in a list that shows the same.
 * A row that is open or unfolded always renders again (its panel and its decisions follow the page).
 */
function sameItem(prev: { item: Item; props: ListProps }, next: { item: Item; props: ListProps }): boolean {
  const a = prev.item;
  const b = next.item;
  const p = prev.props;
  const n = next.props;
  if (a.type !== b.type || a.key !== b.key || p.waiting !== n.waiting || p.inPart !== n.inPart || p.flat !== n.flat || p.latest !== n.latest || p.plan.films !== n.plan.films) return false;
  if (a.type === 'film' && b.type === 'film') return a.film === b.film;
  if (a.type === 'decision' && b.type === 'decision') return a.row === b.row && a.change === b.change && !p.ctx.open.has(a.row.key) && !n.ctx.open.has(b.row.key);
  if (a.type === 'feature' && b.type === 'feature') {
    return a.row === b.row && a.step === b.step && a.decided === b.decided && a.change === b.change && a.foldable === b.foldable
      && keysOf(a.kids) === keysOf(b.kids) && keysOf(a.passing) === keysOf(b.passing)
      && !p.ctx.open.has(a.row.key) && !n.ctx.open.has(b.row.key) && !isUnfolded(a, p.folds) && !isUnfolded(b, n.folds);
  }
  if (a.type === 'bundle' && b.type === 'bundle') {
    return a.title === b.title && a.decided === b.decided && keysOf(a.kids) === keysOf(b.kids) && keysOf(a.passing) === keysOf(b.passing) && !isUnfolded(a, p.folds) && !isUnfolded(b, n.folds);
  }
  return false;
}

const ItemView = memo(ItemDrawn, sameItem);

function ItemDrawn({ item, props }: { item: Item; props: ListProps }) {
  switch (item.type) {
    case 'feature': return <FeatureView item={item} props={props} />;
    case 'decision': return <DecisionView row={item.row} instance={item.key} props={props} flat change={item.change} />;
    case 'bundle': return <Fold holder={item.key} part={item.part} kids={item.kids} passing={item.passing} props={props} head={(unfolded, toggle) => (
      <div className="wa-row bundle" data-fold={item.key}>
        <button type="button" className="wa-hit" aria-expanded={unfolded} aria-label={`${item.title}, ${item.kids.length} ${item.kids.length === 1 ? 'decision' : 'decisions'}`} onClick={toggle} />
        <span className={unfolded ? 'wa-caret on' : 'wa-caret'} aria-hidden="true" />
        <span className="wa-name"><span data-cut="">{item.title}</span></span>
        <Meter decided={item.decided} total={item.kids.length} />
      </div>
    )} />;
    case 'film': return <FilmRow film={item.film} instance={item.key} props={props} />;
  }
}

/** A row's own line and its panel; a press anywhere on the line opens or closes it. */
function Line({ row, instance, props, cls, label, children, lead }: { row: Row; instance: string; props: ListProps; cls: string; label: string; children: ReactNode; lead?: ReactNode }) {
  const { ctx } = props;
  const open = ctx.open.has(row.key);
  const toggle = (event: MouseEvent) => {
    // A click with a modifier key on a link is the browser's.
    if ((event.target as Element).closest('a, .wa-caret')) return;
    // A press on a cell gives the line's button the focus, as a press on the button does, so the arrows go on from here.
    // A pointer's press draws no ring: only the keyboard's focus does.
    const hit = (event.currentTarget as HTMLElement).querySelector<HTMLElement>(':scope > .wa-hit');
    if (hit && document.activeElement !== hit) hit.focus({ preventScroll: true, focusVisible: event.detail === 0 } as FocusOptions);
    ctx.onToggle(row, instance);
  };
  return (
    <>
      <div className={cls} data-row={row.key} onClick={toggle} onPointerEnter={() => ctx.w.prefetch(row)} onPointerDown={() => ctx.w.prefetch(row)}>
        <button
          type="button" className="wa-hit" aria-expanded={open} aria-controls={`${instance}-more`}
          onFocus={() => ctx.w.prefetch(row)} onTouchStart={() => ctx.w.prefetch(row)}
        ><span className="sr-only">{label}</span></button>
        {children}
      </div>
      <LazyPanel row={row} open={open} ctx={ctx} id={`${instance}-more`} lead={lead} />
    </>
  );
}

/**
 * A row's panel, mounted the first time the row opens and kept after, so a list of hundreds of rows mounts none until
 * one opens. Mounted by an opening, it is laid out closed once and then opens, so it grows as a panel that was there
 * closed does. Open in the first frame (an address), it is drawn open at once.
 */
function LazyPanel({ row, open, ctx, id, lead }: { row: Row; open: boolean; ctx: EntryContext; id: string; lead?: ReactNode }) {
  const [ever, setEver] = useState(open);
  const [armed, setArmed] = useState(open);
  if (open && !ever) setEver(true);
  // Drawn closed, its style is read once, so the browser has a closed panel to grow from; then it opens, before paint.
  useLayoutEffect(() => {
    if (!ever || armed) return;
    document.getElementById(id)?.getBoundingClientRect();
    setArmed(true);
  }, [ever, armed, id]);
  if (!ever) return <div className="ws-more" id={id} aria-hidden="true" />;
  return <Panel row={row} open={open && armed} ctx={ctx} id={id} vote lead={lead} />;
}

function filmLead(row: Row, props: ListProps): ReactNode {
  const film = row.films.length ? props.plan.films.find((one) => one.id === row.films[0]) : undefined;
  if (!film) return undefined;
  return <FilmBox film={film} onEntries={() => props.ctx.onFilmEntries?.(film.id)} />;
}

function FeatureView({ item, props }: { item: FeatureItem; props: ListProps }) {
  const { row } = item;
  const open = props.ctx.open.has(row.key);
  const gone = row.status === 'dropped' || row.status === 'replaced';
  const body = (unfolded: boolean | null, toggle: (() => void) | null) => (
    <div className={['wa-item f', open ? 'open' : '', gone ? 'gone' : ''].filter(Boolean).join(' ')} id={item.key}>
      <Line row={row} instance={item.key} props={props} cls="wa-row" label={spokenRow(row, item.step)} lead={open ? filmLead(row, props) : undefined}>
        {toggle ? <button type="button" className="wa-caret" aria-expanded={Boolean(unfolded)} aria-label={`Its decisions, ${item.kids.length}`} onClick={(event) => { event.stopPropagation(); toggle(); }} /> : null}
        <span className="wa-n">{item.step}</span>
        <i className={pipClass(row)} role="img" aria-label={PIP_WORDS[row.status]} data-tip={pipTip(row)} />
        <span className="wa-name"><span data-cut="">{row.name}</span><FilmMark row={row} /></span>
        {item.change !== undefined ? <span className="wa-change">{item.change}</span> : <Meter decided={item.decided} total={item.kids.length} />}
        <span className="wa-part">{props.inPart && props.inPart === row.part ? '' : partTitle(props.plan, row.part)}</span>
        <Side row={row} waiting={props.waiting} />
      </Line>
      {unfolded ? <Kids holder={item.key} part={row.part} kids={item.kids} passing={item.passing} props={props} /> : null}
    </div>
  );
  if (!item.foldable) return body(null, null);
  return <Fold holder={item.key} part={row.part} kids={item.kids} passing={item.passing} props={props} render={body} />;
}

const pipTip = (row: Row) => PIP_WORDS[row.status];

/** Something that folds its decisions under it: a feature, or the decisions of a part that no feature needs. */
function Fold({ holder, part, kids, passing, props, render, head }: {
  holder: string; part: string | null; kids: Row[]; passing: Row[] | null; props: ListProps;
  render?: (unfolded: boolean, toggle: () => void) => ReactNode;
  head?: (unfolded: boolean, toggle: () => void) => ReactNode;
}) {
  const unfolded = isUnfolded({ key: holder, passing }, props.folds);
  const toggle = () => props.onFold(holder, !unfolded);
  if (render) return <>{render(unfolded, toggle)}</>;
  return (
    <div className="wa-item f bundle" id={holder}>
      {head?.(unfolded, toggle)}
      {unfolded ? <Kids holder={holder} part={part} kids={kids} passing={passing} props={props} /> : null}
    </div>
  );
}

/** The decisions under a feature or a part's bundle: those a filter keeps, or all; six, then one line opens the rest. */
function Kids({ holder, part, kids, passing, props }: { holder: string; part: string | null; kids: Row[]; passing: Row[] | null; props: ListProps }) {
  const shown = passing?.length ? passing : kids;
  const all = props.more.has(holder);
  const visible = all ? shown : shown.slice(0, KIDS_SHOWN);
  const rest = shown.length - visible.length;
  // "N more" pressed: the focus goes to the first decision it revealed.
  const revealed = useRef(false);
  useEffect(() => {
    if (!all || !revealed.current) return;
    revealed.current = false;
    const first = shown[KIDS_SHOWN];
    if (first) document.getElementById(underKey(first.key, holder))?.querySelector<HTMLElement>(':scope > .wa-row > .wa-hit')?.focus();
  }, [all, shown, holder]);
  return (
    <div className="wa-kids">
      {visible.map((kid) => <DecisionView key={kid.key} row={kid} instance={underKey(kid.key, holder)} props={props} holderPart={part} />)}
      {rest > 0 ? <button type="button" className="wa-row more" onClick={() => { revealed.current = true; props.onMore(holder); }}><span>{rest} more</span></button> : null}
    </div>
  );
}

function DecisionView({ row, instance, props, flat = false, change, holderPart }: { row: Row; instance: string; props: ListProps; flat?: boolean; change?: string; holderPart?: string | null }) {
  const open = props.ctx.open.has(row.key);
  const gone = row.status === 'dropped' || row.status === 'replaced';
  const part = flat ? (props.inPart && props.inPart === row.part ? '' : partTitle(props.plan, row.part)) : row.part !== holderPart ? partTitle(props.plan, row.part) : '';
  return (
    <div className={['wa-item d', flat ? 'flat' : '', open ? 'open' : '', gone ? 'gone' : ''].filter(Boolean).join(' ')} id={instance}>
      <Line row={row} instance={instance} props={props} cls="wa-row" label={spokenRow(row)} lead={open ? filmLead(row, props) : undefined}>
        <i className={pipClass(row)} role="img" aria-label={PIP_WORDS[row.status]} data-tip={pipTip(row)} />
        <span className="wa-name"><span data-cut="">{row.name}</span><FilmMark row={row} /></span>
        {change !== undefined ? <span className="wa-change">{change}</span> : null}
        <span className="wa-part">{part}</span>
        <Side row={row} waiting={props.waiting} />
      </Line>
    </div>
  );
}

/** A film among the rows (Display, Films): opened, it shows the film and a way to list its entries. */
function FilmRow({ film, instance, props }: { film: FilmItem; instance: string; props: ListProps }) {
  const [open, setOpen] = useState(false);
  const toggle = () => setOpen((now) => !now);
  return (
    <div className={['wa-item f film', open ? 'open' : ''].filter(Boolean).join(' ')} id={instance}>
      <div className="wa-row" onClick={(event) => { if (!(event.target as Element).closest('.wa-hit')) toggle(); }}>
        <button type="button" className="wa-hit" aria-expanded={open} aria-controls={`${instance}-more`} onClick={toggle}><span className="sr-only">{film.title}, film</span></button>
        <span className={film.behind.length ? 'wa-film behind wa-film-pip' : 'wa-film wa-film-pip'} role="img" aria-label={film.behind.length ? BEHIND_TIP : 'Film'} data-tip={film.behind.length ? BEHIND_TIP : undefined}><Icon name="film" /></span>
        <span className="wa-name"><span data-cut="">{film.title}</span></span>
        <span className="wa-part">{film.rows.length} entries</span>
      </div>
      {open ? (
        <div className="wa-open" id={`${instance}-more`}>
          <FilmBox film={film} onEntries={() => props.ctx.onFilmEntries?.(film.id)} />
        </div>
      ) : null}
    </div>
  );
}
