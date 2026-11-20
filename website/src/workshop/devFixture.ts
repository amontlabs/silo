// Development only (`?fixture=today`, with `&as=in` to be signed in): the page on a made-up
// live state, without the API. The page's memory of the last visit is off unless `&memory=1`, so `&delay=<ms>` shows
// a first visit. main.tsx loads this only when import.meta.env.DEV, so a production build has none of it.
import { api } from './api';
import { memoryConfig } from './memory';
import type { Comment } from './api';
import type { BuildData, LiveIssue, LiveLatest, LiveState } from './plan';

const hash = (text: string) => [...text].reduce((x, ch) => (x * 31 + ch.charCodeAt(0)) >>> 0, 7);

function entryIssue(fields: Partial<LiveIssue>): LiveIssue {
  return { kind: 'entry', entry: null, title: '', state: 'open', reason: null, up: 0, comments: 0, author: 'silo-workshop[bot]', bot: true, createdAt: '2026-11-14T10:00:00Z', closedAt: null, ...fields };
}

const PEOPLE: [string, string][] = [
  ['lin-qi', 'Does this also cover a database the merge migrated?'],
  ['devin-oss', 'This is the part that would make me trust it with my main.'],
];

/** Each issue's thread: people's comments on some. */
function threads(data: BuildData): Map<string, Comment[]> {
  const out = new Map<string, Comment[]>();
  data.entries.forEach((feature, index) => {
    const comments: Comment[] = [];
    const at = (days: number) => new Date(Date.parse('2026-11-16T10:00:00Z') - days * 86_400_000 + index * 60_000).toISOString();
    if (hash(feature.id) % 4 === 0) PEOPLE.forEach(([author, text], i) => comments.push({ id: 5000 + index * 10 + i, author, avatar: '', bot: false, createdAt: at(2 - i), text, up: (hash(author) + index) % 4 }));
    out.set(feature.id, comments);
  });
  return out;
}

export function fixtureState(data: BuildData): { state: LiveState; comments: Map<number, Comment[]> } {
  const issues: Record<string, LiveIssue> = {};
  const comments = new Map<number, Comment[]>();
  const byFeature = threads(data);
  const latest: LiveLatest[] = [];
  const threadOf: Record<string, number> = {};
  data.entries.forEach((feature, index) => {
    // Every fifth entry has no thread yet.
    if (index % 5 === 4) return;
    const thread = byFeature.get(feature.id) ?? [];
    const n = 1000 + index;
    threadOf[feature.id] = n;
    issues[n] = entryIssue({
      entry: feature.id, title: feature.name, up: (hash(feature.id) % 9) + (index < 40 ? 3 : 0),
      comments: thread.length,
    });
    comments.set(n, thread);
    for (const comment of thread) latest.push({ n, entry: feature.id, author: comment.author, bot: comment.bot, createdAt: comment.createdAt, text: comment.text });
  });
  issues[9001] = entryIssue({ kind: 'suggestion', title: 'Start a task from a Slack message', author: 'mara-k', bot: false, up: 14 });
  issues[9002] = entryIssue({ kind: 'suggestion', title: 'Share one base computer across a team', author: 'devin-oss', bot: false, up: 9 });
  latest.sort((a, b) => b.createdAt.localeCompare(a.createdAt));
  return { state: { repo: 'amontlabs/silo', fetchedAt: new Date().toISOString(), issues, threads: threadOf, latest: latest.slice(0, 30) }, comments };
}

export function installFixture(data: BuildData, params: URLSearchParams): void {
  const as = params.get('as');
  const { state, comments } = fixtureState(data);
  const delay = Number(params.get('delay') ?? 0);
  memoryConfig.on = params.get('memory') === '1';
  // The session marker the server would set: signed in as someone, or signed out.
  document.cookie = as === 'in' ? 'workshop-hint=1; path=/' : 'workshop-hint=; expires=Thu, 01 Jan 1970 00:00:00 GMT; path=/';
  api.state = () => new Promise((resolve) => setTimeout(() => resolve(state), delay));
  const wait = <T,>(ms: number, value: T) => new Promise<T>((resolve) => setTimeout(() => resolve(value), ms));
  // `me` waits as long as `delay`; `detail=<ms>` holds back each issue's comments.
  const detail = Number(params.get('detail') ?? 0);
  // `login=<name>` replaces the fake login, to see a long one.
  const login = params.get('login');
  api.me = async () => wait(delay, as === 'in' ? { login: login ?? 'visitor-fake', avatar: '' } : null);
  api.issue = async (n: number) => wait(detail, {
    number: n, title: state.issues[n]?.title ?? '', body: '', url: `https://github.com/amontlabs/silo/issues/${n}`, up: state.issues[n]?.up ?? 0,
    comments: comments.get(n) ?? [],
  });
  api.myVote = async () => ({ on: false, comments: [] });
  for (const write of ['comment', 'vote', 'commentVote', 'logout'] as const) api[write] = async () => ({});
}
