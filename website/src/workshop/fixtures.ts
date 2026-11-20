// A small plan in the files of plan/, for the tests: they never read the real one.
import { canonicalText } from '../../../tools/plan/model/index.js';
import { changeComment } from '../../../tools/workshop/model/index.js';
import { buildDataFrom, notesFrom } from './data';
import { EXAMPLE_MAP } from './map-example';
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
  { id: 'core', stage: 'second', area: 'part-a', name: 'The engine runs on its own', what: 'A background engine owns every computer.', verdict: 'take', suggest: { verdict: 'take', why: 'The core piece.' } },
  { id: 'calls', stage: 'second', area: 'part-a', name: 'Every call defined once', what: 'Each call is written down once.', verdict: 'take', suggest: { verdict: 'take' } },
  { id: 'create', stage: 'second', area: 'part-b', name: 'Create and delete a computer', what: 'Make a Linux computer.', suggest: { verdict: 'take', why: 'Key records by id.' } },
  { id: 'exec', stage: 'second', area: 'part-b', name: 'Run a command', what: 'Run a command inside.', suggest: { verdict: 'take' } },
  { id: 'cli', stage: 'second', area: 'part-d', name: 'One silo command', what: 'One command.', suggest: { verdict: 'take' } },
  { id: 'fork', stage: 'third', area: 'part-d', name: 'Fork a computer', what: 'Copy a computer.', suggest: { verdict: 'take' } },
  { id: 'checkpoint', stage: 'third', area: 'part-d', name: 'Save a checkpoint', what: 'Save and go back.', suggest: { verdict: 'take' } },
  { id: 'diff', stage: 'third', area: 'part-d', name: 'See what a fork changed', what: 'The diff.', suggest: { verdict: 'take' } },
  { id: 'install-agent', stage: 'fourth', area: 'part-d', name: 'Install your agent', what: 'Claude Code or Codex.', suggest: { verdict: 'take' } },
  { id: 'shells', stage: 'fifth', area: 'part-c', name: 'Your shell inside', what: 'Your dotfiles.', verdict: 'take', suggest: { verdict: 'take' } },
  { id: 'secrets', stage: 'fifth', area: 'part-d', name: 'Secrets you cannot read', what: 'A placeholder inside.', suggest: { verdict: 'take' } },
  { id: 'triggers', stage: 'sixth', area: 'part-e', name: 'Tasks from GitHub events', what: 'A label starts a task.', suggest: { verdict: 'later' } },
  { id: 'linux', stage: 'seventh', area: 'part-e', name: 'A Linux server', what: 'Run on Linux.', suggest: { verdict: 'later' } },
  { id: 'release', stage: 'eighth', area: 'part-e', name: 'Signed releases', what: 'Signed builds.', suggest: { verdict: 'take' } },
  { id: 'audit', stage: 'ninth', area: 'part-e', name: 'Who did what', what: 'Every call recorded.', suggest: { verdict: 'take' } },
  { id: 'screen', stage: 'tenth', area: 'part-e', name: 'A desktop in a computer', what: 'A shared screen.', suggest: { verdict: 'later' } },
  { id: 'mac-guests', stage: 'eleventh', area: 'part-e', name: 'macOS computers', what: 'macOS inside.', suggest: { verdict: 'later' } },
  { id: 'export', stage: 'twelfth', area: 'part-e', name: 'Share a computer as a file', what: 'Export and import.', suggest: { verdict: 'later' } },
  { id: 'reviewer', stage: 'gone', name: 'A hosted reviewer', what: 'A hosted service.', verdict: 'drop', suggest: { verdict: 'drop' } },
  { id: 'old-desktop', stage: 'gone', name: 'An earlier desktop stack', what: 'KasmVNC.', suggest: { verdict: 'drop' } },
  { id: 'intake', stage: 'first', area: 'part-d', name: 'How research enters the plan', what: 'Where a new line goes.', suggest: { verdict: 'take' } },
];

