// The Workshop's model: the plan built into the page, merged with the live state from GitHub.

export type Verdict = 'take' | 'refine' | 'later' | 'drop';

export interface Feature {
  n: number;
  id: string;
  stage: string;
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

export interface DecidedLine {
  index: number;
  date: string;
  who: string;
  text: string;
  issue: number | null;
}

export interface BuildData {
  features: Feature[];
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

const VERDICTS = new Set(['take', 'refine', 'later', 'drop']);
const RECORDING = /^[A-Za-z0-9][A-Za-z0-9._-]*\.(mp4|webm|mov|m4v)$/;

function str(value: unknown): string | null {
  return typeof value === 'string' && value.trim() ? value : null;
}

export function parseFeatures(text: string): Feature[] {
  const features: Feature[] = [];
  for (const line of text.split('\n')) {
    if (!line.trim()) continue;
    const raw = JSON.parse(line) as Record<string, unknown>;
    const id = str(raw.id);
    const name = str(raw.name);
    const stage = str(raw.stage);
    if (!id || !name || !stage) continue;
    const verdict = typeof raw.verdict === 'string' && VERDICTS.has(raw.verdict) ? (raw.verdict as Verdict) : null;
    const recordings = Array.isArray(raw.recordings)
      ? raw.recordings.filter((file): file is string => typeof file === 'string' && RECORDING.test(file))
      : [];
    features.push({
      n: typeof raw.n === 'number' ? raw.n : features.length + 1,
      id,
      stage,
      name,
      what: str(raw.what) ?? '',
      suggest: str(raw.suggest),
      why: str(raw.why),
      verdict,
      issue: Number.isInteger(raw.issue) && (raw.issue as number) > 0 ? (raw.issue as number) : null,
      recordings,
    });
  }
  return features.sort((a, b) => a.n - b.n);
}

export function parseStages(text: string): Stage[] {
  const raw = JSON.parse(text) as unknown;
  if (!Array.isArray(raw)) return [];
  return raw.flatMap((item: Record<string, unknown>) => {
    const id = str(item?.id);
    const title = str(item?.title);
    return id && title ? [{ id, title, line: str(item.line) ?? '' }] : [];
  });
}

const DECISION_LINE = /^- (\d{4}-\d{2}-\d{2}) · (.+?) · (.+)$/;

/** Markdown links in the repository's own files keep only their label. */
function stripLinks(text: string): string {
  return text.replace(/\[([^\]]+)\]\([^)\s]*\)/g, '$1');
}

export function parseDecisions(markdown: string): DecidedLine[] {
  const lines: DecidedLine[] = [];
  for (const line of markdown.split('\n')) {
    const match = DECISION_LINE.exec(line.trimEnd());
    if (!match) continue;
    let text = match[3];
    let issue: number | null = null;
    const ref = / \(#(\d+)\)$/.exec(text);
    if (ref) {
      issue = Number(ref[1]);
      text = text.slice(0, ref.index);
    }
    lines.push({ index: lines.length, date: match[1], who: match[2], text: stripLinks(text), issue });
  }
  // Newest first; lines of the same day keep the file's order.
  return lines.sort((a, b) => (a.date === b.date ? a.index - b.index : a.date < b.date ? 1 : -1));
}

/** The short phrase beside a stage title: the first clause of its line. */
export function stagePhrase(line: string): string {
  const clause = line.split(/(?<=[.:])\s/)[0].replace(/[.:]$/, '').trim();
  if (!clause) return '';
  const first = clause.split(/\s/)[0];
  return /[A-Z]/.test(first.slice(1)) ? clause : clause[0].toLowerCase() + clause.slice(1);
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
    if (live.verdict) return { status: statusFromVerdict(live.verdict), carried: false };
  }
  return { status: statusFromVerdict(fileVerdict), carried: fileVerdict !== null };
}

function baseRow(key: string, kind: RowKind, name: string): Row {
  return {
    key, kind, name, what: null, issue: null, status: 'undecided', carried: false, fileVerdict: null, stage: null,
    suggest: null, why: null, recordings: [], author: null, up: null, comments: null, date: null, who: null, closedAt: null,
  };
}

export interface StageGroup {
  stage: Stage;
  number: number;
  rows: Row[];
  decided: number;
}

export interface Plan {
  stages: StageGroup[];
  suggestions: Row[];
  dropped: Row[];
  openDecisions: Row[];
  decided: Row[];
  planned: number;
  undecided: number;
  /** Index into `stages` of the last stage whose rows show on load. */
  lastOpenStage: number;
}

export function isDecided(row: Row): boolean {
  return row.status !== 'undecided' && row.status !== 'refine';
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

  const dropped: Row[] = [];
  const byStage = new Map<string, Row[]>();
  for (const feature of data.features) {
    const row = baseRow(`entry-${feature.id}`, 'entry', feature.name);
    row.what = feature.what;
    row.stage = feature.stage;
    row.suggest = feature.suggest;
    row.why = feature.why;
    row.recordings = feature.recordings;
    row.fileVerdict = feature.verdict;
    row.issue = byEntry.get(feature.id) ?? feature.issue;
    Object.assign(row, issueStatus(row.issue === null ? undefined : issues[row.issue], feature.verdict));
    counts(row);
    if (row.status === 'dropped') dropped.push(row);
    else byStage.set(feature.stage, [...(byStage.get(feature.stage) ?? []), row]);
  }

  const stages: StageGroup[] = [];
  data.stages.forEach((stage, index) => {
    const rows = byStage.get(stage.id) ?? [];
    if (rows.length) stages.push({ stage, number: index + 1, rows, decided: rows.filter(isDecided).length });
  });
  const firstUndecided = stages.findIndex((group) => group.rows.some((row) => !isDecided(row)));

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
      (row.status === 'dropped' ? dropped : suggestions).push(row);
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
  suggestions.sort((a, b) => (b.up ?? 0) - (a.up ?? 0) || (a.issue ?? 0) - (b.issue ?? 0));
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

  const entries = [...stages.flatMap((group) => group.rows), ...dropped.filter((row) => row.kind === 'entry')];
  return {
    stages,
    suggestions,
    dropped,
    openDecisions,
    decided,
    planned: entries.filter((row) => row.status === 'planned' || row.status === 'building' || row.status === 'built').length,
    undecided: entries.filter((row) => !isDecided(row)).length,
    lastOpenStage: firstUndecided === -1 ? stages.length - 1 : firstUndecided,
  };
}

/** Every row in page order: the plan's stages, the suggestions, the dropped, then the decisions tab. */
export function allRows(plan: Plan): Row[] {
  return [
    ...plan.stages.flatMap((group) => group.rows),
    ...plan.suggestions,
    ...plan.dropped,
    ...plan.openDecisions,
    ...plan.decided,
  ];
}

/** A row the owner still has to decide: undecided, with an issue to record the verdict on. */
export function awaitsVerdict(row: Row): boolean {
  return (row.kind === 'entry' || row.kind === 'suggestion') && row.status === 'undecided' && !row.carried && row.issue !== null;
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
