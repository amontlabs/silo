import { describe, expect, it } from 'vitest';
import { buildDataFrom } from './data';
import { buildData, DECISIONS, FEATURES, issue, liveState, planFiles } from './fixtures';
import { allRows, buildPlan, changeLine, changePhrase, countDecisions, entryStatuses, isDecided, LATEST_LINES, relativeTime, shortDate, statusLine, stateWords } from './plan';
import type { Change, ChangeName, Entry, PartGroup } from './plan';

const names = (rows: { name: string }[]) => rows.map((row) => row.name);
const byPart = (plan: ReturnType<typeof buildPlan>) => new Map(plan.parts.map((group) => [group.part.id, group]));
/** The fixture plan with these features' files changed. */
const withFeatures = (change: Record<string, Partial<(typeof FEATURES)[number]>>) => buildData({ features: FEATURES.map((feature) => ({ ...feature, ...change[feature.id] })) });
const withBuilt = (...ids: string[]) => withFeatures(Object.fromEntries(ids.map((id) => [id, { verdict: 'take' as const, built: '2026-11-16' }])));

describe('the plan by part', () => {
  it('gives each part its decisions and features in the order of order.json', () => {
    const plan = buildPlan(buildData(), null);
    expect(byPart(plan).get('part-a')!.rows.map((row) => [row.key, row.of])).toEqual([['entry-core-in-zig', 'decision'], ['entry-core', 'feature'], ['entry-calls', 'feature']]);
    expect(byPart(plan).get('part-d')!.rows.map((row) => row.key)).toEqual(['entry-intake', 'entry-cli', 'entry-one-way', 'entry-tamper-proof-logs', 'entry-fork', 'entry-checkpoint', 'entry-diff', 'entry-install-agent', 'entry-secrets']);
  });

  it('draws a decision as a row: its asks the name, its answer the body', () => {
    const zig = byPart(buildPlan(buildData(), null)).get('part-a')!.rows[0];
    expect(zig).toMatchObject({ kind: 'entry', of: 'decision', name: 'Which language is the core written in?', what: 'Zig. It is the core piece.', status: 'planned', notes: true, part: 'part-a' });
  });

  it('puts the decisions of the whole product in a place of their own', () => {
    const plan = buildPlan(buildData(), null);
    expect(plan.product.map((row) => [row.key, row.status])).toEqual([['entry-security-properly', 'planned'], ['entry-one-host', 'undecided'], ['entry-plain-logs', 'replaced']]);
    expect(allRows(plan).filter((row) => row.part === null && row.kind === 'entry')).toEqual(plan.product);
  });

  it('lists every dropped entry of the parts as left out, and keeps it in its own part', () => {
    const plan = buildPlan(withFeatures({ exec: { verdict: 'drop' } }), null);
    expect(plan.left.map((row) => row.key)).toEqual(['entry-exec', 'entry-old-format', 'entry-reviewer', 'entry-old-desktop']);
    expect(plan.left.every((row) => row.status === 'dropped')).toBe(true);
    expect(names(byPart(plan).get('part-b')!.rows)).toContain('Run a command');
  });

  it('takes the status from the file alone: a live issue, open or closed, changes nothing', () => {
    const live = liveState({
      10: issue({ entry: 'core', state: 'closed', reason: 'not_planned' }),
      14: issue({ entry: 'fork', state: 'closed', reason: 'completed', closedAt: '2026-11-16T00:00:00Z' }),
    });
    const statuses = (plan: ReturnType<typeof buildPlan>) => allRows(plan).filter((row) => row.kind === 'entry').map((row) => `${row.key}:${row.status}`);
    expect(statuses(buildPlan(buildData(), live))).toEqual(statuses(buildPlan(buildData(), null)));
    expect(byPart(buildPlan(buildData(), live)).get('part-a')!.rows.map((row) => row.status)).toEqual(['planned', 'planned', 'planned']);
    expect(byPart(buildPlan(withBuilt('fork'), null)).get('part-d')!.rows.find((row) => row.key === 'entry-fork')).toMatchObject({ status: 'built', built: '2026-11-16' });
  });

  it('says a status in the page\'s words, through the plan\'s model', () => {
    const entry = (fields: Partial<Entry>): Entry => ({ id: 'x', kind: 'feature', part: 'p', stage: 's', position: 1, name: 'X', what: 'X.', needs: [], verdict: null, replaces: [], replacedBy: null, built: null, notes: false, more: false, history: null, ...fields });
    const status = (fields: Partial<Entry>, others: Entry[] = []) => entryStatuses([entry(fields), ...others]).get('x');
    expect(status({})).toBe('undecided');
    expect(status({ verdict: 'take' })).toBe('planned');
    // Taken but waiting for a need that is not done: still planned.
    expect(status({ verdict: 'take', needs: ['y'] }, [entry({ id: 'y' })])).toBe('planned');
    expect(status({ verdict: 'take', built: '2026-11-16' })).toBe('built');
    expect(status({ verdict: 'drop' })).toBe('dropped');
    expect(status({ kind: 'decision', what: null })).toBe('undecided');
    expect(status({ kind: 'decision', what: 'Yes.' })).toBe('undecided');
    expect(status({ kind: 'decision', what: 'Yes.', verdict: 'take' })).toBe('planned');
    expect(status({ kind: 'decision', verdict: 'drop' })).toBe('dropped');
  });

  it('counts a part from its features only, and its decisions apart', () => {
    const plan = buildPlan(buildData(), null);
    const partA = byPart(plan).get('part-a')!;
    expect(partA.counts).toEqual({ built: 0, planned: 2, undecided: 0, dropped: 0 });
    expect(partA.decisions).toEqual({ decided: 1, undecided: 0, replaced: 0, dropped: 0 });
    const partE = byPart(plan).get('part-e')!;
    expect(partE.counts).toEqual({ built: 0, planned: 0, undecided: 7, dropped: 1 });
    expect(partE.decisions).toEqual({ decided: 1, undecided: 1, replaced: 0, dropped: 1 });
    // A part that holds only decisions is open, with no feature counted.
    const onlyDecisions = buildPlan(buildData({ features: FEATURES.filter((feature) => feature.part !== 'part-a') }), null);
    expect(byPart(onlyDecisions).get('part-a')).toMatchObject({ counts: { built: 0, planned: 0, undecided: 0, dropped: 0 }, state: 'open', rows: [{ key: 'entry-core-in-zig' }] });
  });

  it('puts a part holding an entry of the first five stages in the first band', () => {
    const plan = buildPlan(buildData(), null);
    expect(plan.parts.map((group) => [group.part.id, group.number, group.first])).toEqual([['part-a', 1, true], ['part-b', 2, true], ['part-c', 3, true], ['part-d', 4, true], ['part-e', 5, false]]);
  });

  it('ignores the issue of an entry the plan no longer holds, closed or open', () => {
    const keys = (plan: ReturnType<typeof buildPlan>) => allRows(plan).map((row) => `${row.key}:${row.status}`);
    const gone = buildPlan(buildData(), liveState({
      40: issue({ entry: 'gone-feature', title: 'Gone', state: 'closed', reason: 'not_planned', closedAt: '2026-11-18T00:00:00Z' }),
      41: issue({ entry: 'other-gone', title: 'Other' }),
    }));
    expect(keys(gone)).toEqual(keys(buildPlan(buildData(), liveState())));
    expect(names(allRows(gone))).not.toContain('Gone');
  });

  it('matches no issue to an entry while entries have no threads; suggestions still come', () => {
    const data = buildDataFrom(planFiles());
    const plan = buildPlan(data, liveState({}, [{ n: 12, entry: 'create', author: 'mara-k', bot: false, createdAt: '2026-11-16T10:00:00Z', text: 'Hello' }]));
    expect(plan.threads).toBe(false);
    expect(allRows(plan).filter((row) => row.kind === 'entry').every((row) => row.issue === null && row.up === null && row.comments === null)).toBe(true);
    expect(plan.latest).toEqual([]);
    expect(names(plan.suggestions)).toEqual(['Share a base computer', 'Start a task from Slack']);
  });

  it('sorts suggestions by votes with dropped ones last', () => {
    const plan = buildPlan(buildData(), liveState({
      32: issue({ kind: 'suggestion', title: 'Dropped idea', up: 50, state: 'closed', reason: 'not_planned', author: 'x' }),
    }));
    expect(names(plan.suggestions)).toEqual(['Share a base computer', 'Start a task from Slack', 'Dropped idea']);
  });

  it('takes a suggestion\'s status from its issue alone', () => {
    const plan = buildPlan(buildData(), liveState({
      32: issue({ kind: 'suggestion', title: 'Dropped idea', state: 'closed', reason: 'not_planned', author: 'x' }),
      33: issue({ kind: 'suggestion', title: 'Built idea', state: 'closed', reason: 'completed', author: 'x' }),
      34: issue({ kind: 'suggestion', title: 'Closed idea', state: 'closed', reason: null, author: 'x' }),
    }));
    const status = (title: string) => plan.suggestions.find((row) => row.name === title)?.status;
    expect([status('Start a task from Slack'), status('Dropped idea'), status('Built idea'), status('Closed idea')]).toEqual(['undecided', 'dropped', 'built', 'undecided']);
  });

  it('counts the comments of an issue as they are: a comment of the App is a comment', () => {
    const plan = buildPlan(buildData(), liveState());
    const create = byPart(plan).get('part-b')!.rows[0];
    expect(create).toMatchObject({ comments: 2, up: 12, issue: 12 });
    expect(create).not.toHaveProperty('updates');
  });

  it('puts the latest comments on their entries, newest first as given, and skips one of another issue', () => {
    const at = '2026-11-16T10:00:00Z';
    const plan = buildPlan(buildData(), liveState({}, [
      { n: 12, entry: 'create', author: 'mara-k', bot: false, createdAt: at, text: 'Hello' },
      { n: 99, entry: 'exec', author: 'x', bot: false, createdAt: at, text: 'Wrong issue' },
      { n: 10, entry: 'core', author: 'silo-workshop[bot]', bot: true, createdAt: at, text: 'From the App' },
      { n: 5, entry: 'nothing', author: 'x', bot: false, createdAt: at, text: 'Gone' },
    ]));
    expect(plan.latest.map((item) => [item.row.key, item.text, item.change])).toEqual([['entry-create', 'Hello', null], ['entry-core', 'From the App', null]]);
  });

  it('holds the decisions of the fixture plan as entries, with nothing of the old list of decisions', () => {
    const data = buildData();
    expect(data.entries.filter((entry) => entry.kind === 'decision').map((entry) => entry.id).sort()).toEqual(DECISIONS.map((decision) => decision.id).sort());
    expect(Object.keys(buildPlan(data, null))).not.toContain('decided');
  });
});

