// The Workshop's model: the plan built into the page, merged with the live state from GitHub.

export type Verdict = 'take' | 'refine' | 'later' | 'drop';

export interface Feature {
  n: number;
  id: string;
  stage: string;
  /** The part of the product it belongs to; null for the old things not carried over. */
  area: string | null;
  name: string;
  what: string;
  suggest: string | null;
  why: string | null;
  verdict: Verdict | null;
  issue: number | null;
  recordings: string[];
}

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

export interface DecidedLine {
  index: number;
  date: string;
  who: string;
  text: string;
  issue: number | null;
}

export interface BuildData {
  features: Feature[];
  areas: Area[];
  stages: Stage[];
  decided: DecidedLine[];
}

export interface LiveIssue {
  kind: 'entry' | 'decision' | 'suggestion';
  entry: string | null;
  title: string;
  state: 'open' | 'closed';
  reason: null | 'completed' | 'not_planned';
  verdict: Verdict | null;
  up: number;
  comments: number;
  author: string;
  bot: boolean;
  createdAt: string;
  closedAt: string | null;
  /** Open and labelled `workshop:building`: the work is under way. */
  building?: boolean;
}

export interface LiveState {
  repo: string;
  fetchedAt: string;
  issues: Record<string, LiveIssue>;
}

export type Status = 'undecided' | 'refine' | 'planned' | 'later' | 'building' | 'built' | 'dropped';

export type RowKind = 'entry' | 'suggestion' | 'decision' | 'decided';

export interface Row {
  key: string;
  kind: RowKind;
  name: string;
  what: string | null;
  issue: number | null;
  status: Status;
  carried: boolean;
  fileVerdict: Verdict | null;
  stage: string | null;
  area: string | null;
  /** Build order, for entries. */
  n: number | null;
  suggest: string | null;
  why: string | null;
  recordings: string[];
  author: string | null;
  up: number | null;
  comments: number | null;
  date: string | null;
  who: string | null;
  closedAt: string | null;
}

/** The row name of a decided line: its first sentence or clause. */
export function decisionTitle(text: string): string {
  return text.split(/(?<=[.:])\s/)[0].replace(/[.:]$/, '').trim();
}

function statusFromVerdict(verdict: Verdict | null): Status {
  switch (verdict) {
    case 'take': return 'planned';
    case 'later': return 'later';
    case 'drop': return 'dropped';
    case 'refine': return 'refine';
    default: return 'undecided';
  }
}

/** A live issue decides the status; a file verdict without a live one is carried over. */
export function issueStatus(live: LiveIssue | undefined, fileVerdict: Verdict | null): { status: Status; carried: boolean } {
  if (live) {
    if (live.state === 'closed' && live.reason === 'completed') return { status: 'built', carried: false };
    if (live.verdict === 'drop' || (live.state === 'closed' && live.reason === 'not_planned')) return { status: 'dropped', carried: false };
    if (live.verdict) return building(live, { status: statusFromVerdict(live.verdict), carried: false });
  }
  return building(live, { status: statusFromVerdict(fileVerdict), carried: fileVerdict !== null });
}

/** A planned entry whose open issue says the work is under way is being built. */
function building(live: LiveIssue | undefined, result: { status: Status; carried: boolean }) {
  return result.status === 'planned' && live?.state === 'open' && live.building === true ? { ...result, status: 'building' as const } : result;
}

function baseRow(key: string, kind: RowKind, name: string): Row {
  return {
    key, kind, name, what: null, issue: null, status: 'undecided', carried: false, fileVerdict: null, stage: null, area: null, n: null,
    suggest: null, why: null, recordings: [], author: null, up: null, comments: null, date: null, who: null, closedAt: null,
  };
}

export type AreaState = 'now' | 'done' | 'next' | 'planned' | 'open';

export interface Counts {
  built: number;
  building: number;
  planned: number;
  later: number;
  /** Undecided or being refined. */
  undecided: number;
  dropped: number;
}

