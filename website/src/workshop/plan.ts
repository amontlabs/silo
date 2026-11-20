// The Workshop's model: the plan built into the page, merged with the live state from GitHub. Names, the
// status rule and the areas' states come from the plan's model (tools/plan/model); this adds the rows.
import { areaGroups, featureStatus } from '../../../tools/plan/model/index.js';
import type { Change } from '../../../tools/workshop/model/markers.js';
import type { Counts } from '../../../tools/plan/model/areas.js';
import type { Status, Verdict } from '../../../tools/plan/model/status.js';
import type { MapLayout } from './map';

export { countRows, statusLine, stateWords, VERDICTS } from '../../../tools/plan/model/index.js';
export type { Change, Counts, Status, Verdict };

export interface Feature {
  id: string;
  name: string;
  what: string;
  /** The part of the product it belongs to; null for the old things not carried over. */
  area: string | null;
  needs: string[];
  stage: string;
  /** Build order, from 1. */
  position: number;
  suggest: { verdict: Verdict | null; why: string | null } | null;
  verdict: Verdict | null;
  /** The day it was built, `YYYY-MM-DD`. */
  built: string | null;
  /** Whether it has notes; their text is a chunk of its own (`virtual:workshop-notes`), loaded on demand. */
  notes: boolean;
}

/** The notes of the features that have some, Markdown, by feature id. */
export type Notes = Record<string, string>;

export interface Stage {
  id: string;
  title: string;
  line: string;
}

export interface Area {
  id: string;
  title: string;
  line: string;
}

/** What the owner settled for the whole product (`about`: the areas it is about; null for all). */
export interface Decision {
  id: string;
  date: string;
  text: string;
  about: string[] | null;
}