describe('Latest: comments and changes merged by time', () => {
  const change = (commit: string, date: string, what: ChangeName[] = ['notes']): Change => ({ commit, date, what });
  const comment = (n: number, entry: string, createdAt: string, text: string) => ({ n, entry, author: 'mara-k', bot: false, createdAt, text });

  it('lists comments and changes together, newest first, each on its entry', () => {
    const histories = {
      core: [change('c3', '2026-11-18T10:00:00Z', ['taken']), change('c1', '2026-11-12T10:00:00Z', ['added'])],
      calls: [change('c2', '2026-11-15T10:00:00Z')],
    };
    const plan = buildPlan(buildData(), liveState({}, [comment(12, 'create', '2026-11-17T10:00:00Z', 'Hello'), comment(13, 'exec', '2026-11-13T10:00:00Z', 'Older')]), histories);
    expect(plan.latest.map((item) => [item.row.key, item.text, item.change?.commit ?? null])).toEqual([
      ['entry-core', null, 'c3'], ['entry-create', 'Hello', null], ['entry-calls', null, 'c2'], ['entry-exec', 'Older', null], ['entry-core', null, 'c1'],
    ]);
    expect(new Set(plan.latest.map((item) => item.key)).size).toBe(5);
  });

  it('puts a comment before a change made at the same time', () => {
    const at = '2026-11-17T10:00:00Z';
    const plan = buildPlan(buildData(), liveState({}, [comment(12, 'create', at, 'Hello')]), { core: [change('c1', at)] });
    expect(plan.latest.map((item) => item.text ?? item.change?.commit)).toEqual(['Hello', 'c1']);
  });

  it('holds 30 lines in all, the oldest dropped, and skips an entry the plan lacks', () => {
    const changes = Array.from({ length: 25 }, (_, i) => change(`c${i}`, new Date(Date.parse('2026-11-20T23:00:00Z') - i * 3_600_000).toISOString()));
    const comments = Array.from({ length: 12 }, (_, i) => comment(12, 'create', `2026-11-0${1 + (i % 9)}T10:00:0${i % 10}Z`, `Comment ${i}`));
    const plan = buildPlan(buildData(), liveState({}, comments), { core: changes, nothing: [change('gone', '2026-11-20T23:59:00Z')] });
    expect(LATEST_LINES).toBe(30);
    expect(plan.latest).toHaveLength(30);
    expect(plan.latest.some((item) => item.change?.commit === 'gone')).toBe(false);
    const times = plan.latest.map((item) => Date.parse(item.createdAt));
    expect(times).toEqual([...times].sort((a, b) => b - a));
    expect(plan.latest.filter((item) => item.change)).toHaveLength(25);
    expect(plan.latest.filter((item) => item.text)).toHaveLength(5);
  });

  it('has no Latest without threads, whatever the history', () => {
    const data = { ...buildData(), threads: false };
    expect(buildPlan(data, liveState({}, [comment(12, 'create', '2026-11-17T10:00:00Z', 'Hello')]), { core: [change('c1', '2026-11-17T10:00:00Z')] }).latest).toEqual([]);
  });
});

