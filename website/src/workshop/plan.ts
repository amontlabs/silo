// The Workshop's model: the plan built into the page, merged with the live state from GitHub. An entry's status
// comes from its file alone, through the plan's model (tools/plan/model); this adds the rows, the parts' counts
// and states, and the words the page uses for them.
import { entryStatus as modelStatus } from '../../../tools/plan/model/index.js';
import type { Verdict } from '../../../tools/plan/model/status.js';
import type { PageLayout } from './map';

export type { Verdict };

/**
 * A row's status, in the page's words: not decided, decided, being built, built. A taken feature is decided until its
 * build starts; a taken decision is decided, or replaced once a later decision has taken its place: a replaced
 * decision is no longer in force.
 */
export type Status = 'built' | 'building' | 'decided' | 'undecided' | 'dropped' | 'replaced';

/** A decision or a feature, as the page holds it. */
export interface Entry {
  id: string;
  kind: 'decision' | 'feature';
  /** The part it belongs to; null for a decision of the whole product. */
  part: string | null;
  stage: string;
  /** Place in order.json, from 1. */
  position: number;
  /** A feature's `name`, a decision's `asks`. */
  name: string;
  /** A feature's `what`, a decision's `answer` (null while it has none). */
  what: string | null;
  needs: string[];
  verdict: Verdict | null;
  /** Who wrote the verdict when it was the agents (`"by": "agents"`); null for the owner, and for no verdict. */
  by: 'agents' | null;
  /** A decision's `replaces`: the ids of the decisions it takes the place of. */
  replaces: string[];
  /** A decision's `replaced_by`: the id of the decision that replaced it, written by `plan take`. */
  replacedBy: string | null;
  /** The day its build started, `YYYY-MM-DD`. */
  started: string | null;
  /** The day it was built, `YYYY-MM-DD`. */
  built: string | null;
  /** Whether it has notes; their text is a chunk of its own (`virtual:workshop-notes`), loaded on demand. */
  notes: boolean;
  /** Whether it has anything to show from `Facts` (the chunk `virtual:workshop-facts`), given its verdict. */
  more: boolean;
  /** What a closed History line says: its newest change and how many it has; null with no change. The changes themselves are `Histories`. */
  history: HistorySummary | null;
}

/** What the history of an entry names (tools/plan/history `WHAT`), never the name of a key of its file. */
export type ChangeName = 'added' | 'moved' | 'text' | 'sets' | 'needs' | 'keeps' | 'surfaces' | 'suggest' | 'notes' | 'taken' | 'dropped' | 'cleared' | 'replaced' | 'started' | 'built';

/** A title and a body, as an entry's text before or after a change; a part is null when the entry has none. */
export interface ChangeText {
  title: string | null;
  body: string | null;
}

/** One change of an entry, as a commit made it. */
export interface Change {
  /** The full hash. */
  commit: string;
  /** The committer date, `2026-11-20T12:00:00Z`. */
  date: string;
  what: ChangeName[];
  /** With `text`: the title and body before and after. */
  text?: { before: ChangeText; after: ChangeText };
  /** With `replaced`: the id of the decision that replaced it. */
  by?: string;
  /** With `taken` or `dropped`: the agents wrote the verdict, not the owner. */
  agents?: true;
}

/** The newest change of an entry and the number of its changes: all a closed History line needs. */
export interface HistorySummary {
  date: string;
  what: ChangeName[];
  /** Whether the newest change is a verdict the agents wrote. */
  agents?: true;
  count: number;
}

/** The changes of every entry that has some, newest first, by entry id: a chunk of its own (`virtual:workshop-history`), loaded when a History is first opened. */
export type Histories = Record<string, Change[]>;

export type StatementKind = 'word' | 'principle' | 'property';

/** A statement of the plan's lists: a word, a principle or a property. */
export interface Statement {
  id: string;
  kind: StatementKind;
  /** A word's own spelling; null for the others. */
  word: string | null;
  /** A word's `means`, a principle's or a property's `says`. */
  text: string;
  /** A principle's question; null for the others. */
  asks: string | null;
}

/** What an entry's file holds beyond what the rows need to be drawn. */
export interface EntryFacts {
  suggest?: { verdict: Verdict; why: string };
  /** What a yes would write into the lists, as statements. */
  sets?: Statement[];
  removes?: string[];
  adds?: string[];
  keeps?: string[];
  surfaces?: { app?: string; cli?: string };
}

