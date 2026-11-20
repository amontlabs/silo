// The Workshop's model: the plan built into the page, merged with the live state from GitHub. An entry's status
// comes from its file alone, through the plan's model (tools/plan/model); this adds the rows, the parts' counts
// and states, and the words the page uses for them.
import { entryStatus as modelStatus } from '../../../tools/plan/model/index.js';
import type { Verdict } from '../../../tools/plan/model/status.js';
import type { Change } from '../../../tools/workshop/model/markers.js';
import type { MapLayout } from './map';

export type { Change, Verdict };

/** A row's status, in the page's words. A taken feature, ready or not, is planned; a taken decision is decided. */
export type Status = 'built' | 'planned' | 'undecided' | 'dropped';

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
  suggest: { verdict: Verdict | null; why: string | null } | null;
  verdict: Verdict | null;
  /** The day it was built, `YYYY-MM-DD`. */
  built: string | null;
  /** Whether it has notes; their text is a chunk of its own (`virtual:workshop-notes`), loaded on demand. */
  notes: boolean;
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
}

export interface BuildData {
  parts: Part[];
  stages: Stage[];
  /** Every decision and feature, in the order of order.json. */
  entries: Entry[];
  /** Where the parts sit on the map: Silo's own (map.ts), or the example the tests draw. Without places, no map. */
  map: MapLayout;
  /** Whether the owner's verdict buttons may show. The server has no verdict route: off in the real build. */
  decide: boolean;
  /** Whether entries have threads on GitHub (votes, comments, history). Off in the real build: no entry has one yet. */
  threads: boolean;
}

export interface LiveIssue {
  kind: 'entry' | 'suggestion';
  entry: string | null;
  title: string;
  state: 'open' | 'closed';
  reason: null | 'completed' | 'not_planned';
  verdict: Verdict | null;
  up: number;
  comments: number;
  /** How many of its comments are history lines. */
  updates?: number;
  author: string;
  bot: boolean;
  createdAt: string;
  closedAt: string | null;
}

/** One of the newest comments or history lines across the entries. */
export interface LiveLatest {
  n: number;
  entry: string;
  author: string;
  bot: boolean;
  createdAt: string;
  text?: string;
  change?: Change;
}

export interface LiveState {
  repo: string;
  fetchedAt: string;
  issues: Record<string, LiveIssue>;
  latest?: LiveLatest[];
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
  suggest: Entry['suggest'];
  /** Whether the entry has notes. */
  notes: boolean;
  /** The day the file says it was built. */
  built: string | null;
  author: string | null;
  up: number | null;
  /** People's comments: the issue's comments less its history lines. */
  comments: number | null;
  updates: number;
  /** The owner's verdict as its issue records it. It never changes the status, which comes from the file. */
  recorded: Verdict | null;
  date: string | null;
  closedAt: string | null;
}

export interface LatestItem {
  key: string;
  row: Row;
  author: string;
  createdAt: string;
  text: string | null;
  change: Change | null;
}

/** A part's features by status. */
export interface Counts {
  built: number;
  planned: number;
  undecided: number;
  dropped: number;
}

/** A part's decisions: taken, open or answered, dropped. */
export interface DecisionCounts {
  decided: number;
  undecided: number;
  dropped: number;
}

/** done: something built and nothing planned; next: holds the first planned feature in order (one part at most); planned; open. */
export type PartState = 'done' | 'next' | 'planned' | 'open';

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
  /** Every dropped entry of the parts, in order. */
  left: Row[];
  /** Suggestions by votes; dropped ones last. */
  suggestions: Row[];
  /** The newest comments and history lines across the entries, newest first; empty without threads. */
  latest: LatestItem[];
  /** Whether entries have threads (BuildData.threads). */
  threads: boolean;
}

/** How many stages make the first band of the sheet. */
export const STARTER_STAGES = 5;

export function isDecided(row: Row): boolean {
  return row.status !== 'undecided';
}

type ModelEntry = Parameters<typeof modelStatus>[0];

function modelEntry(entry: Entry): ModelEntry {
  return {
    kind: entry.kind,
    ...(entry.verdict ? { verdict: entry.verdict } : {}),
    ...(entry.kind === 'decision' && entry.what !== null ? { answer: entry.what } : {}),
    ...(entry.built ? { built: entry.built } : {}),
    needs: entry.needs,
  };
}

/** The page's status of every entry, by id: the model's, from the files alone, in the page's words. */
export function entryStatuses(entries: Entry[]): Map<string, Status> {
  const models: Record<string, ModelEntry> = Object.fromEntries(entries.map((entry) => [entry.id, modelEntry(entry)]));
  return new Map(entries.map((entry) => {
    switch (modelStatus(models[entry.id], models)) {
      case 'built': return [entry.id, 'built'];
      case 'dropped': return [entry.id, 'dropped'];
      case 'taken': case 'ready': return [entry.id, 'planned'];
      default: return [entry.id, 'undecided'];
    }
  }));
}

/** A suggestion: closed as not planned is dropped, closed as completed is built, else its verdict label. */
function suggestionStatus(live: LiveIssue): Status {
  if (live.state === 'closed' && live.reason === 'completed') return 'built';
  if (live.state === 'closed' && live.reason === 'not_planned') return 'dropped';
  return live.verdict === 'take' ? 'planned' : live.verdict === 'drop' ? 'dropped' : 'undecided';
}

function baseRow(key: string, kind: RowKind, name: string): Row {
  return {
    key, kind, of: null, name, what: null, issue: null, status: 'undecided', stage: null, part: null, n: null, suggest: null, notes: false, built: null,
    author: null, up: null, comments: null, updates: 0, recorded: null, date: null, closedAt: null,
  };
}