describe('the phrases of a change', () => {
  const names: ChangeName[] = ['added', 'moved', 'text', 'sets', 'needs', 'keeps', 'surfaces', 'suggest', 'notes', 'taken', 'dropped', 'cleared', 'replaced', 'built'];

  it('say each name in the page\'s words, for a decision and for a feature', () => {
    const words = (of: 'decision' | 'feature') => names.map((name) => changePhrase(name, of));
    expect(words('decision')).toEqual([
      'Added', 'Moved to another part', 'Text changed', 'What a yes would do changed', 'Needs changed', 'What it must keep changed', 'Where it shows changed',
      'Recommendation changed', 'Notes edited', 'Decided', 'Dropped', 'Back to not decided', 'Replaced by', 'Built',
    ]);
    expect(words('feature')).toEqual([...words('decision').slice(0, 9), 'Planned', ...words('decision').slice(10)]);
  });

  it('never let the name of a key through', () => {
    const line = changeLine(names, 'decision');
    expect(line).not.toMatch(/\b(taken|sets|keeps|surfaces|suggest|cleared|replaced)\b|[a-z]+_[a-z]+/);
    expect(line.split(' · ')).toHaveLength(names.length);
  });

  it('join the phrases of one change by a middle dot, in the order given', () => {
    expect(changeLine(['text', 'notes', 'taken'], 'feature')).toBe('Text changed · Notes edited · Planned');
    expect(changeLine(['added'], 'decision')).toBe('Added');
  });

  it('write a date as day and month', () => {
    expect(shortDate('2026-11-20T09:30:00Z')).toBe('20 Nov');
  });
});