/** The statements in force and what each entry says of them: a chunk of its own (`virtual:workshop-facts`), loaded when a row is first opened. */
export interface Facts {
  entries: Record<string, EntryFacts>;
  /** Every statement in force, by id. */
  statements: Record<string, Statement>;
}

/** The notes of the entries that have some, Markdown, by entry id. */
export type Notes = Record<string, string>;

export interface Stage {
  id: string;
  title: string;
  line: string;
}

export interface Part {
  id: string;
  title: string;
  line: string;
  /** The parts it depends on, in the order of the map: one of its entries names an entry of each in `needs`, and that part starts before this one in the plan's order. */
  dependsOn: string[];
}

export interface BuildData {
  parts: Part[];
  stages: Stage[];
  /** Every decision and feature, in the order of order.json. */
  entries: Entry[];
  /** Where the parts sit on the map: Silo's own (map.ts), or the example the tests draw. Without places, no map. */
  map: PageLayout;
  /** Set when the build could not read the whole history of the repository: every History ends with a line that says so. */
  shallow: boolean;
  /** The films of films/, in the order films/README.md gives. */
  films: Film[];
}

/** A film of films/: what its film.json says, and whether the site plays it. */
export interface Film {
  id: string;
  title: string;
  line: string;
  /** The ids of the entries it shows. */
  shows: string[];
  /** The ids of the entries it shows that changed in what they say, or were dropped or replaced, after the film last changed. */
  behind: string[];
  /** Whether the build wrote its page, at /films/<id>/. */
  playable: boolean;
}

/** A film with what the page works out of it. */
export interface FilmItem extends Film {
  /** The parts of the entries it shows, in the order of the parts. */
  parts: string[];
  /** The keys of the rows of the entries it shows, in the plan's order; an id the plan no longer holds is left out. */
  rows: string[];
}

export interface LiveIssue {
  kind: 'entry' | 'suggestion';
  entry: string | null;
  title: string;
  state: 'open' | 'closed';
  reason: null | 'completed' | 'not_planned';
  up: number;
  comments: number;
  author: string;
  bot: boolean;
  createdAt: string;
  closedAt: string | null;
}

export interface LiveState {
  repo: string;
  fetchedAt: string;
  issues: Record<string, LiveIssue>;
  /** The issue number of each entry's thread, by entry id; an entry with no comment or vote yet has none. */
  threads?: Record<string, number>;
  /** Set when the snapshot is older than the server's freshness bound, or a cache served it stale. */
  old?: boolean;
}

export type RowKind = 'entry' | 'suggestion';

export interface Row {
  key: string;
  kind: RowKind;
  /** For an entry: a decision or a feature. */
  of: Entry['kind'] | null;
  name: string;
  what: string | null;
  issue: number | null;
  status: Status;
  stage: string | null;
  part: string | null;
  /** Place in order.json, for entries. */
  n: number | null;
  /** Whether the entry has notes. */
  notes: boolean;
  /** Whether the entry has something to show from `Facts`. */
  more: boolean;
  /** The ids of the films that show this entry, in the films' order. */
  films: string[];
  /** Whether a film that shows it is behind on it. */
  filmBehind: boolean;
  /** The entry's closed History line; null when it has no change. */
  history: HistorySummary | null;
  /** The entry's verdict; null for no verdict, and for anything that is not an entry. */
  verdict: Verdict | null;
  /** Who wrote the verdict when it was the agents; null for the owner, and for anything that is not an entry. */
  by: 'agents' | null;
  /** The keys of the rows of the entries it needs. */
  needs: string[];
  /** The day the file says its build started. */
  started: string | null;
  /** The day the file says it was built. */
  built: string | null;
  author: string | null;
  up: number | null;
  comments: number | null;
  /** The keys of the rows of the decisions this one replaces, or would replace while it has no verdict. */
  replaces: string[];
  /** The key of the row of the decision that replaced this one. */
  replacedBy: string | null;
  date: string | null;
  closedAt: string | null;
  /** Whether the thread of this entry is closed on GitHub. */
  closed: boolean;
}

/** A part's features by status. */
export interface Counts {
  built: number;
  building: number;
  decided: number;
  undecided: number;
  dropped: number;
}

/** A part's decisions: taken, open or answered, replaced, dropped. */
export interface DecisionCounts {
  decided: number;
  undecided: number;
  replaced: number;
  dropped: number;
}

