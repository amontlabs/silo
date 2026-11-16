// Small stand-ins for docs/product: tests never read the real files.
import { parseDecisions, parseFeatures, parseStages } from './data';
import type { BuildData, LiveIssue, LiveState } from './plan';

const line = (fields: Record<string, unknown>) => JSON.stringify({ old: 'built', old_lines: 0, reuse: 'reuse', app: null, cli: null, why: null, suggest: 'take', verdict: null, ...fields });

export const featuresText = [
  line({ n: 1, id: 'engine', stage: 'shell', name: 'The engine runs on its own', what: 'A background engine owns every computer.', verdict: 'take', why: 'The core piece.' }),
  line({ n: 2, id: 'calls', stage: 'shell', name: 'Every call defined once', what: 'Each call is written down once.', verdict: 'take' }),
  line({ n: 3, id: 'computers', stage: 'shell', name: 'Create and delete a computer', what: 'Make a Linux computer.', why: 'Key records by id.', recordings: ['create.mp4'] }),
  line({ n: 4, id: 'exec', stage: 'shell', name: 'Run a command', what: 'Run a command inside.' }),
  line({ n: 5, id: 'fork', stage: 'loop', name: 'Fork a computer', what: 'Copy a computer.' }),
  line({ n: 6, id: 'shells', stage: 'living', name: 'Your shell inside', what: 'Your dotfiles.', verdict: 'take' }),
  line({ n: 7, id: 'devices', stage: 'devices', name: 'A Linux server', what: 'Run on Linux.' }),
  line({ n: 8, id: 'reviewer', stage: 'gone', name: 'A hosted reviewer', what: 'A hosted service.', verdict: 'drop', suggest: 'drop' }),
].join('\n');

export const stagesText = JSON.stringify([
  { id: 'shell', title: 'Shell', line: 'One computer you can open from the app and the command line.' },
  { id: 'loop', title: 'The loop', line: 'Checkpoint, fork, see what changed, merge or drop.' },
  { id: 'living', title: 'Living in it', line: 'Your setup is inside main. This is the starter.' },
  { id: 'devices', title: 'Other devices', line: 'The same computers, on a Linux server.' },
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
  return { features: parseFeatures(featuresText, stages), stages, decided: parseDecisions(decisionsText) };
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