/** The parts of the example map (map-example.ts), which the tests draw this plan with. */
export const AREAS = [
  { id: 'part-a', title: 'Part A', line: 'A tall part with a glyph.' },
  { id: 'part-b', title: 'Part B', line: 'A part that holds another.' },
  { id: 'part-c', title: 'Part C', line: 'A part inside Part B.' },
  { id: 'part-d', title: 'Part D', line: 'A part with copies behind it.' },
  { id: 'part-e', title: 'Part E', line: 'A wide bar under the drawing.' },
];

export const STAGES = [
  { id: 'first', title: 'First', line: 'The first stage.' },
  { id: 'second', title: 'Second', line: 'The second stage.' },
  { id: 'third', title: 'Third', line: 'The third stage.' },
  { id: 'fourth', title: 'Fourth', line: 'The fourth stage.' },
  { id: 'fifth', title: 'Fifth', line: 'The fifth stage, the last of the first band.' },
  { id: 'sixth', title: 'Sixth', line: 'The sixth stage.' },
  { id: 'seventh', title: 'Seventh', line: 'The seventh stage.' },
  { id: 'eighth', title: 'Eighth', line: 'The eighth stage.' },
  { id: 'ninth', title: 'Ninth', line: 'The ninth stage.' },
  { id: 'tenth', title: 'Tenth', line: 'The tenth stage.' },
  { id: 'eleventh', title: 'Eleventh', line: 'The eleventh stage.' },
  { id: 'twelfth', title: 'Twelfth', line: 'The twelfth stage.' },
  { id: 'gone', title: 'Gone', line: 'Dropped.' },
];

export const DECISIONS = [
  { id: 'security-properly', date: '2026-11-15', text: 'Security is built properly. It is not the focus yet.' },
  { id: 'core-in-zig', date: '2026-11-16', text: 'The core is written in Zig. It is the core piece.', about: ['part-a'] },
  { id: 'one-way', date: '2026-11-16', text: 'Changes go one way only. Nothing else moves.', about: ['part-d', 'part-e'] },
];

/** The notes of `core`: every kind of block, a relative link and a link to another scheme. */
export const CORE_NOTES = `## Notes

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
export function planFiles({ features = FEATURES, notes = { core: CORE_NOTES } as Record<string, string>, decisions = DECISIONS as unknown[] } = {}): Map<string, string> {
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

/** The files of a plan that holds no part, no stage and no feature, with the fixture's decisions. */
export function emptyPlanFiles(): Map<string, string> {
  return new Map([
    ['PRODUCT.md', 'Silo.\n'],
    ['DESIGN.md', 'The design.\n'],
    ['areas.json', canonicalText([])],
    ['order.json', canonicalText({ stages: [] })],
    ['decisions.json', canonicalText(DECISIONS.map(({ about: _about, ...decision }) => decision))],
  ]);
}

const NOTES = new WeakMap<BuildData, Notes>();

export function buildData(options?: Parameters<typeof planFiles>[0]): BuildData {
  const files = planFiles(options);
  const data = buildDataFrom(files, EXAMPLE_MAP);
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

/** Live state: `core` is taken on GitHub, `create` has votes, comments and history lines. */
export function liveState(extra: Record<string, LiveIssue> = {}, latest: LiveLatest[] = []): LiveState {
  return {
    repo: 'amontlabs/silo',
    fetchedAt: new Date(Date.now()).toISOString(),
    issues: {
      10: issue({ entry: 'core', title: 'The engine runs on its own', verdict: 'take', up: 8 }),
      11: issue({ entry: 'calls', title: 'Every call defined once', up: 3 }),
      12: issue({ entry: 'create', title: 'Create and delete a computer', up: 12, comments: 5, updates: 3 }),
      13: issue({ entry: 'exec', title: 'Run a command', up: 4 }),
      14: issue({ entry: 'fork', title: 'Fork a computer', up: 21 }),
      15: issue({ entry: 'shells', title: 'Your shell inside', verdict: 'take' }),
      16: issue({ entry: 'linux', title: 'A Linux server' }),
      17: issue({ entry: 'reviewer', title: 'A hosted reviewer' }),
      30: issue({ kind: 'suggestion', title: 'Start a task from Slack', up: 9, author: 'devin-oss', bot: false }),
      31: issue({ kind: 'suggestion', title: 'Share a base computer', up: 14, author: 'mara-k', bot: false }),
      ...extra,
    },
    latest,
  };
}
