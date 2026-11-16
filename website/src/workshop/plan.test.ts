import { describe, expect, it } from 'vitest';
import { buildData, issue, liveState } from './fixtures';
import { areaStates, awaitsVerdict, buildPlan, countRows, decisionTitle, issueStatus, statusLine, stateWords } from './plan';
import type { AreaGroup, Row } from './plan';

const names = (rows: { name: string }[]) => rows.map((row) => row.name);
const byArea = (plan: ReturnType<typeof buildPlan>) => new Map(plan.areas.map((group) => [group.area.id, group]));
const built = (entry: string) => issue({ entry, verdict: 'take', state: 'closed', reason: 'completed', closedAt: '2026-11-16T00:00:00Z' });
const building = (entry: string) => issue({ entry, verdict: 'take', building: true });
const taken = (entry: string) => issue({ entry, verdict: 'take' });

function row(status: Row['status'], n: number): Row {
  return { status, n } as Row;
}

describe('the plan by area', () => {
  it('groups entries by area in the order of the areas file, in build order', () => {
    const plan = buildPlan(buildData(), null);
    expect(plan.areas.map((group) => group.area.id)).toEqual(['engine', 'computers', 'app', 'checkpoints', 'forks', 'changes', 'agents', 'access', 'setup', 'tasks', 'devices', 'releases', 'trust', 'desktop', 'macos', 'sharing']);
    expect(plan.areas.map((group) => group.number)).toEqual([...Array(16).keys()].map((i) => i + 1));
    expect(names(plan.areas[0].rows)).toEqual(['The engine runs on its own', 'Every call defined once']);
    expect(plan.areas[1].rows.map((r) => r.n)).toEqual([3, 4]);
    expect(plan.areas[0].rows.every((r) => r.up === null && r.comments === null)).toBe(true);
  });

  it('puts areas with an entry up to the starter in the first band', () => {
    const plan = buildPlan(buildData(), null);
    expect(plan.areas.filter((group) => group.first).map((group) => group.area.id)).toEqual(['engine', 'computers', 'app', 'checkpoints', 'forks', 'changes', 'agents', 'access', 'setup']);
  });

  it('lists what is not carried over: entries without an area, then every dropped entry', () => {
    const plan = buildPlan(buildData(), liveState({ 13: issue({ entry: 'exec', verdict: 'drop', state: 'closed', reason: 'not_planned' }) }));
    expect(names(plan.left)).toEqual(['A hosted reviewer', 'An earlier desktop stack', 'Run a command']);
    expect(plan.left.map((r) => r.status)).toEqual(['dropped', 'undecided', 'dropped']);
    // A dropped entry still shows in its own area.
    expect(names(byArea(plan).get('computers')!.rows)).toContain('Run a command');
  });

  it('prefers the live verdict to the file verdict, and counts each area', () => {
    const plan = buildPlan(buildData(), liveState({ 10: issue({ entry: 'engine', verdict: 'later' }), 14: built('fork') }));
    const engine = byArea(plan).get('engine')!;
    expect(engine.rows.map((r) => [r.status, r.carried])).toEqual([['later', false], ['planned', true]]);
    expect(engine.counts).toEqual({ built: 0, building: 0, planned: 1, later: 1, undecided: 0, dropped: 0 });
    expect(byArea(plan).get('forks')!.counts.built).toBe(1);
  });

  it('marks a planned entry whose issue says building, and only a planned one', () => {
    expect(issueStatus(issue({ verdict: 'take', building: true }), null).status).toBe('building');
    expect(issueStatus(issue({ building: true }), 'take')).toEqual({ status: 'building', carried: true });
    expect(issueStatus(issue({ verdict: 'later', building: true }), null).status).toBe('later');
    expect(issueStatus(issue({ building: true }), null).status).toBe('undecided');
    expect(issueStatus(issue({ verdict: 'take', building: true, state: 'closed', reason: 'completed' }), null).status).toBe('built');
  });

  it('sorts suggestions by votes with dropped ones last, and decisions newest first', () => {
    const plan = buildPlan(buildData(), liveState({
      32: issue({ kind: 'suggestion', title: 'Dropped idea', up: 50, verdict: 'drop', state: 'closed', reason: 'not_planned', author: 'x' }),
      43: issue({ kind: 'decision', title: 'Decided on GitHub', state: 'closed', reason: 'completed', closedAt: '2026-11-17T00:00:00Z' }),
    }));
    expect(names(plan.suggestions)).toEqual(['Share a base computer', 'Start a task from Slack', 'Dropped idea']);
    expect(names(plan.openDecisions)).toEqual(['After a task ends, what happens to its computer?', 'Which Zig version to pin']);
    expect(names(plan.decided)).toEqual(['Decided on GitHub', 'The engine is written in Zig', 'Merging changes main only', 'Security is built properly']);
    expect(plan.decided[1]).toMatchObject({ key: 'issue-40', comments: 2 });
    expect(decisionTitle('GitHub is the storage: issues. Agents post as a bot.')).toBe('GitHub is the storage');
  });

  it('waits for a verdict only on undecided rows with an issue', () => {
    const rows = buildPlan(buildData(), liveState()).areas[1].rows;
    expect(rows.map(awaitsVerdict)).toEqual([true, true]);
    const engine = buildPlan(buildData(), liveState()).areas[0].rows;
    expect(engine.map(awaitsVerdict)).toEqual([false, false]);
    expect(buildPlan(buildData(), null).areas[1].rows.map(awaitsVerdict)).toEqual([false, false]);
  });
});