describe('time', () => {
  it('says how long ago', () => {
    const now = Date.parse('2026-11-17T12:00:00Z');
    expect(relativeTime('2026-11-17T11:59:00Z', now)).toBe('just now');
    expect(relativeTime('2026-11-17T10:00:00Z', now)).toBe('2 hours ago');
    expect(relativeTime('2026-11-16T10:00:00Z', now)).toBe('yesterday');
    expect(relativeTime('2026-11-10T10:00:00Z', now)).toBe('7 days ago');
  });
});

describe('part states', () => {
  it('says the few words on a part', () => {
    const words = (state: PartGroup['state'], counts: Partial<PartGroup['counts']>) => stateWords({ state, counts: { built: 0, planned: 0, undecided: 0, dropped: 0, ...counts } });
    expect(words('done', { built: 4 })).toBe('done');
    expect(words('next', { planned: 3 })).toBe('next');
    expect(words('planned', { planned: 4 })).toBe('4 planned');
    expect(words('planned', { built: 1, planned: 4 })).toBe('1/5 built');
    expect(words('open', { undecided: 4 })).toBe('');
  });
});

describe('the line under the title', () => {
  const line = (data = buildData()) => statusLine(buildPlan(data, null).parts);
  const undecided = (...ids: string[]) => Object.fromEntries(ids.map((id) => [id, { verdict: undefined }]));

  it('starts with the part holding the first planned feature, before anything is built', () => {
    expect(line()).toBe('Starting with Part A.');
    expect(line(withFeatures(undecided('core', 'calls')))).toBe('Starting with Part C.');
  });

  it('says what is next once something is built', () => {
    expect(line(withBuilt('core', 'calls'))).toBe('Part C is next.');
  });

  it('says nothing when nothing is planned', () => {
    expect(line(withBuilt('core', 'calls', 'shells'))).toBe('');
    expect(line(withFeatures(undecided('core', 'calls', 'shells')))).toBe('');
  });

  it('is never moved by a decision: a taken decision is not a planned feature', () => {
    expect(line(buildData({ features: FEATURES.filter((feature) => feature.part !== 'part-a') }))).toBe('Starting with Part C.');
  });
});

