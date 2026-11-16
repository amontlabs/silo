// How GitHub issues and comments become Workshop data: kinds, verdicts, the
// entry marker, and the cuts that bound every answer.

export const LABELS = { entry: 'workshop:entry', decision: 'workshop:decision', suggestion: 'workshop:suggestion' };
export const VERDICTS = ['take', 'refine', 'later', 'drop'];
export const CUTS = { body: 20_000, comment: 5000 };
const KIND_BY_LABEL = new Map(Object.entries(LABELS).map(([kind, label]) => [label, kind]));
const ENTRY_ID = /^[A-Za-z0-9_.-]{1,100}$/;

const DIRECTION_CONTROLS = /[\u202A-\u202E\u2066-\u2069]/g;
const CONTROLS = /[\u0000-\u0008\u000B\u000C\u000E-\u001F\u007F-\u009F]/g;
const LINE_BREAKS = /[\n\r\t\u2028\u2029]/g;

// Text on its way out: no direction overrides or control characters; with `oneLine`, no line breaks either.
export function clean(text, { oneLine = false } = {}) {
  const value = String(text ?? '').replace(DIRECTION_CONTROLS, '').replace(CONTROLS, '');
  return oneLine ? value.replace(LINE_BREAKS, ' ') : value;
}

// Cuts text to `limit` UTF-16 units without splitting a surrogate pair.
export function cut(text, limit) {
  if (text.length <= limit) return text;
  const end = /[\uD800-\uDBFF]/.test(text[limit - 1]) ? limit - 1 : limit;
  return text.slice(0, end);
}

function markerId(line) {
  const trimmed = line.trim();
  if (!trimmed.startsWith('<!--') || !trimmed.endsWith('-->') || trimmed.length < 7) return null;
  const inner = trimmed.slice(4, -3).trim();
  if (!inner.startsWith('silo-entry:')) return null;
  const id = inner.slice('silo-entry:'.length).trim();
  return ENTRY_ID.test(id) ? id : '';
}

// The entry id of an issue body, and the body without its marker line(s), in linear time.
export function readBody(raw) {
  const body = cut(raw || '', CUTS.body);
  let entry = null;
  const kept = [];
  for (const line of body.split('\n')) {
    const id = markerId(line);
    if (id === null) kept.push(line);
    else if (id && entry === null) entry = id;
  }
  return { entry, text: clean(kept.join('\n').trimEnd()) };
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
  const labels = labelNames(issue);
  for (const verdict of VERDICTS) if (labels.includes(`verdict:${verdict}`)) return verdict;
  return null;
}

export function reasonOf(issue) {
  return issue.state === 'closed' && (issue.state_reason === 'completed' || issue.state_reason === 'not_planned') ? issue.state_reason : null;
}

// A comment as `issue` returns it.
export function commentOf(comment) {
  const full = comment.body || '';
  const text = clean(cut(full, CUTS.comment));
  return {
    id: comment.id,
    author: clean(comment.user?.login ?? 'ghost', { oneLine: true }),
    avatar: comment.user?.avatar_url ?? null,
    bot: comment.user?.type === 'Bot',
    createdAt: comment.created_at,
    text,
    ...(full.length > CUTS.comment ? { cut: true } : {}),
  };
}

// The issue number a repository-wide comment belongs to.
export function commentIssue(comment) {
  const match = /\/issues\/(\d+)$/.exec(comment.issue_url || '');
  return match ? Number(match[1]) : null;
}
