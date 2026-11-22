// What the page wrote itself, kept apart from what the server answered. A `state` answer can come from a cache up
// to a day old, and the page shows the answer it remembers before any new one, so the page's own writes are kept
// as a list, each with its time: values set on an issue (a vote or comment count as the server answered
// it for that write; a whole issue for a new suggestion). Never a change to a count: the server patches its
// snapshot in place, so an answer may already count a write made after its `fetchedAt`, and a change would count
// it twice. The state shown is the newest answer with every write applied in order, except the writes the answer
// was fetched more than a minute after (a minute's room for the server's clock); a write is dropped after a day.
// Applying is repeatable: the same answer and the same list give the same state. The page's own comments are
// added to an `issue` answer that does not carry them yet, for six minutes, and so is the 👍 count the server answered
// for a comment's vote.
// A write that opened an entry's thread says so (`opens`): the answer a cached state gave does not hold the thread yet,
// so the thread is added to it, never replacing one it already has.
// The owner's verdicts are kept too: a verdict changes the plan, which only a new build of the site carries, so it is
// shown over whatever build the page is, until a build's plan shows that verdict for the entry (the deploy of another
// push may go live first), and then forgotten; after a day it is forgotten anyway.
import type { Comment } from './api';
import type { LiveIssue, LiveState, Verdict } from './plan';

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
  /** When the write was sent: writes apply in the order they were sent, not the order their answers came. */
  at: number;
  /** Which of two writes sent in the same millisecond came first. */
  seq?: number;
  /** Values set on the issue; a whole issue for a new suggestion. */
  set: Partial<LiveIssue>;
  /** The entry whose thread this write made or found, with the issue as it was before the write. */
  opens?: { entry: string; issue: LiveIssue };
}

/** A verdict the owner wrote from this page: the entry, the verdict, and when. */
export interface KeptVerdict {
  entry: string;
  verdict: Verdict;
  at: number;
}

interface Store {
  writes: Write[];
  verdicts: KeptVerdict[];
  comments: { n: number; at: number; comment: Comment }[];
  /** The 👍 count of a comment as the server answered it for the person's own vote. */
  ups: { n: number; id: string | number; at: number; up: number }[];
}

const empty = (): Store => ({ writes: [], verdicts: [], comments: [], ups: [] });
// The store as last written, used only once storage has refused it: the page keeps its writes until it reloads.
let held: Store | null = null;
let refused = false;

function fresh(store: Store, now: number): Store {
  return {
    writes: store.writes.filter((write) => now >= write.at && now - write.at < KEEP_FIELDS_MS),
    verdicts: store.verdicts.filter((kept) => now >= kept.at && now - kept.at < KEEP_FIELDS_MS),
    comments: store.comments.filter((kept) => now >= kept.at && now - kept.at < KEEP_MS),
    ups: store.ups.filter((kept) => now >= kept.at && now - kept.at < KEEP_MS),
  };
}

