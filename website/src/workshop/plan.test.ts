import { describe, expect, it } from 'vitest';
import { buildData, issue, liveState } from './fixtures';
import { awaitsVerdict, buildPlan, decisionTitle, parseDecisions, parseFeatures, stagePhrase } from './plan';

const names = (rows: { name: string }[]) => rows.map((row) => row.name);

describe('parsing the repository files', () => {
  it('reads decided lines newest first, with their issue and without Markdown links', () => {
    const lines = buildData().decided;
    expect(lines.map((line) => [line.date, line.issue])).toEqual([
      ['2026-11-16', 40],
      ['2026-11-16', null],
      ['2026-11-15', null],
    ]);
    expect(lines[0].text).toBe('The engine is written in Zig. It is the core piece');
    expect(lines[1].who).toBe("orchestrator, at the owner's word");
    expect(lines[1].text).toBe('Merging changes main only. See the list.');
  });

  it('keeps only plain recording file names', () => {
    const computers = buildData().features.find((feature) => feature.id === 'computers');
    expect(computers?.recordings).toEqual(['create.mp4']);
  });

  it('ignores unknown verdicts and skips lines without an id', () => {
    const features = parseFeatures('{"n":1,"id":"a","stage":"s","name":"A","verdict":"maybe"}\n{"n":2,"stage":"s","name":"B"}\n');
    expect(features).toHaveLength(1);
    expect(features[0].verdict).toBeNull();
  });

  it('shortens stage lines and decided lines to a phrase', () => {
    expect(stagePhrase('One computer you can open from the app and the command line.')).toBe('one computer you can open from the app and the command line');
    expect(stagePhrase('Updates, releases, logs and disk: what someone else needs.')).toBe('updates, releases, logs and disk');
    expect(stagePhrase('A graphical desktop in a computer.')).toBe('a graphical desktop in a computer');
    expect(stagePhrase('macOS guests.')).toBe('macOS guests');
    expect(decisionTitle('GitHub is the storage: issues. Agents post as a bot.')).toBe('GitHub is the storage');
    expect(parseDecisions('- 2026-11-16 · owner · Text (#7)\n- not a line')).toHaveLength(1);
  });
});

describe('the plan', () => {
  it('builds the same plan from the files alone, with every file verdict carried over', () => {
    const plan = buildPlan(buildData(), null);
    expect(plan.stages.map((group) => group.stage.id)).toEqual(['shell', 'loop', 'living', 'devices']);
    expect(plan.stages[0].rows.map((row) => [row.name, row.status, row.carried])).toEqual([
      ['The engine runs on its own', 'planned', true],
      ['Every call defined once', 'planned', true],
      ['Create and delete a computer', 'undecided', false],
      ['Run a command', 'undecided', false],
    ]);
    expect(plan.stages[0].rows.every((row) => row.up === null && row.comments === null)).toBe(true);
    expect(plan.suggestions).toEqual([]);
    expect(plan.openDecisions).toEqual([]);
    expect([plan.planned, plan.undecided]).toEqual([3, 4]);
  });

  it('prefers the live verdict to the file verdict', () => {
    const plan = buildPlan(buildData(), liveState({
      10: issue({ entry: 'engine', verdict: 'later' }),
      13: issue({ entry: 'exec', verdict: 'take' }),
      14: issue({ entry: 'fork', state: 'closed', reason: 'completed', closedAt: '2026-11-16T00:00:00Z' }),
    }));
    const byName = new Map(plan.stages.flatMap((group) => group.rows).map((row) => [row.name, row]));
    expect(byName.get('The engine runs on its own')).toMatchObject({ status: 'later', carried: false });
    expect(byName.get('Every call defined once')).toMatchObject({ status: 'planned', carried: true, issue: 11 });
    expect(byName.get('Run a command')).toMatchObject({ status: 'planned', carried: false });
    expect(byName.get('Fork a computer')).toMatchObject({ status: 'built' });
    expect(byName.get('Create and delete a computer')).toMatchObject({ status: 'undecided', up: 12, comments: 2 });
    // engine later; calls, exec, fork (built), shells planned.
    expect([plan.planned, plan.undecided]).toEqual([4, 2]);
  });

  it('folds dropped entries out of their stages, whether dropped in the file or live', () => {
    const plan = buildPlan(buildData(), liveState({
      13: issue({ entry: 'exec', verdict: 'drop', state: 'closed', reason: 'not_planned' }),
      16: issue({ entry: 'devices', state: 'closed', reason: 'not_planned' }),
    }));
    expect(names(plan.dropped)).toEqual(['Run a command', 'A Linux server', 'A hosted reviewer']);
    expect(plan.dropped.map((row) => row.carried)).toEqual([false, false, true]);
    expect(plan.stages.map((group) => group.stage.id)).toEqual(['shell', 'loop', 'living']);
    expect(plan.stages[0].rows).toHaveLength(3);
  });

  it('opens every stage up to the first one with an undecided entry', () => {
    expect(buildPlan(buildData(), liveState()).lastOpenStage).toBe(0);
    const decidedShell = buildPlan(buildData(), liveState({
      12: issue({ entry: 'computers', verdict: 'take' }),
      13: issue({ entry: 'exec', verdict: 'later' }),
    }));
    expect(decidedShell.lastOpenStage).toBe(1);
    expect(decidedShell.stages[0].decided).toBe(4);
  });

  it('counts decided entries per stage', () => {
    const plan = buildPlan(buildData(), liveState({ 12: issue({ entry: 'computers', verdict: 'refine' }) }));
    expect(plan.stages.map((group) => `${group.decided} of ${group.rows.length}`)).toEqual(['2 of 4', '0 of 1', '1 of 1', '0 of 1']);
  });

  it('lists suggestions by votes and decisions newest first', () => {
    const plan = buildPlan(buildData(), liveState({
      43: issue({ kind: 'decision', title: 'Decided on GitHub', state: 'closed', reason: 'completed', closedAt: '2026-11-17T00:00:00Z' }),
    }));
    expect(names(plan.suggestions)).toEqual(['Share a base computer', 'Start a task from Slack']);
    expect(plan.suggestions[0].author).toBe('mara-k');
    expect(names(plan.openDecisions)).toEqual(['After a task ends, what happens to its computer?', 'Which Zig version to pin']);
    expect(names(plan.decided)).toEqual(['Decided on GitHub', 'The engine is written in Zig', 'Merging changes main only', 'Security is built properly']);
    expect(plan.decided[1]).toMatchObject({ key: 'issue-40', comments: 2 });
    expect(plan.decided[2]).toMatchObject({ key: 'decided-2', comments: null });
  });

  it('waits for a verdict only on undecided rows with an issue', () => {
    const rows = buildPlan(buildData(), liveState()).stages[0].rows;
    expect(rows.map(awaitsVerdict)).toEqual([false, false, true, true]);
    const offline = buildPlan(buildData(), null).stages[0].rows;
    expect(offline.map(awaitsVerdict)).toEqual([false, false, false, false]);
  });
});
