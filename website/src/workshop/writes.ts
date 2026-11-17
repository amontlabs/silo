// What the page wrote itself. A `state` answer can come from a cache up to a day old, and the page shows the state
// it remembers before any answer, so the page's own values for an issue win over any state fetched before the
// write (with a minute's room for the server's clock), across reloads too, for a day at most. Its own comments
// are added to an `issue` answer that does not carry them yet, for six minutes.
import type { Comment } from './api';
import type { LiveIssue, LiveState } from './plan';

/** How long the page's own comments are added to an answer. */
export const KEEP_MS = 6 * 60_000;
/** How long the page's own values for an issue are kept. */
export const KEEP_FIELDS_MS = 24 * 3600_000;
/** A state fetched more than this after a write already holds it. */
export const NEWER_MS = 60_000;
const KEY = 'silo-workshop-writes';

export interface Kept {
  /** When the issue's fields were last written. */
  at: number;
  /** Fields of the issue after the write; a whole issue for a new suggestion. */
  issue: Partial<LiveIssue>;
  comments: Comment[];
  /** When a comment was last added; older entries have only `at`. */
  commentsAt?: number;
}

type Store = Record<string, Kept>;

const commentsAt = (kept: Kept) => kept.commentsAt ?? kept.at;
const fieldsFresh = (kept: Kept, now: number) => now >= kept.at && now - kept.at < KEEP_FIELDS_MS;
const commentsFresh = (kept: Kept, now: number) => now >= commentsAt(kept) && now - commentsAt(kept) < KEEP_MS;

function read(now: number): Store {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? '{}') as Store;
    const fresh: Store = {};
    for (const [n, kept] of Object.entries(raw)) {
      if (!kept || typeof kept.at !== 'number' || !kept.issue || !Array.isArray(kept.comments)) continue;
      const fields = fieldsFresh(kept, now);
      const comments = commentsFresh(kept, now);
      if (fields || comments) fresh[n] = { ...kept, issue: fields ? kept.issue : {}, comments: comments ? kept.comments : [] };
    }
    return fresh;
  } catch {
    return {};
  }
}

function write(store: Store) {
  try {
    localStorage.setItem(KEY, JSON.stringify(store));
  } catch {
    // Without storage the page still keeps its values until it reloads.
  }
}

export function keep(n: number, issue: Partial<LiveIssue>, now: number, comment?: Comment) {
  const store = read(now);
  const current = store[n];
  const fields = Object.keys(issue).length > 0;
  store[n] = {
    at: fields || !current ? now : current.at,
    issue: { ...current?.issue, ...issue },
    comments: comment ? [...(current?.comments ?? []), comment] : current?.comments ?? [],
    commentsAt: comment ? now : current ? commentsAt(current) : now,
  };
  write(store);
}

function isWhole(issue: Partial<LiveIssue>): issue is LiveIssue {
  return typeof issue.kind === 'string' && typeof issue.title === 'string' && typeof issue.state === 'string';
}

/** The state with the page's own writes over it, except where the state was fetched well after them. */
export function applyKept(state: LiveState, now: number): LiveState {
  const store = read(now);
  const fetched = Date.parse(state.fetchedAt);
  const issues = { ...state.issues };
  for (const [n, kept] of Object.entries(store)) {
    if (!Object.keys(kept.issue).length || fetched > kept.at + NEWER_MS) continue;
    const base = issues[n];
    if (base) issues[n] = { ...base, ...kept.issue };
    else if (isWhole(kept.issue)) issues[n] = kept.issue;
  }
  return { ...state, issues };
}

/** What the page shows after an answer: the answer with the page's writes over it, unless the state shown is newer. */
export function afterAnswer(shown: LiveState | null, answer: LiveState, now: number): LiveState {
  if (shown && Date.parse(shown.fetchedAt) > Date.parse(answer.fetchedAt)) return shown;
  const { old: _old, ...next } = answer;
  return applyKept({ ...next, issues: next.issues ?? {} }, now);
}

/** The page's own comments that a cached answer does not carry yet. */
export function keptComments(n: number, comments: Comment[], now: number): Comment[] {
  const mine = read(now)[n]?.comments ?? [];
  const missing = mine.filter((own) => !comments.some((c) => c.author === own.author && c.text.trim() === own.text.trim()));
  return missing.length ? [...comments, ...missing] : comments;
}
