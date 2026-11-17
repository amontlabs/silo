// What the page wrote itself, kept apart from what the server answered. A `state` answer can come from a cache up
// to a day old, and the page shows the answer it remembers before any new one, so the page's own writes are kept
// as a list, each with its time: values set on an issue (a verdict; a vote or comment count as the server answered
// it for that write; a whole issue for a new suggestion). Never a change to a count: the server patches its
// snapshot in place, so an answer may already count a write made after its `fetchedAt`, and a change would count
// it twice. The state shown is the newest answer with every write applied in order, except the writes the answer
// was fetched more than a minute after (a minute's room for the server's clock); a write is dropped after a day.
// Applying is repeatable: the same answer and the same list give the same state. The page's own comments are
// added to an `issue` answer that does not carry them yet, for six minutes.
import type { Comment } from './api';
import type { LiveIssue, LiveState } from './plan';

/** How long the page's own comments are added to an answer. */
export const KEEP_MS = 6 * 60_000;
/** How long a write to an issue is kept. */
export const KEEP_FIELDS_MS = 24 * 3600_000;
/** A state fetched more than this after a write already holds it. */
export const NEWER_MS = 60_000;
const KEY = 'silo-workshop-writes-2';
/** The store before writes were a list; its comments and set values are carried over, its counts are not. */
const OLD_KEY = 'silo-workshop-writes';

export interface Write {
  n: number;
  at: number;
  /** Values set on the issue; a whole issue for a new suggestion. */
  set: Partial<LiveIssue>;
}

interface Store {
  writes: Write[];
  comments: { n: number; at: number; comment: Comment }[];
}

const empty = (): Store => ({ writes: [], comments: [] });
// The store as last written, used only once storage has refused it: the page keeps its writes until it reloads.
let held: Store | null = null;
let refused = false;

function fresh(store: Store, now: number): Store {
  return {
    writes: store.writes.filter((write) => now >= write.at && now - write.at < KEEP_FIELDS_MS),
    comments: store.comments.filter((kept) => now >= kept.at && now - kept.at < KEEP_MS),
  };
}

function valid(raw: unknown): Store {
  const store = empty();
  if (typeof raw !== 'object' || raw === null) return store;
  const { writes, comments } = raw as Partial<Store>;
  if (Array.isArray(writes)) store.writes = writes.filter((w) => w && typeof w.n === 'number' && typeof w.at === 'number' && typeof w.set === 'object' && w.set !== null);
  if (Array.isArray(comments)) store.comments = comments.filter((c) => c && typeof c.n === 'number' && typeof c.at === 'number' && c.comment);
  return store;
}

/** The older store: per issue, one time, fields and comments. */
function carried(): Store {
  const store = empty();
  try {
    const raw = JSON.parse(localStorage.getItem(OLD_KEY) ?? '{}') as Record<string, { at?: unknown; issue?: Partial<LiveIssue>; comments?: Comment[] }>;
    for (const [key, kept] of Object.entries(raw)) {
      const n = Number(key);
      if (!kept || typeof kept.at !== 'number' || !Number.isFinite(n)) continue;
      const { up: _up, comments: _comments, ...set } = kept.issue ?? {};
      if (Object.keys(set).length) store.writes.push({ n, at: kept.at, set });
      for (const comment of kept.comments ?? []) store.comments.push({ n, at: kept.at, comment });
    }
  } catch {
    // Nothing to carry over.
  }
  return store;
}

function read(now: number): Store {
  // At build time there is no browser and nothing is kept.
  if (typeof window === 'undefined') return empty();
  try {
    if (refused && held) return fresh(held, now);
    const raw = localStorage.getItem(KEY);
    return fresh(raw === null ? carried() : valid(JSON.parse(raw)), now);
  } catch {
    return fresh(held ?? empty(), now);
  }
}

function write(store: Store) {
  held = store;
  try {
    localStorage.setItem(KEY, JSON.stringify(store));
    localStorage.removeItem(OLD_KEY);
  } catch {
    // Without storage the page still keeps its writes until it reloads.
    refused = true;
  }
}

/** Keeps a write the server confirmed, and the page's own comment with it. */
export function keep(write$: Omit<Write, 'at'>, now: number, comment?: Comment) {
  if (write$.n <= 0) return;
  const store = read(now);
  store.writes.push({ ...write$, at: now });
  if (comment) store.comments.push({ n: write$.n, at: now, comment });
  write(store);
}

function isWhole(issue: Partial<LiveIssue>): issue is LiveIssue {
  return typeof issue.kind === 'string' && typeof issue.title === 'string' && typeof issue.state === 'string'
    && typeof issue.up === 'number' && typeof issue.comments === 'number';
}

/** The answer with the page's own writes applied, in order, except those it was fetched well after. */
export function applyKept(state: LiveState, now: number): LiveState {
  const fetched = Date.parse(state.fetchedAt);
  const writes = read(now).writes.filter((write) => !(fetched > write.at + NEWER_MS)).sort((a, b) => a.at - b.at);
  if (!writes.length) return state;
  const issues = { ...state.issues };
  for (const write of writes) {
    const issue = issues[write.n];
    if (issue) issues[write.n] = { ...issue, ...write.set };
    else if (isWhole(write.set)) issues[write.n] = write.set;
  }
  return { ...state, issues };
}

/** The answer to keep after another: the newer of the two by `fetchedAt`, as the server sent it. */
export function newerAnswer(current: LiveState | null, answer: LiveState): LiveState {
  if (current && Date.parse(current.fetchedAt) > Date.parse(answer.fetchedAt)) return current;
  const { old: _old, ...next } = answer;
  return { ...next, issues: next.issues ?? {} };
}

/** The page's own comments that a cached answer does not carry yet. */
export function keptComments(n: number, comments: Comment[], now: number): Comment[] {
  const mine = read(now).comments.filter((kept) => kept.n === n).map((kept) => kept.comment);
  const missing = mine.filter((own) => !comments.some((c) => c.author === own.author && c.text.trim() === own.text.trim()));
  return missing.length ? [...comments, ...missing] : comments;
}