/**
 * building: a feature being built; built: something built and nothing decided left; next: holds the first decided
 * feature in order, neither being built nor built (one part at most); decided; open: no feature decided.
 */
export type PartState = 'building' | 'built' | 'next' | 'decided' | 'open';

export interface PartGroup {
  part: Part;
  /** Position in plan/parts.json, from 1. */
  number: number;
  /** Its decisions and features in the order of order.json, dropped ones included. */
  rows: Row[];
  /** Its features' counts: a part's state, counts and status line come from its features only. */
  counts: Counts;
  decisions: DecisionCounts;
  state: PartState;
  /** Holds an entry of the first STARTER_STAGES stages, so it sits in the first band of the sheet. */
  first: boolean;
}

export interface Plan {
  parts: PartGroup[];
  /** The decisions of the whole product (plan/decisions/), in order. */
  product: Row[];
  /** Suggestions by votes; dropped ones last. */
  suggestions: Row[];
  /** The films, in the order films/README.md gives. */
  films: FilmItem[];
  /** Whether the history is cut short (BuildData.shallow). */
  shallow: boolean;
}

/** How many stages make the first band of the sheet. */
export const STARTER_STAGES = 5;

/** A list's features and its decisions, each in the order the list had. A list shows them in that order: features first. */
export function splitRows(rows: Row[]): { features: Row[]; decisions: Row[] } {
  return { features: rows.filter((row) => row.of === 'feature'), decisions: rows.filter((row) => row.of === 'decision') };
}

/**
 * A row's state word with who decided it: "decided by agents" and "dropped by agents" for a verdict the agents wrote,
 * the word as it is for anything else.
 */
export function byWho(row: Row, word: string): string {
  return row.by === 'agents' && (row.status === 'decided' || row.status === 'dropped') ? `${word} by agents` : word;
}

/** Whether a row stands as decided: a replaced decision does not, whatever it once was. */
export function isDecided(row: Row): boolean {
  return row.status !== 'undecided' && row.status !== 'replaced';
}

type ModelEntry = Parameters<typeof modelStatus>[0];

function modelEntry(entry: Entry): ModelEntry {
  return {
    kind: entry.kind,
    ...(entry.verdict ? { verdict: entry.verdict } : {}),
    ...(entry.kind === 'decision' && entry.what !== null ? { answer: entry.what } : {}),
    ...(entry.started ? { started: entry.started } : {}),
    ...(entry.built ? { built: entry.built } : {}),
    // The model reads a replaced decision from this key alone.
    ...(entry.kind === 'decision' && entry.replacedBy !== null ? { replaced_by: entry.replacedBy } : {}),
    needs: entry.needs,
  };
}

/** The page's status of every entry, by id: the model's, from the files alone, in the page's words. */
export function entryStatuses(entries: Entry[]): Map<string, Status> {
  const models: Record<string, ModelEntry> = Object.fromEntries(entries.map((entry) => [entry.id, modelEntry(entry)]));
  return new Map(entries.map((entry) => {
    switch (modelStatus(models[entry.id], models)) {
      case 'built': return [entry.id, 'built'];
      case 'building': return [entry.id, 'building'];
      case 'dropped': return [entry.id, 'dropped'];
      case 'replaced': return [entry.id, 'replaced'];
      case 'taken': case 'ready': return [entry.id, 'decided'];
      default: return [entry.id, 'undecided'];
    }
  }));
}

/** A suggestion, by its issue alone: closed as not planned is dropped, closed as completed is built, open is not decided. */
function suggestionStatus(live: LiveIssue): Status {
  if (live.state === 'closed' && live.reason === 'completed') return 'built';
  if (live.state === 'closed' && live.reason === 'not_planned') return 'dropped';
  return 'undecided';
}

function baseRow(key: string, kind: RowKind, name: string): Row {
  return {
    key, kind, of: null, name, what: null, issue: null, status: 'undecided', stage: null, part: null, n: null, notes: false, more: false, films: [], filmBehind: false, history: null, verdict: null, by: null, needs: [], started: null, built: null,
    author: null, up: null, comments: null, replaces: [], replacedBy: null, date: null, closedAt: null, closed: false,
  };
}

/** @param rows the features of a part */
export function countRows(rows: { status: Status }[]): Counts {
  const counts: Counts = { built: 0, building: 0, decided: 0, undecided: 0, dropped: 0 };
  for (const row of rows) if (row.status !== 'replaced') counts[row.status] += 1;
  return counts;
}

