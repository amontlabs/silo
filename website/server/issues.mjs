// How GitHub issues and comments become Workshop data: kinds, the
// markers, and the cuts that bound every answer. Names, labels and markers come
// from the Workshop's model (tools/workshop/model).
import { ENTRY_LABEL, readEntryMarker, SUGGESTION_LABEL, withoutMarkers } from '../../tools/workshop/model/index.js';

export const LABELS = { entry: ENTRY_LABEL, suggestion: SUGGESTION_LABEL };
export const CUTS = { body: 20_000, comment: 5000 };
const KIND_BY_LABEL = new Map(Object.entries(LABELS).map(([kind, label]) => [label, kind]));

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

// The entry id of an issue body and the body without its markers, both read by the model on the whole body; the text is cut after.
export function readBody(raw) {
  return { entry: readEntryMarker(raw), text: fresh(clean(cut(withoutMarkers(raw), CUTS.body))) };
}

// Whether a comment was written by the Workshop's own App: its author is the App's `<slug>[bot]` account.
// `performed_via_github_app` says nothing here: GitHub sets it on a person's comment written with their token
// of the App. `app` is `{ login }`, with a null login while the installation was not read yet.
export function byApp(comment, app) {
  return Boolean(app?.login) && comment.user?.login === app.login;
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

export function reasonOf(issue) {
  return issue.state === 'closed' && (issue.state_reason === 'completed' || issue.state_reason === 'not_planned') ? issue.state_reason : null;
}

// A comment as `issue` returns it, with `up`, its 👍 count from the reaction summary GitHub returns with
// the comment.
export function commentOf(comment, app) {
  const full = comment.body || '';
  const text = fresh(clean(cut(full, CUTS.comment)));
  const bot = byApp(comment, app);
  return {
    id: comment.id,
    author: fresh(clean(comment.user?.login ?? 'ghost', { oneLine: true })),
    avatar: comment.user?.avatar_url ?? null,
    bot,
    createdAt: comment.created_at,
    text,
    up: countOf(comment.reactions?.['+1']) ?? 0,
    ...(full.length > CUTS.comment ? { cut: true } : {}),
  };
}

// The issue number a repository-wide comment belongs to.
export function commentIssue(comment) {
  const match = /\/issues\/(\d+)$/.exec(comment.issue_url || '');
  return match ? Number(match[1]) : null;
}

// A count GitHub gave, or null when it gave none.
export function countOf(value) {
  return Number.isSafeInteger(value) && value >= 0 ? value : null;
}