export interface AreaGroup {
  area: Area;
  /** Position in docs/product/areas.json, from 1. */
  number: number;
  /** Its entries in build order, dropped ones included. */
  rows: Row[];
  counts: Counts;
  state: AreaState;
  /** Holds an entry of the stages up to the starter, so it sits in the first band of the sheet. */
  first: boolean;
}

export interface Plan {
  areas: AreaGroup[];
  /** Not carried over: the entries without an area, then every dropped entry. */
  left: Row[];
  /** Suggestions by votes; dropped ones last. */
  suggestions: Row[];
  openDecisions: Row[];
  decided: Row[];
}

/** The last stage of the starter: areas with an entry up to it come first in the sheet. */
export const STARTER_LAST = 'living';

export function isDecided(row: Row): boolean {
  return row.status !== 'undecided' && row.status !== 'refine';
}

export function countRows(rows: Row[]): Counts {
  const counts: Counts = { built: 0, building: 0, planned: 0, later: 0, undecided: 0, dropped: 0 };
  for (const row of rows) {
    if (row.status === 'refine') counts.undecided += 1;
    else counts[row.status] += 1;
  }
  return counts;
}

/**
 * Each area's state, first match wins: now (an entry is building), done (something built, nothing planned),
 * next (the area holding the lowest-numbered planned entry among areas not being built; at most one),
 * planned (an entry is planned), open (not decided yet).
 */
export function areaStates(groups: { rows: Row[]; counts: Counts }[]): AreaState[] {
  const states: AreaState[] = groups.map(({ counts }) => (counts.building ? 'now' : counts.built && !counts.planned ? 'done' : counts.planned ? 'planned' : 'open'));
  let next = -1;
  let lowest = Infinity;
  groups.forEach(({ rows }, index) => {
    if (states[index] === 'now') return;
    for (const row of rows) {
      if (row.status === 'planned' && row.n !== null && row.n < lowest) {
        lowest = row.n;
        next = index;
      }
    }
  });
  if (next !== -1) states[next] = 'next';
  return states;
}

function listNames(names: string[]): string {
  return names.length < 2 ? names.join('') : `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`;
}

/** Areas whose name reads with "the" in a sentence. */
const WITH_THE = new Set(['engine']);

/** The one line under the title: where the build is. */
export function statusLine(groups: AreaGroup[]): string {
  const now = groups.filter((group) => group.state === 'now').map((group) => group.area.title);
  const next = groups.find((group) => group.state === 'next');
  const built = groups.some((group) => group.counts.built > 0);
  if (now.length) return `Building ${listNames(now)}.${next ? ` ${next.area.title} is next.` : ''}`;
  if (!next) return '';
  if (built) return `${next.area.title} is next.`;
  return `Starting with ${WITH_THE.has(next.area.id) ? 'the ' : ''}${next.area.title}.`;
}

