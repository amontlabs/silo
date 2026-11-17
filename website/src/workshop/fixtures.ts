// A small plan in the files of plan/, for the tests: they never read the real one.
import { canonicalText, changeComment } from '../../../tools/plan/model/index.js';
import { buildDataFrom, notesFrom } from './data';
import type { Comment } from './api';
import type { BuildData, Change, LiveIssue, LiveLatest, LiveState, Notes } from './plan';

interface FixtureFeature {
  id: string;
  stage: string;
  area?: string;
  name: string;
  what: string;
  verdict?: 'take' | 'later' | 'drop';
  built?: string;
  suggest?: { verdict: 'take' | 'later' | 'drop'; why?: string };
}

/** In build order. */
export const FEATURES: FixtureFeature[] = [
  { id: 'engine', stage: 'shell', area: 'engine', name: 'The engine runs on its own', what: 'A background engine owns every computer.', verdict: 'take', suggest: { verdict: 'take', why: 'The core piece.' } },
  { id: 'calls', stage: 'shell', area: 'engine', name: 'Every call defined once', what: 'Each call is written down once.', verdict: 'take', suggest: { verdict: 'take' } },
  { id: 'computers', stage: 'shell', area: 'computers', name: 'Create and delete a computer', what: 'Make a Linux computer.', suggest: { verdict: 'take', why: 'Key records by id.' } },
  { id: 'exec', stage: 'shell', area: 'computers', name: 'Run a command', what: 'Run a command inside.', suggest: { verdict: 'take' } },
  { id: 'cli', stage: 'shell', area: 'app', name: 'One silo command', what: 'One command.', suggest: { verdict: 'take' } },
  { id: 'fork', stage: 'loop', area: 'forks', name: 'Fork a computer', what: 'Copy a computer.', suggest: { verdict: 'take' } },
  { id: 'checkpoint', stage: 'loop', area: 'checkpoints', name: 'Save a checkpoint', what: 'Save and go back.', suggest: { verdict: 'take' } },
  { id: 'diff', stage: 'loop', area: 'changes', name: 'See what a fork changed', what: 'The diff.', suggest: { verdict: 'take' } },
  { id: 'agent', stage: 'agent', area: 'agents', name: 'Install your agent', what: 'Claude Code or Codex.', suggest: { verdict: 'take' } },
  { id: 'shells', stage: 'living', area: 'setup', name: 'Your shell inside', what: 'Your dotfiles.', verdict: 'take', suggest: { verdict: 'take' } },
  { id: 'secrets', stage: 'living', area: 'access', name: 'Secrets you cannot read', what: 'A placeholder inside.', suggest: { verdict: 'take' } },
  { id: 'triggers', stage: 'tasks', area: 'tasks', name: 'Tasks from GitHub events', what: 'A label starts a task.', suggest: { verdict: 'later' } },
  { id: 'devices', stage: 'devices', area: 'devices', name: 'A Linux server', what: 'Run on Linux.', suggest: { verdict: 'later' } },
  { id: 'release', stage: 'running', area: 'releases', name: 'Signed releases', what: 'Signed builds.', suggest: { verdict: 'take' } },
  { id: 'audit', stage: 'trust', area: 'trust', name: 'Who did what', what: 'Every call recorded.', suggest: { verdict: 'take' } },
  { id: 'screen', stage: 'desktop', area: 'desktop', name: 'A desktop in a computer', what: 'A shared screen.', suggest: { verdict: 'later' } },
  { id: 'macos', stage: 'macos', area: 'macos', name: 'macOS computers', what: 'macOS inside.', suggest: { verdict: 'later' } },
  { id: 'export', stage: 'extras', area: 'sharing', name: 'Share a computer as a file', what: 'Export and import.', suggest: { verdict: 'later' } },
  { id: 'reviewer', stage: 'gone', name: 'A hosted reviewer', what: 'A hosted service.', verdict: 'drop', suggest: { verdict: 'drop' } },
  { id: 'old-desktop', stage: 'gone', name: 'An earlier desktop stack', what: 'KasmVNC.', suggest: { verdict: 'drop' } },
];

export const AREAS = [
  { id: 'engine', title: 'Engine', line: 'The core. Everything else talks to it.' },
  { id: 'computers', title: 'Computers', line: 'A whole virtual machine for your agent.' },
  { id: 'app', title: 'App and command line', line: 'A Mac app and one command.' },
  { id: 'checkpoints', title: 'Checkpoints', line: 'Save a computer and go back to it.' },
  { id: 'forks', title: 'Forks', line: 'A copy of a computer for each task.' },
  { id: 'changes', title: 'Diff and merge', line: 'See everything a copy changed.' },
  { id: 'agents', title: 'Agents inside', line: 'Your agent lives inside.' },
  { id: 'access', title: 'Access', line: 'Repositories, secrets and the network.' },
  { id: 'setup', title: 'Your setup', line: 'Your tools, inside.' },
  { id: 'tasks', title: 'Tasks from outside', line: 'Work started by an event.' },
  { id: 'devices', title: 'Other devices', line: 'Computers on a server.' },
  { id: 'releases', title: 'Releases', line: 'Signed builds and updates.' },
  { id: 'trust', title: 'Trust', line: 'Who did what.' },
  { id: 'desktop', title: 'Desktop', line: 'A screen you share.' },
  { id: 'macos', title: 'macOS computers', line: 'macOS inside.' },
  { id: 'sharing', title: 'Sharing', line: 'Move a computer as a file.' },
];

