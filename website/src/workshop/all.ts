// The All list's model: what the address asks of it (the view, the order, the filters, the search, what it displays),
// what stands on what, and the list as sections of items to draw. Pure functions of the plan, so the page, a part's
// place and the tests read them the same way.
import { changeLine, shortDate } from './plan';
import type { FilmItem, Plan, Row, Stage, Status } from './plan';

export type View = 'map' | 'all';
export type Order = 'build' | 'latest';
export type Show = 'feature' | 'decision' | 'film';

export const STATES: Status[] = ['undecided', 'decided', 'building', 'built', 'dropped', 'replaced'];
/** The page's words for the states, as the State menu and the chips say them. */
export const STATE_WORDS: Record<Status, string> = { undecided: 'not decided', decided: 'decided', building: 'being built', built: 'built', dropped: 'dropped', replaced: 'replaced' };
export const SHOWS: Show[] = ['feature', 'decision', 'film'];
/** Features and decisions show; films are a row kind of their own, off until chosen. */
export const DEFAULT_SHOW: Show[] = ['feature', 'decision'];
/** The part filter's value for the decisions of the whole product. */
export const PRODUCT = 'product';
/** How many decisions an unfolded feature shows before one line opens the rest. */
export const KIDS_SHOWN = 6;

/** What All shows: its order, its filters, its search and the kinds of row it displays. */
export interface Query {
  order: Order;
  states: Status[];
  parts: string[];
  films: string[];
  show: Show[];
  q: string;
}

export const NO_QUERY: Query = { order: 'build', states: [], parts: [], films: [], show: DEFAULT_SHOW, q: '' };
const PARAMS = ['view', 'order', 'state', 'part', 'film', 'show', 'q'];

const list = (value: string | null): string[] => (value ? value.split(',').map((one) => one.trim()).filter(Boolean) : []);
const pick = <T extends string>(values: string[], allowed: readonly T[]): T[] => allowed.filter((one) => values.includes(one));

/**
 * What the address's query asks: the view (null when it names none; the old `view=order` names none) and All's query.
 * `order` is null when the address does not name one.
 */
export function parseQuery(search: string): { view: View | null; order: Order | null; query: Query } {
  const params = new URLSearchParams(search);
  const view = params.get('view');
  const show = pick(list(params.get('show')), SHOWS);
  const order = params.get('order') === 'latest' ? 'latest' : params.get('order') === 'build' ? 'build' : null;
  return {
    view: view === 'all' ? 'all' : view === 'map' ? 'map' : null,
    order,
    query: {
      order: order ?? 'build',
      states: pick(list(params.get('state')), STATES),
      parts: list(params.get('part')),
      films: list(params.get('film')),
      show: params.has('show') ? show : DEFAULT_SHOW,
      q: params.get('q') ?? '',
    },
  };
}

/** The query of an address for a view and All's query: a default is left out, and the address's other parameters stay. */
export function formatQuery(search: string, view: View, query: Query): string {
  const params = new URLSearchParams(search);
  for (const name of PARAMS) params.delete(name);
  if (view === 'all') {
    params.set('view', 'all');
    if (query.order !== 'build') params.set('order', query.order);
    if (query.states.length) params.set('state', query.states.join(','));
    if (query.parts.length) params.set('part', query.parts.join(','));
    if (query.films.length) params.set('film', query.films.join(','));
    if (query.show.join() !== DEFAULT_SHOW.join()) params.set('show', query.show.join(','));
    if (query.q) params.set('q', query.q);
  }
  const text = params.toString().replace(/%2C/g, ',');
  return text ? `?${text}` : '';
}

/** Whether a filter applies (the search aside). */
export const filtered = (query: Query): boolean => Boolean(query.states.length || query.parts.length || query.films.length);
/** Whether a filter or a search applies: the count shows then. */
export const narrowed = (query: Query): boolean => filtered(query) || Boolean(query.q.trim());

/** What stands on what, among the plan's entries. */
export interface Graph {
  /** The features in the order they are built; a step's number is its place here, from 1. */
  steps: Row[];
  step: Map<string, number>;
  /** The keys of the entries each entry needs. */
  needs: Map<string, string[]>;
  /** The keys of the entries that need each entry. */
  waits: Map<string, string[]>;
  /** The decisions each feature rests on (`restsOn`), in the plan's order. */
  decisions: Map<string, Row[]>;
  /** Every entry of the plan by key, in the plan's order. */
  rows: Map<string, Row>;
}

/**
 * What a feature rests on: the decisions it needs, those they need, and so on. A feature is never followed, so what
 * rests under a needed feature stays there.
 */