/** The few words on a part of the map or a tile. */
export function stateWords(group: Pick<AreaGroup, 'state' | 'counts'>): string {
  const { built, building, planned } = group.counts;
  switch (group.state) {
    case 'done': return 'done';
    case 'now': return `now · ${built}/${built + building + planned}`;
    case 'next': return 'next';
    case 'planned': return built ? `${built}/${built + planned} built` : `${planned} planned`;
    default: return '';
  }
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
      row.up = issue.up;
      row.comments = issue.comments;
      row.closedAt = issue.closedAt;
    }
  };

  const byArea = new Map<string, Row[]>();
  const unplaced: Row[] = [];
  for (const feature of data.features) {
    const row = baseRow(`entry-${feature.id}`, 'entry', feature.name);
    row.what = feature.what;
    row.stage = feature.stage;
    row.area = feature.area;
    row.n = feature.n;
    row.suggest = feature.suggest;
    row.why = feature.why;
    row.recordings = feature.recordings;
    row.fileVerdict = feature.verdict;
    row.issue = byEntry.get(feature.id) ?? feature.issue;
    Object.assign(row, issueStatus(row.issue === null ? undefined : issues[row.issue], feature.verdict));
    counts(row);
    if (feature.area === null) unplaced.push(row);
    else byArea.set(feature.area, [...(byArea.get(feature.area) ?? []), row]);
  }

  const stageIndex = new Map(data.stages.map((stage, index) => [stage.id, index]));
  const starterEnd = stageIndex.get(STARTER_LAST) ?? Infinity;
  const groups = data.areas.map((area, index) => {
    const rows = (byArea.get(area.id) ?? []).sort((a, b) => (a.n ?? 0) - (b.n ?? 0));
    const first = rows.some((row) => (stageIndex.get(row.stage ?? '') ?? Infinity) <= starterEnd);
    return { area, number: index + 1, rows, counts: countRows(rows), state: 'open' as AreaState, first };
  });
  areaStates(groups).forEach((state, index) => { groups[index].state = state; });

  const suggestions: Row[] = [];
  const openDecisions: Row[] = [];
  const closedDecisions: Row[] = [];
  const referenced = new Set(data.decided.flatMap((line) => (line.issue === null ? [] : [line.issue])));
  for (const [key, issue] of Object.entries(issues)) {
    const number = Number(key);
    if (issue.kind === 'suggestion') {
      const row = baseRow(`issue-${number}`, 'suggestion', issue.title);
      row.issue = number;
      row.author = issue.author;
      row.date = issue.createdAt;
      Object.assign(row, issueStatus(issue, null));
      counts(row);
      suggestions.push(row);
    } else if (issue.kind === 'decision' && !referenced.has(number)) {
      const row = baseRow(`issue-${number}`, 'decision', issue.title);
      row.issue = number;
      row.date = issue.createdAt;
      counts(row);
      if (issue.state === 'open') openDecisions.push(row);
      else if (issue.reason === 'completed') {
        row.status = 'planned';
        row.date = issue.closedAt ?? issue.createdAt;
        closedDecisions.push(row);
      }
    }
  }
  const dropped = (row: Row) => (row.status === 'dropped' ? 1 : 0);
  suggestions.sort((a, b) => dropped(a) - dropped(b) || (b.up ?? 0) - (a.up ?? 0) || (a.issue ?? 0) - (b.issue ?? 0));
  openDecisions.sort((a, b) => (b.date ?? '').localeCompare(a.date ?? ''));
  closedDecisions.sort((a, b) => (b.date ?? '').localeCompare(a.date ?? ''));

  const decided = [
    ...closedDecisions,
    ...data.decided.map((line) => {
      const row = baseRow(line.issue === null ? `decided-${line.index}` : `issue-${line.issue}`, 'decided', decisionTitle(line.text));
      row.what = line.text;
      row.issue = line.issue;
      row.status = 'planned';
      row.date = line.date;
      row.who = line.who;
      counts(row);
      return row;
    }),
  ];

  const left = [...unplaced, ...groups.flatMap((group) => group.rows.filter((row) => row.status === 'dropped')).sort((a, b) => (a.n ?? 0) - (b.n ?? 0))];
  return { areas: groups, left, suggestions, openDecisions, decided };
}

/** Every row once: the areas' entries, those left without an area, suggestions, then decisions. */
export function allRows(plan: Plan): Row[] {
  return [
    ...plan.areas.flatMap((group) => group.rows),
    ...plan.left.filter((row) => row.area === null),
    ...plan.suggestions,
    ...plan.openDecisions,
    ...plan.decided,
  ];
}

/** A row the owner still has to decide: undecided or being refined, with a real issue to record the verdict on. */
export function awaitsVerdict(row: Row): boolean {
  return (row.kind === 'entry' || row.kind === 'suggestion') && !isDecided(row) && !row.carried && row.issue !== null && row.issue > 0;
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
