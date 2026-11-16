// What the page wrote itself. A cached `state` or `issue` answer can be up to six minutes old, so for
// that long the page's own values for an issue win over what the server sends, across reloads too.
import type { Comment } from './api';
import type { LiveIssue, LiveState } from './plan';

export const KEEP_MS = 6 * 60_000;
const KEY = 'silo-workshop-writes';

export interface Kept {
  at: number;
  /** Fields of the issue after the write; a whole issue for a new suggestion. */
  issue: Partial<LiveIssue>;
  comments: Comment[];
}

type Store = Record<string, Kept>;

function read(now: number): Store {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) ?? '{}') as Store;
    const fresh: Store = {};
    for (const [n, kept] of Object.entries(raw)) {
      if (kept && typeof kept.at === 'number' && now - kept.at < KEEP_MS && now >= kept.at) fresh[n] = kept;
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
  store[n] = {
    at: now,
    issue: { ...current?.issue, ...issue },
    comments: comment ? [...(current?.comments ?? []), comment] : current?.comments ?? [],
  };
  write(store);
}

function isWhole(issue: Partial<LiveIssue>): issue is LiveIssue {
  return typeof issue.kind === 'string' && typeof issue.title === 'string' && typeof issue.state === 'string';
}

export function applyKept(state: LiveState, now: number): LiveState {
  const store = read(now);
  const issues = { ...state.issues };
  for (const [n, kept] of Object.entries(store)) {
    const base = issues[n];
    if (base) issues[n] = { ...base, ...kept.issue };
    else if (isWhole(kept.issue)) issues[n] = kept.issue;
  }
  return { ...state, issues };
}

/** The page's own comments that a cached answer does not carry yet. */
export function keptComments(n: number, comments: Comment[], now: number): Comment[] {
  const mine = read(now)[n]?.comments ?? [];
  const missing = mine.filter((own) => !comments.some((c) => c.author === own.author && c.text.trim() === own.text.trim()));
  return missing.length ? [...comments, ...missing] : comments;
}