function restsOn(feature: string, needs: ReadonlyMap<string, string[]>, rows: ReadonlyMap<string, Row>): Row[] {
  const found = new Set<string>();
  const follow = (key: string) => {
    for (const next of needs.get(key) ?? []) {
      if (found.has(next) || rows.get(next)?.of !== 'decision') continue;
      found.add(next);
      follow(next);
    }
  };
  follow(feature);
  return [...found].map((key) => rows.get(key)!).sort((a, b) => (a.n ?? 0) - (b.n ?? 0));
}

export function graphOf(plan: Plan): Graph {
  const all = [...plan.parts.flatMap((group) => group.rows), ...plan.product].sort((a, b) => (a.n ?? 0) - (b.n ?? 0));
  const rows = new Map(all.map((row) => [row.key, row]));
  const steps = all.filter((row) => row.of === 'feature');
  const step = new Map(steps.map((row, index) => [row.key, index + 1]));
  const needs = new Map<string, string[]>();
  const waits = new Map<string, string[]>();
  const decisions = new Map<string, Row[]>();
  for (const row of all) {
    const own = row.needs.filter((key) => rows.has(key) && key !== row.key);
    needs.set(row.key, own);
    for (const key of own) waits.set(key, [...(waits.get(key) ?? []), row.key]);
  }
  for (const row of steps) decisions.set(row.key, restsOn(row.key, needs, rows));
  return { steps, step, needs, waits, decisions, rows };
}

/**
 * The state a list places some rows by, by key: a row that is open keeps the one it had when it opened, so it keeps its
 * place whatever becomes of it. Every other row is placed by its state, and what is counted always is.
 */
export type Placed = ReadonlyMap<string, Status>;
const AS_IS: Placed = new Map();
const placedState = (row: Row, placed: Placed): Status => placed.get(row.key) ?? row.status;

/**
 * A feature's decisions as it unfolds them: those not decided first, then the others, each in the plan's order. The
 * plan's order puts an entry after what it needs, so none waits for one still to decide under it.
 */
export function kidOrder(rows: Row[], placed: Placed = AS_IS): Row[] {
  const open = (row: Row) => placedState(row, placed) === 'undecided';
  return [...rows.filter(open), ...rows.filter((row) => !open(row))];
}

/** Whether an entry passes the filters and the search. */
export function passes(row: Row, query: Query, placed: Placed = AS_IS): boolean {
  if (query.states.length && !query.states.includes(placedState(row, placed))) return false;
  if (query.parts.length && !query.parts.includes(row.part ?? PRODUCT)) return false;
  if (query.films.length && !row.films.some((id) => query.films.includes(id))) return false;
  const q = query.q.trim().toLowerCase();
  return !q || row.name.toLowerCase().includes(q);
}

/** A feature with its decisions. `kids` are all of them, in the order it unfolds them; `passing` those a filter keeps, or null without a filter. */
export interface FeatureItem {
  type: 'feature';
  key: string;
  row: Row;
  step: number;
  kids: Row[];
  passing: Row[] | null;
  decided: number;
  /** Folding is off when decisions are not displayed. */
  foldable: boolean;
  /** What changed, in Latest. */
  change?: string;
}

/** A decision on its own line (not under a feature). */
export interface DecisionItem {
  type: 'decision';
  key: string;
  row: Row;
  change?: string;
}

/** The decisions of one stage and part that no feature rests on, folded under one row. */
export interface BundleItem {
  type: 'bundle';
  key: string;
  part: string | null;
  title: string;
  kids: Row[];
  passing: Row[] | null;
  decided: number;
}

export interface FilmRowItem {
  type: 'film';
  key: string;
  film: FilmItem;
}

export type Item = FeatureItem | DecisionItem | BundleItem | FilmRowItem;

export interface Section {
  key: string;
  title: string;
  items: Item[];
}

/** How many of some decisions are decided. */
const decidedOf = (rows: Row[]) => rows.filter((row) => row.status === 'decided').length;

function featureItem(row: Row, graph: Graph, query: Query, placed: Placed, foldable: boolean, change?: string): FeatureItem {
  const kids = kidOrder(graph.decisions.get(row.key) ?? [], placed);
  const passing = filtered(query) ? kids.filter((kid) => passes(kid, query, placed)) : null;
  return { type: 'feature', key: row.key, row, step: graph.step.get(row.key) ?? 0, kids, passing, decided: decidedOf(kids), foldable: foldable && kids.length > 0, ...(change ? { change } : {}) };
}

/** The title of a part, or "The whole product". */
export const partTitle = (plan: Plan, part: string | null): string => (part ? plan.parts.find((group) => group.part.id === part)?.part.title ?? part : 'The whole product');