/** @param rows the decisions of a part */
export function countDecisions(rows: { status: Status }[]): DecisionCounts {
  const counts: DecisionCounts = { decided: 0, undecided: 0, replaced: 0, dropped: 0 };
  for (const row of rows) {
    if (row.status === 'dropped') counts.dropped += 1;
    else if (row.status === 'replaced') counts.replaced += 1;
    else if (row.status === 'undecided') counts.undecided += 1;
    else counts.decided += 1;
  }
  return counts;
}

/** Each part's state, from its features: being built, built, next (at most one), decided, open. */
export function partStates(groups: { rows: Row[]; counts: Counts }[]): PartState[] {
  const states: PartState[] = groups.map(({ counts }) => (counts.building ? 'building' : counts.built && !counts.decided ? 'built' : counts.decided ? 'decided' : 'open'));
  let next = -1;
  let lowest = Infinity;
  groups.forEach(({ rows }, index) => {
    for (const row of rows) {
      if (row.of === 'feature' && row.status === 'decided' && row.n !== null && row.n < lowest) {
        lowest = row.n;
        next = index;
      }
    }
  });
  // A part both next and being built reads being built.
  if (next !== -1 && states[next] !== 'building') states[next] = 'next';
  return states;
}

/**
 * Where the build is, in one line: while no feature is decided, how many are proposed; then the part being built, or
 * else the part that holds the first decided feature.
 */
export function statusLine(groups: { part: Part; state: PartState; counts: Counts }[]): string {
  if (groups.every((group) => !group.counts.decided && !group.counts.building && !group.counts.built)) {
    const proposed = groups.reduce((sum, group) => sum + group.counts.undecided, 0);
    return `${proposed} ${proposed === 1 ? 'feature' : 'features'} proposed, none decided yet.`;
  }
  const building = groups.find((group) => group.state === 'building');
  if (building) return `${building.part.title} is being built.`;
  const next = groups.find((group) => group.state === 'next');
  if (!next) return '';
  if (groups.some((group) => group.counts.built > 0)) return `${next.part.title} is next.`;
  return `Starting with ${next.part.title}.`;
}

/** The few words on a part's tile or place on the map. */
export function stateWords(group: { state: PartState; counts: Counts }): string {
  const { built, decided } = group.counts;
  switch (group.state) {
    case 'building': return 'being built';
    case 'built': return 'built';
    case 'next': return 'next';
    case 'decided': return built ? `${built}/${built + decided} built` : `${decided} decided`;
    default: return '';
  }
}

