// How GitHub issues and comments become Workshop data: kinds, verdicts, the
// markers, and the cuts that bound every answer. Names, labels and markers come
// from the plan's model (tools/plan/model).
import { BUILDING_LABEL, ENTRY_LABEL, ID_PATTERN, labelVerdict, readChange, SUGGESTION_LABEL, VERDICTS } from '../../tools/plan/model/index.js';

export { BUILDING_LABEL, VERDICTS };
export const LABELS = { entry: ENTRY_LABEL, suggestion: SUGGESTION_LABEL };
export const CUTS = { body: 20_000, comment: 5000 };
const KIND_BY_LABEL = new Map(Object.entries(LABELS).map(([kind, label]) => [label, kind]));
const CHANGE_PREFIX = '<!-- silo-change: ';

const DIRECTION_CONTROLS = /[\u202A-\u202E\u2066-\u2069]/g;
const CONTROLS = /[\u0000-\u0008\u000B\u000C\u000E-\u001F\u007F-\u009F]/g;
const LINE_BREAKS = /[\n\r\t\u2028\u2029]/g;

// Text on its way out: no direction overrides or control characters; with `oneLine`, no line breaks either.
export function clean(text, { oneLine = false } = {}) {
  const value = String(text ?? '').replace(DIRECTION_CONTROLS, '').replace(CONTROLS, '');
  return oneLine ? value.replace(LINE_BREAKS, ' ') : value;
}

// A copy of `text` that shares no memory with it: a slice of a long string keeps the whole string alive.
export function fresh(text) {
  return Buffer.from(text, 'utf8').toString('utf8');
}

// Cuts text to `limit` UTF-16 units without splitting a surrogate pair; a cut text is a fresh copy.
export function cut(text, limit) {
  if (text.length <= limit) return text;
  const end = /[\uD800-\uDBFF]/.test(text[limit - 1]) ? limit - 1 : limit;
  return fresh(text.slice(0, end));
}

// A marker line the bot writes in a body: `<!-- silo-entry: <id> -->` or `<!-- silo-synced: <commit> -->`.
// Returns null for any other line, '' for a marker line whose value is not valid, else the value.
function markerOf(line, name) {
  const trimmed = line.trim();
  if (!trimmed.startsWith('<!--') || !trimmed.endsWith('-->') || trimmed.length < 7) return null;
  const inner = trimmed.slice(4, -3).trim();
  if (!inner.startsWith(`${name}:`)) return null;
  const value = inner.slice(name.length + 1).trim();
  if (name === 'silo-entry') return ID_PATTERN.test(value) ? value : '';
  return /^[0-9a-f]{40}$/.test(value) ? value : '';
}

// The entry id of an issue body, and the body without its entry and synced marker lines, in linear time.
export function readBody(raw) {
  const body = cut(raw || '', CUTS.body);
  let entry = null;
  const kept = [];
  for (const line of body.split('\n')) {
    const id = markerOf(line, 'silo-entry');
    if (id === null && markerOf(line, 'silo-synced') === null) kept.push(line);
    else if (id && entry === null) entry = id;
  }
  return { entry, text: fresh(clean(kept.join('\n').trimEnd())) };
}

// The change a bot's history comment records (its last line is the `silo-change` marker), or null.
// Only the last line is read, so a long comment costs one pass.
export function changeOf(text, bot) {
  if (!bot) return null;
  const body = text.trimEnd();
  const line = body.slice(body.lastIndexOf('\n') + 1).trim();
  return line.startsWith(CHANGE_PREFIX) && line.endsWith(' -->') ? readChange(line) : null;
}

export function labelNames(issue) {
  return (issue.labels || []).map((label) => (typeof label === 'string' ? label : label.name));
}

// The issue's kind when it carries exactly one Workshop label and is not a pull request.
export function kindOf(issue) {
  if (issue.pull_request) return null;
  const kinds = labelNames(issue).map((name) => KIND_BY_LABEL.get(name)).filter(Boolean);
  return kinds.length === 1 ? kinds[0] : null;
}

export function verdictOf(issue) {
  const verdicts = new Set(labelNames(issue).map(labelVerdict).filter(Boolean));
  for (const verdict of VERDICTS) if (verdicts.has(verdict)) return verdict;
  return null;
}

export function reasonOf(issue) {
  return issue.state === 'closed' && (issue.state_reason === 'completed' || issue.state_reason === 'not_planned') ? issue.state_reason : null;
}

// A comment as `issue` returns it.
export function commentOf(comment) {
  const full = comment.body || '';
  const text = fresh(clean(cut(full, CUTS.comment)));
  const bot = comment.user?.type === 'Bot';
  const change = full.length > CUTS.comment ? null : changeOf(text, bot);
  return {
    id: comment.id,
    author: fresh(clean(comment.user?.login ?? 'ghost', { oneLine: true })),
    avatar: comment.user?.avatar_url ?? null,
    bot,
    createdAt: comment.created_at,
    text,
    ...(full.length > CUTS.comment ? { cut: true } : {}),
    ...(change ? { change } : {}),
  };
}

// The issue number a repository-wide comment belongs to.
export function commentIssue(comment) {
  const match = /\/issues\/(\d+)$/.exec(comment.issue_url || '');
  return match ? Number(match[1]) : null;
}