/**
 * All's list. In build order: the stages by their titles, each starting with its decisions that no feature rests on,
 * one foldable row per part, then its features, each with the decisions it rests on folded under it. A filter keeps a
 * feature when it or one of its decisions passes. A search, and Latest, list every match on its own line, Latest newest
 * change first under a title per day.
 */
export function allSections(plan: Plan, graph: Graph, stages: Stage[], query: Query, placed: Placed = AS_IS): Section[] {
  const listed = (row: Row) => passes(row, query, placed);
  const sections: Section[] = [];
  const features = query.show.includes('feature');
  const decisions = query.show.includes('decision');
  if (query.show.includes('film') && !query.q.trim()) {
    const films = plan.films.filter((film) => (!query.films.length || query.films.includes(film.id)) && (!query.parts.length || film.parts.some((id) => query.parts.includes(id))));
    if (films.length) sections.push({ key: 'films', title: 'Films', items: films.map((film) => ({ type: 'film', key: `film-${film.id}`, film })) });
  }
  const entries = [...graph.rows.values()].filter((row) => (row.of === 'feature' ? features : decisions));

  if (query.order === 'latest') {
    const dated = entries.filter((row) => row.history && listed(row)).sort((a, b) => Date.parse(b.history!.date) - Date.parse(a.history!.date) || (a.n ?? 0) - (b.n ?? 0));
    let current: Section | null = null;
    for (const row of dated) {
      const day = shortDate(row.history!.date);
      if (!current || current.title !== day) sections.push((current = { key: `day-${row.history!.date.slice(0, 10)}`, title: day, items: [] }));
      const change = changeLine(row.history!.what, row.history!.agents);
      current.items.push(row.of === 'feature' ? featureItem(row, graph, query, placed, false, change) : { type: 'decision', key: row.key, row, change });
    }
    return sections;
  }

  const stageTitle = new Map(stages.map((stage) => [stage.id, stage.title]));
  const inStages = (rows: Row[], make: (row: Row) => Item) => {
    let current: Section | null = null;
    for (const row of rows) {
      const stage = row.stage ?? '';
      if (!current || current.key !== `stage-${stage}`) sections.push((current = { key: `stage-${stage}`, title: stageTitle.get(stage) ?? '', items: [] }));
      current.items.push(make(row));
    }
  };

  // A search lists every match on its own line, in build order.
  if (query.q.trim()) {
    inStages(entries.filter(listed), (row) => (row.of === 'feature' ? featureItem(row, graph, query, placed, false) : { type: 'decision', key: row.key, row }));
    return sections;
  }

  if (!features) {
    inStages(entries.filter(listed), (row) => ({ type: 'decision', key: row.key, row }));
    return sections;
  }

  const kept = graph.steps.map((row) => featureItem(row, graph, query, placed, decisions)).filter((item) => listed(item.row) || (decisions && item.passing?.length));
  const loose = decisions ? looseOf(graph) : [];
  for (const stage of stages) {
    const items = [
      ...bundlesOf(plan, stage.id, loose.filter((row) => row.stage === stage.id), query, placed),
      ...kept.filter((item) => item.row.stage === stage.id),
    ];
    if (items.length) sections.push({ key: `stage-${stage.id}`, title: stage.title, items });
  }
  return sections;
}

/** The plan's decisions that no feature rests on. */
function looseOf(graph: Graph): Row[] {
  const held = heldBy(graph, graph.steps);
  return [...graph.rows.values()].filter((row) => row.of === 'decision' && !held.has(row.key));
}

/** One foldable row per part for a stage's decisions that no feature rests on; under a filter, those where one passes. */
function bundlesOf(plan: Plan, stage: string, rows: Row[], query: Query, placed: Placed): BundleItem[] {
  const bundles: BundleItem[] = [];
  for (const part of [...plan.parts.map((group) => group.part.id), null]) {
    const kids = kidOrder(rows.filter((row) => row.part === part), placed);
    if (!kids.length) continue;
    const passing = filtered(query) ? kids.filter((kid) => passes(kid, query, placed)) : null;
    if (passing && !passing.length) continue;
    // A stage's id and a part's hold no dot, so no two rows share a key.
    bundles.push({ type: 'bundle', key: `bundle-${stage}.${part ?? PRODUCT}`, part, title: partTitle(plan, part), kids, passing, decided: decidedOf(kids) });
  }
  return bundles;
}

/** How many entries pass the filters and the search, of how many the plan holds. */
export function countOf(graph: Graph, query: Query): { shown: number; total: number } {
  let shown = 0;
  for (const row of graph.rows.values()) if (passes(row, query)) shown += 1;
  return { shown, total: graph.rows.size };
}