describe('a replaced decision', () => {
  const decision = (fields: Partial<Entry>): Entry => ({ id: 'x', kind: 'decision', part: 'p', stage: 's', position: 1, name: 'X?', what: 'Yes.', needs: [], verdict: 'take', replaces: [], replacedBy: null, built: null, notes: false, more: false, history: null, ...fields });
  const status = (fields: Partial<Entry>) => entryStatuses([decision(fields)]).get('x');

  it('has the status replaced once its file names the decision that replaced it, and is decided before', () => {
    expect(status({})).toBe('planned');
    expect(status({ replacedBy: 'y' })).toBe('replaced');
    // Not planned, not dropped, and no verdict makes it neither.
    expect(status({ replacedBy: 'y', verdict: 'drop' })).toBe('dropped');
    expect(status({ replaces: ['y'] })).toBe('planned');
    expect(status({ replaces: ['y'], verdict: null })).toBe('undecided');
  });

  it('is read from the file by the build: replaces and replaced_by', () => {
    const entries = buildData().entries;
    expect(entries.find((entry) => entry.id === 'plain-logs')).toMatchObject({ replaces: [], replacedBy: 'signed-logs' });
    expect(entries.find((entry) => entry.id === 'signed-logs')).toMatchObject({ replaces: ['plain-logs'], replacedBy: null });
    expect(entries.find((entry) => entry.id === 'tamper-proof-logs')).toMatchObject({ replaces: ['signed-logs'], verdict: null });
    expect(entries.find((entry) => entry.id === 'core-in-zig')).toMatchObject({ replaces: [], replacedBy: null });
  });

  it('is a row with the status replaced, which names the decision that replaced it', () => {
    const plan = buildPlan(buildData(), null);
    expect(plan.product.find((row) => row.key === 'entry-plain-logs')).toMatchObject({ status: 'replaced', replacedBy: 'entry-signed-logs', replaces: [] });
    expect(byPart(plan).get('part-e')!.rows.find((row) => row.key === 'entry-signed-logs')).toMatchObject({ status: 'planned', replacedBy: null, replaces: ['entry-plain-logs'] });
    expect(byPart(plan).get('part-d')!.rows.find((row) => row.key === 'entry-tamper-proof-logs')).toMatchObject({ status: 'undecided', replaces: ['entry-signed-logs'] });
  });

  it('is counted as replaced, never as decided', () => {
    const plan = buildPlan(buildData(), null);
    expect(countDecisions(plan.product)).toEqual({ decided: 1, undecided: 1, replaced: 1, dropped: 0 });
    expect(countDecisions(byPart(plan).get('part-e')!.rows.filter((row) => row.of === 'decision'))).toEqual({ decided: 1, undecided: 1, replaced: 0, dropped: 1 });
    expect(isDecided(plan.product[2])).toBe(false);
    expect(isDecided(plan.product[0])).toBe(true);
  });

  it('stays at its place, and is not in the list of dropped entries', () => {
    const plan = buildPlan(buildData(), null);
    expect(plan.product.map((row) => row.key)).toEqual(['entry-security-properly', 'entry-one-host', 'entry-plain-logs']);
    expect(plan.left.map((row) => row.key)).not.toContain('entry-plain-logs');
    expect(allRows(plan).filter((row) => row.key === 'entry-plain-logs')).toHaveLength(1);
  });

  it('is named by no dropped decision: a dropped one replaces nothing', () => {
    const decisions = DECISIONS.map((item) => (item.id === 'tamper-proof-logs' ? { ...item, verdict: 'drop' as const } : item));
    const row = buildPlan(buildData({ decisions }), null).parts.flatMap((group) => group.rows).find((item) => item.key === 'entry-tamper-proof-logs');
    expect(row).toMatchObject({ status: 'dropped', replaces: [] });
  });
});
