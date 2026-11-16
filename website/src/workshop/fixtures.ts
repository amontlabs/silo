// Small stand-ins for docs/product: tests never read the real files.
import { parseAreas, parseDecisions, parseFeatures, parseStages } from './data';
import type { BuildData, LiveIssue, LiveState } from './plan';

const line = (fields: Record<string, unknown>) => JSON.stringify({ old: 'built', old_lines: 0, reuse: 'reuse', app: null, cli: null, why: null, suggest: 'take', verdict: null, ...fields });

export const featuresText = [
  line({ n: 1, id: 'engine', stage: 'shell', area: 'engine', name: 'The engine runs on its own', what: 'A background engine owns every computer.', verdict: 'take', why: 'The core piece.' }),
  line({ n: 2, id: 'calls', stage: 'shell', area: 'engine', name: 'Every call defined once', what: 'Each call is written down once.', verdict: 'take' }),
  line({ n: 3, id: 'computers', stage: 'shell', area: 'computers', name: 'Create and delete a computer', what: 'Make a Linux computer.', why: 'Key records by id.', recordings: ['create.mp4'] }),
  line({ n: 4, id: 'exec', stage: 'shell', area: 'computers', name: 'Run a command', what: 'Run a command inside.' }),
  line({ n: 5, id: 'fork', stage: 'loop', area: 'forks', name: 'Fork a computer', what: 'Copy a computer.' }),
  line({ n: 6, id: 'shells', stage: 'living', area: 'setup', name: 'Your shell inside', what: 'Your dotfiles.', verdict: 'take' }),
  line({ n: 7, id: 'devices', stage: 'devices', area: 'devices', name: 'A Linux server', what: 'Run on Linux.' }),
  line({ n: 8, id: 'reviewer', stage: 'gone', name: 'A hosted reviewer', what: 'A hosted service.', verdict: 'drop', suggest: 'drop' }),
  line({ n: 9, id: 'cli', stage: 'shell', area: 'app', name: 'One silo command', what: 'One command.' }),
  line({ n: 10, id: 'checkpoint', stage: 'loop', area: 'checkpoints', name: 'Save a checkpoint', what: 'Save and go back.' }),
  line({ n: 11, id: 'diff', stage: 'loop', area: 'changes', name: 'See what a fork changed', what: 'The diff.' }),
  line({ n: 12, id: 'agent', stage: 'agent', area: 'agents', name: 'Install your agent', what: 'Claude Code or Codex.' }),
  line({ n: 13, id: 'secrets', stage: 'living', area: 'access', name: 'Secrets you cannot read', what: 'A placeholder inside.' }),
  line({ n: 14, id: 'triggers', stage: 'tasks', area: 'tasks', name: 'Tasks from GitHub events', what: 'A label starts a task.' }),
  line({ n: 15, id: 'release', stage: 'running', area: 'releases', name: 'Signed releases', what: 'Signed builds.' }),
  line({ n: 16, id: 'audit', stage: 'trust', area: 'trust', name: 'Who did what', what: 'Every call recorded.' }),
  line({ n: 17, id: 'screen', stage: 'desktop', area: 'desktop', name: 'A desktop in a computer', what: 'A shared screen.' }),
  line({ n: 18, id: 'macos', stage: 'macos', area: 'macos', name: 'macOS computers', what: 'macOS inside.' }),
  line({ n: 19, id: 'export', stage: 'extras', area: 'sharing', name: 'Share a computer as a file', what: 'Export and import.' }),
  line({ n: 20, id: 'old-desktop', stage: 'gone', name: 'An earlier desktop stack', what: 'KasmVNC.' }),
].join('\n');

export const areasText = JSON.stringify([
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
], null, 1);

export const stagesText = JSON.stringify([
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
]);

export const decisionsText = `# Decisions

Intro paragraph.

- 2026-11-15 · owner · Security is built properly. It is not the focus yet.
- 2026-11-16 · owner · The engine is written in Zig. It is the core piece (#40)
- 2026-11-16 · orchestrator, at the owner's word · Merging changes main only. See [the list](features.jsonl).
`;

export function buildData(): BuildData {
  const stages = parseStages(stagesText);
  const areas = parseAreas(areasText);
  return { features: parseFeatures(featuresText, stages, areas), areas, stages, decided: parseDecisions(decisionsText) };
}

const day = '2026-11-14T10:00:00Z';

export function issue(fields: Partial<LiveIssue>): LiveIssue {
  return { kind: 'entry', entry: null, title: '', state: 'open', reason: null, verdict: null, up: 0, comments: 0, author: 'silo-workshop[bot]', bot: true, createdAt: day, closedAt: null, ...fields };
}

/** Live state: the engine is taken on GitHub, `calls` is carried over, `computers` has votes and comments. */
export function liveState(extra: Record<string, LiveIssue> = {}): LiveState {
  return {
    repo: 'amontlabs/silo',
    fetchedAt: day,
    issues: {
      10: issue({ entry: 'engine', title: 'The engine runs on its own', verdict: 'take', up: 8 }),
      11: issue({ entry: 'calls', title: 'Every call defined once', up: 3 }),
      12: issue({ entry: 'computers', title: 'Create and delete a computer', up: 12, comments: 2 }),
      13: issue({ entry: 'exec', title: 'Run a command', up: 4 }),
      14: issue({ entry: 'fork', title: 'Fork a computer', up: 21 }),
      15: issue({ entry: 'shells', title: 'Your shell inside', verdict: 'take' }),
      16: issue({ entry: 'devices', title: 'A Linux server' }),
      17: issue({ entry: 'reviewer', title: 'A hosted reviewer' }),
      30: issue({ kind: 'suggestion', title: 'Start a task from Slack', up: 9, author: 'devin-oss', bot: false }),
      31: issue({ kind: 'suggestion', title: 'Share a base computer', up: 14, author: 'mara-k', bot: false }),
      40: issue({ kind: 'decision', title: 'The engine is written in Zig', state: 'closed', reason: 'completed', comments: 2 }),
      41: issue({ kind: 'decision', title: 'Which Zig version to pin', createdAt: '2026-11-15T09:00:00Z' }),
      42: issue({ kind: 'decision', title: 'After a task ends, what happens to its computer?', comments: 5, createdAt: '2026-11-16T09:00:00Z' }),
      ...extra,
    },
  };
}