export interface BuildData {
  features: Feature[];
  areas: Area[];
  stages: Stage[];
  /** Newest first. */
  decisions: Decision[];
  /** Where the areas sit on the map: Silo's own (map.ts), or the example the tests draw. */
  map: MapLayout;
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
  /** Open, taken and labelled `workshop:building`: the work is under way. */
  building?: boolean;
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

export type RowKind = 'entry' | 'suggestion' | 'decided';

export interface Row {
  key: string;
  kind: RowKind;
  name: string;
  what: string | null;
  issue: number | null;
  status: Status;
  stage: string | null;
  area: string | null;
  /** Build order, for entries. */
  n: number | null;
  suggest: Feature['suggest'];
  /** Whether the entry has notes. */
  notes: boolean;
  /** The day the file says it was built. */
  built: string | null;
  /** The decisions about its area, for entries; the areas it is about, for decisions. */
  decisions: Decision[];
  about: string[] | null;
  author: string | null;
  up: number | null;
  /** People's comments: the issue's comments less its history lines. */
  comments: number | null;
  updates: number;
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

/** The row name of a decision: its first sentence or clause. */
export function decisionTitle(text: string): string {
  return text.split(/(?<=[.:])\s/)[0].replace(/[.:]$/, '').trim();
}

/** A suggestion: closed as not planned is dropped, closed as completed is built, else its verdict label. */
function suggestionStatus(live: LiveIssue): Status {
  if (live.state === 'closed' && live.reason === 'completed') return 'built';
  if (live.state === 'closed' && live.reason === 'not_planned') return 'dropped';
  return featureStatus({}, { verdict: live.verdict, building: false });
}

/** An entry: the status rule of the model, from its file and its live issue. */
export function entryStatus(feature: Pick<Feature, 'verdict' | 'built'>, live: LiveIssue | undefined): Status {
  return featureStatus(
    { ...(feature.verdict ? { verdict: feature.verdict } : {}), ...(feature.built ? { built: feature.built } : {}) },
    live ? { verdict: live.verdict, building: live.building === true } : null,
  );
}

function baseRow(key: string, kind: RowKind, name: string): Row {
  return {
    key, kind, name, what: null, issue: null, status: 'undecided', stage: null, area: null, n: null, suggest: null, notes: false, built: null,
    decisions: [], about: null, author: null, up: null, comments: null, updates: 0, date: null, closedAt: null,
  };
}

export type AreaState = 'now' | 'done' | 'next' | 'planned' | 'open';

export interface AreaGroup {
  area: Area;
  /** Position in plan/areas.json, from 1. */
  number: number;
  /** Its entries in build order, dropped ones included. */
  rows: Row[];
  counts: Counts;
  state: AreaState;
  /** Holds an entry of the starter's stages, so it sits in the first band of the sheet. */
  first: boolean;
}

export interface Plan {
  areas: AreaGroup[];
  /** Not carried over: the entries without an area, then every dropped entry. */
  left: Row[];
  /** Suggestions by votes; dropped ones last. */
  suggestions: Row[];
  /** The owner's decisions, newest first. */
  decided: Row[];
  /** The newest comments and history lines across the entries, newest first. */
  latest: LatestItem[];
}

export function isDecided(row: Row): boolean {
  return row.status !== 'undecided';
}

/** The decisions about a feature: those whose `about` names its area (`about` holds area ids only). */
export function decisionsAbout(decisions: Decision[], feature: Pick<Feature, 'area'>): Decision[] {
  return decisions.filter((decision) => feature.area !== null && decision.about?.includes(feature.area) === true);
}

/** The areas with their features in build order, counts and states: the model's `areaGroups` on the page's data. */
export function groupAreas(data: BuildData, statusOf: (id: string) => Status) {
  const features = Object.fromEntries(data.features.map((feature) => [feature.id, { ...feature, area: feature.area ?? undefined }]));
  const stages = data.stages.map((stage) => ({ id: stage.id, features: data.features.filter((feature) => feature.stage === stage.id).map((feature) => feature.id) }));
  return areaGroups({ areas: data.areas, stages, features }, statusOf);
}

export function buildPlan(data: BuildData, live: LiveState | null): Plan {
  const issues = live?.issues ?? {};
  const byEntry = new Map<string, number>();
  for (const [number, issue] of Object.entries(issues)) {
    if (issue.kind === 'entry' && issue.entry) byEntry.set(issue.entry, Number(number));
  }
  const counts = (row: Row) => {
    const issue = row.issue === null ? undefined : issues[row.issue];
    if (issue) {
      row.updates = Math.max(0, Math.min(issue.updates ?? 0, issue.comments));
      row.up = issue.up;
      row.comments = issue.comments - row.updates;
      row.closedAt = issue.closedAt;
    }
  };

  const unplaced: Row[] = [];
  const entries = new Map<string, Row>();
  for (const feature of data.features) {
    const row = baseRow(`entry-${feature.id}`, 'entry', feature.name);
    row.what = feature.what;
    row.stage = feature.stage;
    row.area = feature.area;
    row.n = feature.position;
    row.suggest = feature.suggest;
    row.notes = feature.notes;
    row.built = feature.built;
    row.decisions = decisionsAbout(data.decisions, feature);
    row.issue = byEntry.get(feature.id) ?? null;
    row.status = entryStatus(feature, row.issue === null ? undefined : issues[row.issue]);
    counts(row);
    entries.set(feature.id, row);
    if (feature.area === null) unplaced.push(row);
  }

  const groups: AreaGroup[] = groupAreas(data, (id) => (entries.get(id) as Row).status).map((group) => ({
    ...group,
    rows: group.rows.map((row) => entries.get(row.id) as Row),
  }));

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

  const decided = data.decisions.map((decision) => {
    const row = baseRow(`decided-${decision.id}`, 'decided', decisionTitle(decision.text));
    row.what = decision.text;
    row.status = 'planned';
    row.date = decision.date;
    row.about = decision.about;
    return row;
  });

  const latest: LatestItem[] = [];
  for (const [index, item] of (live?.latest ?? []).entries()) {
    const row = entries.get(item.entry);
    if (!row || row.issue !== item.n) continue;
    latest.push({ key: `latest-${index}`, row, author: item.author, createdAt: item.createdAt, text: item.text ?? null, change: item.change ?? null });
  }

  const left = [...unplaced, ...groups.flatMap((group) => group.rows.filter((row) => row.status === 'dropped')).sort((a, b) => (a.n ?? 0) - (b.n ?? 0))];
  return { areas: groups, left, suggestions, decided, latest };
}

/** Every row once: the areas' entries, those left without an area, suggestions, then decisions. */
export function allRows(plan: Plan): Row[] {
  return [
    ...plan.areas.flatMap((group) => group.rows),
    ...plan.left.filter((row) => row.area === null),
    ...plan.suggestions,
    ...plan.decided,
  ];
}

/** An entry the owner still has to decide, with a real issue to record the verdict on. The server records verdicts on entries only. */
export function awaitsVerdict(row: Row): boolean {
  return row.kind === 'entry' && row.status === 'undecided' && row.issue !== null && row.issue > 0;
}

export const needsOwner = awaitsVerdict;

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
    const word = field === 'verdict'
      ? change.verdict === 'take' ? 'Taken' : change.verdict === 'drop' ? 'Dropped' : change.verdict === 'later' ? 'Later' : 'Verdict cleared'
      : PHRASES[field];
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