/** The keys of the features the list shows, for the strip: the others go faint. Null when nothing is filtered. */
export function stripKept(sections: Section[], query: Query): ReadonlySet<string> | null {
  if (!narrowed(query)) return null;
  const keys = new Set<string>();
  for (const section of sections) for (const item of section.items) if (item.type === 'feature') keys.add(item.row.key);
  return keys;
}

/** What an entry needs and what waits for it, among the entries of the plan. For a decision, what waits is the features that need it. */
export function relationsOf(graph: Graph, key: string): { needs: string[]; waits: string[] } {
  const row = graph.rows.get(key);
  if (!row) return { needs: [], waits: [] };
  const waits = graph.waits.get(key) ?? [];
  return { needs: graph.needs.get(key) ?? [], waits: row.of === 'decision' ? waits.filter((other) => graph.rows.get(other)?.of === 'feature') : waits };
}

/** The keys of the decisions some of these features rest on. */
const heldBy = (graph: Graph, features: Row[]) => new Set(features.flatMap((row) => (graph.decisions.get(row.key) ?? []).map((kid) => kid.key)));

/** The decisions a part's place lists flat: the part's decisions that none of its own features rests on. */
export function looseOfPart(graph: Graph, rows: Row[]): Row[] {
  const held = heldBy(graph, rows.filter((row) => row.of === 'feature'));
  return rows.filter((row) => row.of === 'decision' && !held.has(row.key));
}

/** The film a part keeps a control for in its place: one whose entries are all of that part. */
export const partFilm = (plan: Plan, part: string): FilmItem | undefined => plan.films.find((film) => film.parts.length === 1 && film.parts[0] === part);

/** The instance key of a row drawn under a feature or a part's bundle; a row on its own line is its own key. */
export const underKey = (rowKey: string, holder: string) => `${rowKey}@${holder}`;
/** The entry an instance key names. */
export const rowKeyOf = (instance: string) => instance.split('@')[0];

/** Features of some rows, each with the decisions it rests on folded under it (or none, `foldable` false). */
export function featureItemsOf(graph: Graph, rows: Row[], foldable = true, placed: Placed = AS_IS): Item[] {
  return rows.filter((row) => row.of === 'feature').map((row) => {
    const kids = foldable ? kidOrder(graph.decisions.get(row.key) ?? [], placed) : [];
    return { type: 'feature', key: row.key, row, step: graph.step.get(row.key) ?? 0, kids, passing: null, decided: decidedOf(kids), foldable: kids.length > 0 };
  });
}

const decisionItemsOf = (rows: Row[]): Item[] => rows.filter((row) => row.of === 'decision').map((row) => ({ type: 'decision', key: row.key, row }));

/** A part's place: its features with the decisions they rest on, then its decisions that none of its features rests on. */
export const partSections = (graph: Graph, rows: Row[], placed: Placed = AS_IS): Section[] => [
  { key: 'features', title: 'Features', items: featureItemsOf(graph, rows, true, placed) },
  { key: 'decisions', title: 'Decisions', items: decisionItemsOf(looseOfPart(graph, rows)) },
].filter((section) => section.items.length);

/** Product's place: its decisions, each on its own line. */
export const productSections = (rows: Row[]): Section[] => [{ key: 'product', title: '', items: decisionItemsOf(rows) }].filter((section) => section.items.length);

/** A film's place: the entries it shows, each on its own line. */
export const filmSections = (graph: Graph, rows: Row[]): Section[] => [
  { key: 'features', title: 'Features', items: featureItemsOf(graph, rows, false) },
  { key: 'decisions', title: 'Decisions', items: decisionItemsOf(rows) },
].filter((section) => section.items.length);

/** Where an entry stands in a list: its own line, or under the first feature or bundle that holds it (which must unfold, and may need its "more"). */
export function locate(sections: Section[], key: string, more: ReadonlySet<string>): { instance: string; holder: string | null; more: boolean } | null {
  for (const section of sections) {
    for (const item of section.items) {
      if ((item.type === 'feature' || item.type === 'decision') && item.key === key) return { instance: key, holder: null, more: false };
    }
  }
  for (const section of sections) {
    for (const item of section.items) {
      if ((item.type !== 'feature' || !item.foldable) && item.type !== 'bundle') continue;
      const shown = item.passing?.length ? item.passing : item.kids;
      const at = shown.findIndex((kid) => kid.key === key);
      if (at !== -1) return { instance: underKey(key, item.key), holder: item.key, more: at >= KIDS_SHOWN && !more.has(item.key) };
    }
  }
  return null;
}