/** @param closedKeys the rows whose thread the server refused a write on as closed, before the state says so */
export function buildPlan(data: BuildData, live: LiveState | null, closedKeys: ReadonlySet<string> = new Set()): Plan {
  const issues = live?.issues ?? {};
  // An entry's thread is the issue the server names for it; the page matches none by itself.
  const threads = live?.threads ?? {};
  const threadNumbers = new Set(Object.values(threads));
  // An entry with no thread yet shows no vote and no comment, once a state is shown.
  const counts = (row: Row) => {
    const issue = row.issue === null ? undefined : issues[row.issue];
    if (issue) {
      row.up = issue.up;
      row.comments = issue.comments;
      row.closedAt = issue.closedAt;
      row.closed = row.kind === 'entry' && (issue.state === 'closed' || closedKeys.has(row.key));
    } else if (live && row.kind === 'entry') {
      row.up = 0;
      row.comments = 0;
    }
  };

  const statuses = entryStatuses(data.entries);
  const entries = new Map<string, Row>();
  for (const entry of data.entries) {
    const row = baseRow(`entry-${entry.id}`, 'entry', entry.name);
    row.of = entry.kind;
    row.what = entry.what;
    row.stage = entry.stage;
    row.part = entry.part;
    row.n = entry.position;
    row.notes = entry.notes;
    row.more = entry.more;
    row.history = entry.history;
    row.verdict = entry.verdict;
    row.by = entry.by;
    row.needs = entry.needs.map((id) => `entry-${id}`);
    row.started = entry.started;
    row.built = entry.built;
    // A dropped decision replaces nothing, so it names nothing.
    row.replaces = entry.kind === 'decision' && statuses.get(entry.id) !== 'dropped' ? entry.replaces.map((id) => `entry-${id}`) : [];
    row.replacedBy = entry.replacedBy === null ? null : `entry-${entry.replacedBy}`;
    row.issue = Number.isSafeInteger(threads[entry.id]) ? threads[entry.id] : null;
    row.status = statuses.get(entry.id) as Status;
    counts(row);
    entries.set(entry.id, row);
  }
  const rows = [...entries.values()];

  const starter = new Set(data.stages.slice(0, STARTER_STAGES).map((stage) => stage.id));
  const groups: PartGroup[] = data.parts.map((part, index) => {
    const own = rows.filter((row) => row.part === part.id);
    return {
      part, number: index + 1, rows: own,
      counts: countRows(own.filter((row) => row.of === 'feature')),
      decisions: countDecisions(own.filter((row) => row.of === 'decision')),
      state: 'open', first: own.some((row) => row.stage !== null && starter.has(row.stage)),
    };
  });
  partStates(groups).forEach((state, index) => { groups[index].state = state; });

  const suggestions: Row[] = [];
  for (const [key, issue] of Object.entries(issues)) {
    if (issue.kind !== 'suggestion') continue;
    const number = Number(key);
    // A feature written from a suggestion shows once, as the feature.
    if (threadNumbers.has(number)) continue;
    const row = baseRow(`issue-${number}`, 'suggestion', issue.title);
    row.issue = number;
    row.author = issue.author;
    row.date = issue.createdAt;
    row.status = suggestionStatus(issue);
    counts(row);
    suggestions.push(row);
  }
  const dropped = (row: Row) => (row.status === 'dropped' ? 1 : 0);
  suggestions.sort((a, b) => dropped(a) - dropped(b) || (b.up ?? 0) - (a.up ?? 0) || (a.issue ?? 0) - (b.issue ?? 0));

  const films: FilmItem[] = data.films.map((film) => {
    const shown = film.shows.map((id) => entries.get(id)).filter((row): row is Row => row !== undefined).sort((a, b) => (a.n ?? 0) - (b.n ?? 0));
    for (const row of shown) {
      row.films.push(film.id);
      if (film.behind.includes(row.key.slice('entry-'.length))) row.filmBehind = true;
    }
    const covered = new Set(shown.flatMap((row) => (row.part ? [row.part] : [])));
    return { ...film, parts: data.parts.map((part) => part.id).filter((id) => covered.has(id)), rows: shown.map((row) => row.key) };
  });

  const product = rows.filter((row) => row.part === null);
  return { parts: groups, product, suggestions, films, shallow: data.shallow };
}

/** Every row once: the parts' entries, the whole product's decisions, then suggestions. */
export function allRows(plan: Plan): Row[] {
  return [...plan.parts.flatMap((group) => group.rows), ...plan.product, ...plan.suggestions];
}

/** The page's words for each name of a change; a taken entry is decided. */
const PHRASES: Record<ChangeName, string> = {
  added: 'Added',
  moved: 'Moved to another part',
  text: 'Text changed',
  sets: 'What a yes would do changed',
  needs: 'Needs changed',
  keeps: 'What it must keep changed',
  surfaces: 'Where it shows changed',
  suggest: 'Recommendation changed',
  notes: 'Notes edited',
  dropped: 'Dropped',
  cleared: 'Back to not decided',
  replaced: 'Replaced by',
  taken: 'Decided',
  started: 'Being built',
  built: 'Built',
};

/** What one name of a change says, on an entry of either kind. */
export function changePhrase(name: ChangeName, agents = false): string {
  return agents && (name === 'taken' || name === 'dropped') ? `${PHRASES[name]} by agents` : PHRASES[name];
}

/** What a change says on one line: its phrases, in the module's order, joined by " · ". */
export function changeLine(what: readonly ChangeName[], agents = false): string {
  return what.map((name) => changePhrase(name, agents)).join(' · ');
}

const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];

export function shortDate(iso: string): string {
  const match = /^(\d{4})-(\d{2})-(\d{2})/.exec(iso);
  if (!match) return '';
  return `${Number(match[3])} ${MONTHS[Number(match[2]) - 1]}`;
}

export function relativeTime(iso: string, now = Date.now()): string {
  const then = Date.parse(iso);
  if (Number.isNaN(then)) return '';
  const days = Math.floor((now - then) / 86_400_000);
  if (days <= 0) {
    const hours = Math.floor((now - then) / 3_600_000);
    if (hours <= 0) return 'just now';
    return hours === 1 ? '1 hour ago' : `${hours} hours ago`;
  }
  if (days === 1) return 'yesterday';
  if (days < 30) return `${days} days ago`;
  return shortDate(iso);
}