export const STAGES = [
  { id: 'shell', title: 'Shell', line: 'One computer you can open from the app and the command line.' },
  { id: 'loop', title: 'The loop', line: 'Checkpoint, fork, see what changed, merge or drop.' },
  { id: 'agent', title: 'Agent inside', line: 'Your agent lives inside.' },
  { id: 'living', title: 'Living in it', line: 'Your setup is inside main. This is the starter.' },
  { id: 'tasks', title: 'Tasks', line: 'From outside.' },
  { id: 'devices', title: 'Other devices', line: 'The same computers, on a Linux server.' },
  { id: 'running', title: 'Running', line: 'Releases.' },
  { id: 'trust', title: 'Trust', line: 'Proof.' },
  { id: 'desktop', title: 'Desktop', line: 'A screen.' },
  { id: 'macos', title: 'macOS', line: 'macOS guests.' },
  { id: 'extras', title: 'Extras', line: 'Sharing.' },
  { id: 'gone', title: 'Not carried over', line: 'Dropped.' },
];

export const DECISIONS = [
  { id: 'security-properly', date: '2026-11-15', text: 'Security is built properly. It is not the focus yet.' },
  { id: 'engine-in-zig', date: '2026-11-16', text: 'The engine is written in Zig. It is the core piece.', about: ['engine'] },
  { id: 'merge-main-only', date: '2026-11-16', text: 'Merging changes main only. Nothing else moves.', about: ['changes', 'forks'] },
];

/** The notes of `engine`: every kind of block, a relative link and a link to another scheme. */
export const ENGINE_NOTES = `## Notes

The engine owns every computer. See [the design](https://example.com/design) and [a file](../../docs/x.md).

## From the old code

Lessons:
- It must restart cleanly.
- Calls are \`engine.call\`:
  - one per verb
1. First
2. Second

\`\`\`
silo engine start
\`\`\`
`;

/** The files of the fixture plan, by their path under plan/. */
export function planFiles({ features = FEATURES, notes = { engine: ENGINE_NOTES } as Record<string, string>, decisions = DECISIONS as unknown[] } = {}): Map<string, string> {
  const files = new Map<string, string>();
  files.set('PRODUCT.md', 'Silo.\n');
  files.set('areas.json', canonicalText(AREAS));
  files.set('order.json', canonicalText({ stages: STAGES.map((stage) => ({ ...stage, features: features.filter((f) => f.stage === stage.id).map((f) => f.id) })) }));
  files.set('decisions.json', canonicalText(decisions));
  for (const feature of features) {
    const file: Record<string, unknown> = { name: feature.name, what: feature.what };
    if (feature.area) file.area = feature.area;
    file.needs = [];
    if (feature.suggest) file.suggest = feature.suggest;
    if (feature.verdict) file.verdict = feature.verdict;
    if (feature.built) file.built = feature.built;
    files.set(`features/${feature.id}.json`, canonicalText(file));
    if (notes[feature.id]) files.set(`features/${feature.id}.md`, notes[feature.id]);
  }
  return files;
}

const NOTES = new WeakMap<BuildData, Notes>();

export function buildData(options?: Parameters<typeof planFiles>[0]): BuildData {
  const files = planFiles(options);
  const data = buildDataFrom(files);
  NOTES.set(data, notesFrom(files));
  return data;
}

/** The notes of a fixture plan made by `buildData`, as the page's notes chunk gives them. */
export function fixtureNotes(data: BuildData): Notes {
  return NOTES.get(data) ?? {};
}

const day = '2026-11-14T10:00:00Z';

export function issue(fields: Partial<LiveIssue>): LiveIssue {
  return { kind: 'entry', entry: null, title: '', state: 'open', reason: null, verdict: null, up: 0, comments: 0, author: 'silo-workshop[bot]', bot: true, createdAt: day, closedAt: null, ...fields };
}

export const SHA = { a: 'a'.repeat(40), b: 'b'.repeat(40), c: 'c'.repeat(40), d: 'd'.repeat(40) };

/** A history comment of the bot, as `GET issue` gives it. */
export function historyComment(id: number, change: Change, createdAt: string): Comment {
  return { id, author: 'silo-workshop[bot]', avatar: '', bot: true, createdAt, text: changeComment(change), change };
}

/** Live state: the engine is taken on GitHub, `computers` has votes, comments and history lines. */
export function liveState(extra: Record<string, LiveIssue> = {}, latest: LiveLatest[] = []): LiveState {
  return {
    repo: 'amontlabs/silo',
    fetchedAt: new Date(Date.now()).toISOString(),
    issues: {
      10: issue({ entry: 'engine', title: 'The engine runs on its own', verdict: 'take', up: 8 }),
      11: issue({ entry: 'calls', title: 'Every call defined once', up: 3 }),
      12: issue({ entry: 'computers', title: 'Create and delete a computer', up: 12, comments: 5, updates: 3 }),
      13: issue({ entry: 'exec', title: 'Run a command', up: 4 }),
      14: issue({ entry: 'fork', title: 'Fork a computer', up: 21 }),
      15: issue({ entry: 'shells', title: 'Your shell inside', verdict: 'take' }),
      16: issue({ entry: 'devices', title: 'A Linux server' }),
      17: issue({ entry: 'reviewer', title: 'A hosted reviewer' }),
      30: issue({ kind: 'suggestion', title: 'Start a task from Slack', up: 9, author: 'devin-oss', bot: false }),
      31: issue({ kind: 'suggestion', title: 'Share a base computer', up: 14, author: 'mara-k', bot: false }),
      ...extra,
    },
    latest,
  };
}