describe('area states', () => {
  const states = (...areas: Row[][]) => areaStates(areas.map((rows) => ({ rows, counts: countRows(rows) })));

  it('is now when an entry is building, done when built with nothing planned', () => {
    expect(states([row('building', 3), row('built', 1)])).toEqual(['now']);
    expect(states([row('built', 1), row('later', 9)])).toEqual(['done']);
    expect(states([row('built', 1), row('planned', 9)], [row('planned', 2)])).toEqual(['planned', 'next']);
  });

  it('gives next to the area with the lowest planned entry, skipping one being built', () => {
    expect(states([row('building', 1), row('planned', 2)], [row('planned', 5)], [row('planned', 4)])).toEqual(['now', 'planned', 'next']);
    expect(states([row('planned', 7)], [row('undecided', 1)], [row('planned', 3), row('planned', 9)])).toEqual(['planned', 'open', 'next']);
  });

  it('is open when nothing is decided or only later and dropped, and has no next without a planned entry', () => {
    expect(states([row('undecided', 1)], [row('later', 2), row('dropped', 3)], [row('refine', 4)])).toEqual(['open', 'open', 'open']);
  });

  it('says the few words on a part', () => {
    const words = (state: AreaGroup['state'], counts: Partial<AreaGroup['counts']>) => stateWords({ state, counts: { built: 0, building: 0, planned: 0, later: 0, undecided: 0, dropped: 0, ...counts } });
    expect(words('done', { built: 4 })).toBe('done');
    expect(words('now', { built: 2, building: 2, planned: 3 })).toBe('now · 2/7');
    expect(words('next', { planned: 3 })).toBe('next');
    expect(words('planned', { planned: 4 })).toBe('4 planned');
    expect(words('planned', { built: 1, planned: 4 })).toBe('1/5 built');
    expect(words('open', { undecided: 4 })).toBe('');
  });

  it('reads the fixture plan: engine next, the planned areas, the rest open', () => {
    const plan = buildPlan(buildData(), liveState());
    expect(Object.fromEntries(plan.areas.map((group) => [group.area.id, group.state]))).toMatchObject({ engine: 'next', setup: 'planned', computers: 'open', devices: 'open' });
  });
});

describe('the line under the title', () => {
  const line = (extra: Parameters<typeof liveState>[0]) => statusLine(buildPlan(buildData(), liveState(extra)).areas);

  it('starts with the next area before anything is built', () => {
    expect(line({})).toBe('Starting with the Engine.');
    expect(line({ 10: issue({ entry: 'engine', verdict: 'later' }), 11: issue({ entry: 'calls', verdict: 'later' }) })).toBe('Starting with Your setup.');
  });

  it('names what is being built, and what is next', () => {
    expect(line({ 10: building('engine') })).toBe('Building Engine. Your setup is next.');
    expect(line({ 10: building('engine'), 15: building('shells') })).toBe('Building Engine and Your setup.');
    expect(line({ 10: building('engine'), 15: building('shells'), 14: building('fork') })).toBe('Building Engine, Forks and Your setup.');
  });

  it('says what is next once something is built', () => {
    expect(line({ 10: built('engine'), 11: built('calls') })).toBe('Your setup is next.');
  });

  it('says nothing when nothing is next or building', () => {
    expect(line({ 10: built('engine'), 11: built('calls'), 15: built('shells') })).toBe('');
    expect(line({ 10: issue({ entry: 'engine', verdict: 'later' }), 11: issue({ entry: 'calls', verdict: 'later' }), 15: issue({ entry: 'shells', verdict: 'later' }) })).toBe('');
  });

  it('follows the area order when two are next candidates', () => {
    expect(line({ 13: taken('exec') })).toBe('Starting with the Engine.');
  });
});