/** @param rows the features of a part */
export function countRows(rows: { status: Status }[]): Counts {
  const counts: Counts = { built: 0, planned: 0, undecided: 0, dropped: 0 };
  for (const row of rows) counts[row.status] += 1;
  return counts;
}

/** @param rows the decisions of a part */
export function countDecisions(rows: { status: Status }[]): DecisionCounts {
  const counts: DecisionCounts = { decided: 0, undecided: 0, dropped: 0 };
  for (const row of rows) {
    if (row.status === 'dropped') counts.dropped += 1;
    else if (row.status === 'undecided') counts.undecided += 1;
    else counts.decided += 1;
  }
  return counts;
}

/** Each part's state, from its features: done, next (at most one), planned, open. */
export function partStates(groups: { rows: Row[]; counts: Counts }[]): PartState[] {
  const states: PartState[] = groups.map(({ counts }) => (counts.built && !counts.planned ? 'done' : counts.planned ? 'planned' : 'open'));
  let next = -1;
  let lowest = Infinity;
  groups.forEach(({ rows }, index) => {
    for (const row of rows) {
      if (row.of === 'feature' && row.status === 'planned' && row.n !== null && row.n < lowest) {
        lowest = row.n;
        next = index;
      }
    }
  });
  if (next !== -1) states[next] = 'next';
  return states;
}

/** Where the build is, in one line: the part that holds the first planned feature. */
export function statusLine(groups: { part: Part; state: PartState; counts: Counts }[]): string {
  const next = groups.find((group) => group.state === 'next');
  if (!next) return '';
  if (groups.some((group) => group.counts.built > 0)) return `${next.part.title} is next.`;
  return `Starting with ${next.part.title}.`;
}

/** The few words on a part's tile or place on the map. */
export function stateWords(group: { state: PartState; counts: Counts }): string {
  const { built, planned } = group.counts;
  switch (group.state) {
    case 'done': return 'done';
    case 'next': return 'next';
    case 'planned': return built ? `${built}/${built + planned} built` : `${planned} planned`;
    default: return '';
  }
}

export function buildPlan(data: BuildData, live: LiveState | null): Plan {
  const issues = live?.issues ?? {};
  // Old issues on GitHub are never matched to an entry while entries have no threads.
  const byEntry = new Map<string, number>();
  if (data.threads) {
    for (const [number, issue] of Object.entries(issues)) {
      if (issue.kind === 'entry' && issue.entry) byEntry.set(issue.entry, Number(number));
    }
  }
  const counts = (row: Row) => {
    const issue = row.issue === null ? undefined : issues[row.issue];
    if (issue) {
      row.updates = Math.max(0, Math.min(issue.updates ?? 0, issue.comments));
      row.up = issue.up;
      row.comments = issue.comments - row.updates;
      row.recorded = issue.verdict === 'take' || issue.verdict === 'drop' ? issue.verdict : null;
      row.closedAt = issue.closedAt;
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
    row.suggest = entry.suggest;
    row.notes = entry.notes;
    row.built = entry.built;
    row.issue = byEntry.get(entry.id) ?? null;
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

  const latest: LatestItem[] = [];
  for (const [index, item] of (data.threads ? live?.latest ?? [] : []).entries()) {
    const row = entries.get(item.entry);
    if (!row || row.issue !== item.n) continue;
    latest.push({ key: `latest-${index}`, row, author: item.author, createdAt: item.createdAt, text: item.text ?? null, change: item.change ?? null });
  }

  const product = rows.filter((row) => row.part === null);
  const left = groups.flatMap((group) => group.rows.filter((row) => row.status === 'dropped')).sort((a, b) => (a.n ?? 0) - (b.n ?? 0));
  return { parts: groups, product, left, suggestions, latest, threads: data.threads };
}

/** Every row once: the parts' entries, the whole product's decisions, then suggestions. */
export function allRows(plan: Plan): Row[] {
  return [...plan.parts.flatMap((group) => group.rows), ...plan.product, ...plan.suggestions];
}

/**
 * A feature the owner may still take or drop: undecided in its file, with a real issue that records no verdict yet.
 * Decisions are taken with `plan take`.
 */
export function awaitsVerdict(row: Row): boolean {
  return row.kind === 'entry' && row.of === 'feature' && row.status === 'undecided' && row.recorded === null && row.issue !== null && row.issue > 0;
}

export const needsOwner = awaitsVerdict;

/** By the field names of a history marker (tools/workshop/model): `area` is that wire format's name for the part. */
const PHRASES: Record<string, string> = {
  name: 'Text changed',
  what: 'Text changed',
  notes: 'Notes edited',
  needs: 'Needs changed',
  area: 'Moved to another part',
  suggest: 'Suggestion changed',
  surfaces: 'Surfaces changed',
  built: 'Built',
};

/** What a history line says changed, one phrase per change in the order of its fields; `suggested_in` says nothing. */
export function changeWords(change: Change): string[] {
  const words: string[] = [];
  for (const field of change.fields) {
    let word: string | undefined;
    if (field !== 'verdict') word = PHRASES[field];
    else if (change.verdict === 'take') word = 'Taken';
    else if (change.verdict === 'drop') word = 'Dropped';
    // A verdict that is no longer one (the old "later") says nothing.
    else if (!change.verdict) word = 'Verdict cleared';
    if (word && !words.includes(word)) words.push(word);
  }
  return words;
}

/** The phrases of a change on one line. */
export const changeLine = (change: Change) => changeWords(change).join(' · ');

/** Whether a change touched the text a person can compare: `name` or `what`. */
export const changesText = (change: Change) => change.fields.includes('name') || change.fields.includes('what');

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