function valid(raw: unknown): Store {
  const store = empty();
  if (typeof raw !== 'object' || raw === null) return store;
  const { writes, verdicts, comments, ups } = raw as Partial<Store>;
  // A store an older page wrote has no verdicts.
  if (Array.isArray(verdicts)) store.verdicts = verdicts.filter((v) => v && typeof v.entry === 'string' && (v.verdict === 'take' || v.verdict === 'drop') && typeof v.at === 'number');
  if (Array.isArray(writes)) store.writes = writes.filter((w) => w && typeof w.n === 'number' && typeof w.at === 'number' && typeof w.set === 'object' && w.set !== null);
  if (Array.isArray(comments)) store.comments = comments.filter((c) => c && typeof c.n === 'number' && typeof c.at === 'number' && c.comment);
  if (Array.isArray(ups)) store.ups = ups.filter((u) => u && typeof u.n === 'number' && typeof u.at === 'number' && typeof u.up === 'number' && (typeof u.id === 'number' || typeof u.id === 'string'));
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

/** Keeps a write the server confirmed, and the page's own comment with it; `at` is when it was sent, else `now`. */
export function keep(write$: Omit<Write, 'at'>, now: number, comment?: Comment, at: number = now) {
  if (write$.n <= 0) return;
  const store = read(now);
  store.writes.push({ ...write$, at });
  if (comment) store.comments.push({ n: write$.n, at, comment });
  write(store);
}

/** Keeps a verdict the server wrote on main; a later one on the same entry replaces it. */
export function keepVerdict(verdict: Omit<KeptVerdict, 'at'>, now: number) {
  const store = read(now);
  store.verdicts = [...store.verdicts.filter((kept) => kept.entry !== verdict.entry), { ...verdict, at: now }];
  write(store);
}

/**
 * The verdicts kept, by entry. One the page's built plan already shows (`carried`) is forgotten: the plan holds it now,
 * whatever build the page is, and a build served after it shows its own plan.
 */
export function keptVerdicts(carried: (entry: string, verdict: Verdict) => boolean, now: number): Map<string, Verdict> {
  const store = read(now);
  const left = store.verdicts.filter((kept) => !carried(kept.entry, kept.verdict));
  if (left.length !== store.verdicts.length) {
    store.verdicts = left;
    write(store);
  }
  return new Map(left.map((kept) => [kept.entry, kept.verdict]));
}

/** Keeps the 👍 count the server answered for the person's vote on a comment. */
export function keepCommentUp(n: number, id: string | number, up: number, now: number) {
  if (n <= 0) return;
  const store = read(now);
  store.ups.push({ n, id, at: now, up });
  write(store);
}

function isWhole(issue: Partial<LiveIssue>): issue is LiveIssue {
  return typeof issue.kind === 'string' && typeof issue.title === 'string' && typeof issue.state === 'string'
    && typeof issue.up === 'number' && typeof issue.comments === 'number';
}

/** The answer with the page's own writes applied, in order, except those it was fetched well after. */
export function applyKept(state: LiveState, now: number): LiveState {
  const fetched = Date.parse(state.fetchedAt);
  const writes = read(now).writes.filter((write) => !(fetched > write.at + NEWER_MS)).sort((a, b) => a.at - b.at || (a.seq ?? 0) - (b.seq ?? 0));
  if (!writes.length) return state;
  const issues = { ...state.issues };
  const threads = { ...state.threads };
  for (const write of writes) {
    const opens = write.opens;
    if (opens && !(write.n in issues)) issues[write.n] = opens.issue;
    if (opens && threads[opens.entry] === undefined) threads[opens.entry] = write.n;
    const issue = issues[write.n];
    if (issue) issues[write.n] = { ...issue, ...write.set };
    else if (isWhole(write.set)) issues[write.n] = write.set;
  }
  return { ...state, issues, threads };
}

/** The answer to keep after another: the newer of the two by `fetchedAt`, as the server sent it. */
export function newerAnswer(current: LiveState | null, answer: LiveState): LiveState {
  if (current && Date.parse(current.fetchedAt) > Date.parse(answer.fetchedAt)) return current;
  const { old: _old, ...next } = answer;
  return { ...next, issues: next.issues ?? {} };
}

/**
 * The page's own comments that a cached answer does not carry yet, and the 👍 counts the server answered for the
 * person's own votes on comments, except those the answer was fetched (`now`) more than a minute after.
 */
export function keptComments(n: number, comments: Comment[], now: number): Comment[] {
  const store = read(now);
  const mine = store.comments.filter((kept) => kept.n === n).map((kept) => kept.comment);
  const missing = mine.filter((own) => !comments.some((c) => c.author === own.author && c.text.trim() === own.text.trim()));
  const all = missing.length ? [...comments, ...missing] : comments;
  const ups = store.ups.filter((kept) => kept.n === n && !(now > kept.at + NEWER_MS)).sort((a, b) => a.at - b.at);
  if (!ups.length) return all;
  return all.map((comment) => {
    const set = ups.filter((kept) => String(kept.id) === String(comment.id)).pop();
    return set ? { ...comment, up: set.up } : comment;
  });
}
