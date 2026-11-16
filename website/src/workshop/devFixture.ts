// Development only (`?fixture=today` or `?fixture=later`, with `&as=owner` or `&as=visitor`): the page on a made-up
// live state, without the API. main.tsx loads this only when import.meta.env.DEV, so a production build has none of it.
import { api } from './api';
import type { BuildData, LiveIssue, LiveState, Verdict } from './plan';

/** Areas the "later" state shows as built; Diff and merge is under way, the agent's areas are decided. */
const BUILT = new Set(['engine', 'app', 'computers', 'checkpoints', 'forks']);
const AHEAD = new Set(['agents', 'access', 'setup']);

const hash = (text: string) => [...text].reduce((x, ch) => (x * 31 + ch.charCodeAt(0)) >>> 0, 7);
const asVerdict = (value: string | null): Verdict | null => (value === 'take' || value === 'later' || value === 'drop' || value === 'refine' ? value : null);

function entryIssue(fields: Partial<LiveIssue>): LiveIssue {
  return { kind: 'entry', entry: null, title: '', state: 'open', reason: null, verdict: null, up: 0, comments: 0, author: 'silo-workshop[bot]', bot: true, createdAt: '2026-11-14T10:00:00Z', closedAt: null, ...fields };
}

export function fixtureState(data: BuildData, when: 'today' | 'later'): LiveState {
  const issues: Record<string, LiveIssue> = {};
  for (const feature of data.features) {
    const base = entryIssue({ entry: feature.id, title: feature.name, verdict: feature.verdict, up: (hash(feature.id) % 9) + (feature.n < 40 ? 3 : 0), comments: hash(feature.id) % 4 === 0 ? 2 : 0 });
    let issue = base;
    if (when === 'later' && feature.area) {
      const suggested = asVerdict(feature.suggest === 'refine' ? null : feature.suggest);
      if (BUILT.has(feature.area)) {
        issue = suggested === 'later' || suggested === 'drop' ? { ...base, verdict: suggested, ...(suggested === 'drop' ? { state: 'closed', reason: 'not_planned' } : {}) } : { ...base, verdict: 'take', state: 'closed', reason: 'completed', closedAt: '2026-11-15T10:00:00Z' };
      } else if (feature.area === 'changes') {
        issue = feature.n <= 20 ? { ...base, verdict: 'take', state: 'closed', reason: 'completed', closedAt: '2026-11-15T10:00:00Z' } : feature.n <= 22 ? { ...base, verdict: 'take', building: true } : { ...base, verdict: suggested === 'drop' ? null : suggested };
      } else if (AHEAD.has(feature.area)) {
        issue = { ...base, verdict: suggested === 'drop' ? null : suggested };
      }
    }
    issues[1000 + feature.n] = issue;
  }
  issues[9001] = entryIssue({ kind: 'suggestion', title: 'Start a task from a Slack message', author: 'mara-k', bot: false, up: 14 });
  issues[9002] = entryIssue({ kind: 'suggestion', title: 'Share one base computer across a team', author: 'devin-oss', bot: false, up: 9 });
  issues[9101] = entryIssue({ kind: 'decision', title: 'After a task ends, what happens to its computer?', comments: 3 });
  return { repo: 'amontlabs/silo', fetchedAt: new Date().toISOString(), issues };
}

export function installFixture(data: BuildData, params: URLSearchParams): void {
  const when = params.get('fixture') === 'later' ? 'later' : 'today';
  const as = params.get('as');
  const state = fixtureState(data, when);
  const delay = Number(params.get('delay') ?? 0);
  api.state = () => new Promise((resolve) => setTimeout(() => resolve(state), delay));
  const wait = <T,>(ms: number, value: T) => new Promise<T>((resolve) => setTimeout(() => resolve(value), ms));
  // `me` waits as long as `delay`; `detail=<ms>` holds back each issue's comments.
  const detail = Number(params.get('detail') ?? 0);
  // `login=<name>` replaces the fake login, to see a long one.
  const login = params.get('login');
  api.me = async () => wait(delay, as === 'owner' ? { login: login ?? 'owner-fake', avatar: '', canDecide: true } : as === 'visitor' ? { login: login ?? 'visitor-fake', avatar: '', canDecide: false } : null);
  api.issue = async (n: number) => wait(detail, {
    number: n, title: state.issues[n]?.title ?? '', body: '', url: `https://github.com/amontlabs/silo/issues/${n}`, up: state.issues[n]?.up ?? 0,
    comments: (state.issues[n]?.comments ?? 0) > 0 ? [
      { id: `${n}-1`, author: 'lin-qi', avatar: '', bot: false, createdAt: '2026-11-14T10:00:00Z', text: 'Does this also cover a database the merge migrated?' },
      { id: `${n}-2`, author: 'devin-oss', avatar: '', bot: false, createdAt: '2026-11-15T10:00:00Z', text: 'This is the part that would make me trust it with my main.' },
    ] : [],
  });
  api.myVote = async () => false;
  for (const write of ['comment', 'vote', 'verdict', 'decide', 'logout'] as const) api[write] = async () => ({});
}
