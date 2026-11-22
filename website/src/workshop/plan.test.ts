import { describe, expect, it } from 'vitest';
import { buildDataFrom } from './data';
import { buildData, DECISIONS, FEATURES, issue, liveState, planFiles } from './fixtures';
import { allRows, buildPlan, changeLine, changePhrase, countDecisions, entryStatuses, isDecided, relativeTime, shortDate, statusLine, stateWords } from './plan';
import type { ChangeName, Entry, Film, PartGroup } from './plan';

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
    expect(zig).toMatchObject({ kind: 'entry', of: 'decision', name: 'Which language is the core written in?', what: 'Zig. It is the core piece.', status: 'decided', notes: true, part: 'part-a' });
  });

  it('puts the decisions of the whole product in a place of their own', () => {
    const plan = buildPlan(buildData(), null);
    expect(plan.product.map((row) => [row.key, row.status])).toEqual([['entry-security-properly', 'decided'], ['entry-one-host', 'undecided'], ['entry-plain-logs', 'replaced']]);
    expect(allRows(plan).filter((row) => row.part === null && row.kind === 'entry')).toEqual(plan.product);
  });

  it('takes the status from the file alone: a live issue, open or closed, changes nothing', () => {
    const live = liveState({
      10: issue({ entry: 'core', state: 'closed', reason: 'not_planned' }),
      14: issue({ entry: 'fork', state: 'closed', reason: 'completed', closedAt: '2026-11-16T00:00:00Z' }),
    });
    const statuses = (plan: ReturnType<typeof buildPlan>) => allRows(plan).filter((row) => row.kind === 'entry').map((row) => `${row.key}:${row.status}`);
    expect(statuses(buildPlan(buildData(), live))).toEqual(statuses(buildPlan(buildData(), null)));
    expect(byPart(buildPlan(buildData(), live)).get('part-a')!.rows.map((row) => row.status)).toEqual(['decided', 'decided', 'decided']);
    expect(byPart(buildPlan(withBuilt('fork'), null)).get('part-d')!.rows.find((row) => row.key === 'entry-fork')).toMatchObject({ status: 'built', built: '2026-11-16' });
  });

  it('says a status in the page\'s words, through the plan\'s model', () => {
    const entry = (fields: Partial<Entry>): Entry => ({ id: 'x', kind: 'feature', part: 'p', stage: 's', position: 1, name: 'X', what: 'X.', needs: [], verdict: null, by: null, replaces: [], replacedBy: null, started: null, built: null, notes: false, more: false, history: null, ...fields });
    const status = (fields: Partial<Entry>, others: Entry[] = []) => entryStatuses([entry(fields), ...others]).get('x');
    expect(status({})).toBe('undecided');
    expect(status({ verdict: 'take' })).toBe('decided');
    // Taken but waiting for a need that is not done: still decided.
    expect(status({ verdict: 'take', needs: ['y'] }, [entry({ id: 'y' })])).toBe('decided');
    expect(status({ verdict: 'take', started: '2026-11-15' })).toBe('building');
    expect(status({ verdict: 'take', built: '2026-11-16' })).toBe('built');
    expect(status({ verdict: 'take', started: '2026-11-15', built: '2026-11-16' })).toBe('built');
    expect(status({ verdict: 'drop' })).toBe('dropped');
    expect(status({ kind: 'decision', what: null })).toBe('undecided');
    expect(status({ kind: 'decision', what: 'Yes.' })).toBe('undecided');
    expect(status({ kind: 'decision', what: 'Yes.', verdict: 'take' })).toBe('decided');
    expect(status({ kind: 'decision', verdict: 'drop' })).toBe('dropped');
  });

  it('counts a part from its features only, and its decisions apart', () => {
    const plan = buildPlan(buildData(), null);
    const partA = byPart(plan).get('part-a')!;
    expect(partA.counts).toEqual({ built: 0, building: 0, decided: 2, undecided: 0, dropped: 0 });
    expect(partA.decisions).toEqual({ decided: 1, undecided: 0, replaced: 0, dropped: 0 });
    const partE = byPart(plan).get('part-e')!;
    expect(partE.counts).toEqual({ built: 0, building: 0, decided: 0, undecided: 7, dropped: 1 });
    expect(byPart(plan).get('part-c')!.counts).toEqual({ built: 0, building: 1, decided: 0, undecided: 0, dropped: 1 });
    expect(partE.decisions).toEqual({ decided: 1, undecided: 1, replaced: 0, dropped: 1 });
    // A part that holds only decisions is open, with no feature counted.
    const onlyDecisions = buildPlan(buildData({ features: FEATURES.filter((feature) => feature.part !== 'part-a') }), null);
    expect(byPart(onlyDecisions).get('part-a')).toMatchObject({ counts: { built: 0, building: 0, decided: 0, undecided: 0, dropped: 0 }, state: 'open', rows: [{ key: 'entry-core-in-zig' }] });
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

  it('takes an entry\'s thread from the state\'s `threads` alone, never from an issue that merely names the entry', () => {
    const plan = buildPlan(buildData(), liveState({ 50: issue({ entry: 'exec', title: 'Run a command, again', up: 9 }) }, { core: 10, create: 12 }));
    const row = (id: string) => allRows(plan).find((r) => r.key === `entry-${id}`)!;
    expect([row('core').issue, row('core').up]).toEqual([10, 8]);
    expect([row('create').issue, row('create').up, row('create').comments]).toEqual([12, 12, 2]);
    // No thread named: no issue, and a count of 0 once a state is shown; nothing without a state.
    expect([row('exec').issue, row('exec').up, row('exec').comments]).toEqual([null, 0, 0]);
    const none = buildPlan(buildData(), null);
    expect(allRows(none).filter((r) => r.kind === 'entry').every((r) => r.issue === null && r.up === null && r.comments === null)).toBe(true);
  });

  it('shows a feature written from a suggestion once, as the feature, with that issue as its thread', () => {
    const state = liveState({}, { core: 10, fork: 31 });
    const plan = buildPlan(buildData(), state);
    expect(names(plan.suggestions)).toEqual(['Start a task from Slack']);
    const fork = allRows(plan).find((r) => r.key === 'entry-fork')!;
    expect([fork.issue, fork.up]).toEqual([31, 14]);
    expect(allRows(plan).filter((r) => r.issue === 31)).toHaveLength(1);
  });

  it('marks a row closed when its thread is closed on GitHub', () => {
    const plan = buildPlan(buildData(), liveState({ 10: issue({ entry: 'core', title: 'x', state: 'closed', reason: 'completed', closedAt: '2026-11-18T00:00:00Z' }) }));
    expect(allRows(plan).find((r) => r.key === 'entry-core')!.closed).toBe(true);
    expect(allRows(plan).find((r) => r.key === 'entry-create')!.closed).toBe(false);
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

  it('holds the decisions of the fixture plan as entries, with nothing of the old list of decisions', () => {
    const data = buildData();
    expect(data.entries.filter((entry) => entry.kind === 'decision').map((entry) => entry.id).sort()).toEqual(DECISIONS.map((decision) => decision.id).sort());
    expect(Object.keys(buildPlan(data, null))).not.toContain('decided');
  });
});

describe('the phrases of a change', () => {
  const names: ChangeName[] = ['added', 'moved', 'text', 'sets', 'needs', 'keeps', 'surfaces', 'suggest', 'notes', 'taken', 'dropped', 'cleared', 'replaced', 'started', 'built'];

  it('say each name in the page\'s words, the same for a decision and a feature', () => {
    expect(names.map((name) => changePhrase(name))).toEqual([
      'Added', 'Moved to another part', 'Text changed', 'What a yes would do changed', 'Needs changed', 'What it must keep changed', 'Where it shows changed',
      'Recommendation changed', 'Notes edited', 'Decided', 'Dropped', 'Back to not decided', 'Replaced by', 'Being built', 'Built',
    ]);
  });

  it('never let the name of a key through', () => {
    const line = changeLine(names);
    expect(line).not.toMatch(/\b(taken|sets|keeps|surfaces|suggest|cleared|replaced|started|planned)\b|[a-z]+_[a-z]+/);
    expect(line.split(' · ')).toHaveLength(names.length);
  });

  it('join the phrases of one change by a middle dot, in the order given', () => {
    expect(changeLine(['text', 'notes', 'taken'])).toBe('Text changed · Notes edited · Decided');
    expect(changeLine(['added'])).toBe('Added');
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
    const words = (state: PartGroup['state'], counts: Partial<PartGroup['counts']>) => stateWords({ state, counts: { built: 0, building: 0, decided: 0, undecided: 0, dropped: 0, ...counts } });
    expect(words('built', { built: 4 })).toBe('built');
    expect(words('building', { built: 1, building: 1, decided: 2 })).toBe('being built');
    expect(words('next', { decided: 3 })).toBe('next');
    expect(words('decided', { decided: 4 })).toBe('4 decided');
    expect(words('decided', { built: 1, decided: 4 })).toBe('1/5 built');
    expect(words('open', { undecided: 4 })).toBe('');
  });

  it('gives a part being built its own state, and keeps next for the first decided feature neither being built nor built', () => {
    const states = (data = buildData()) => buildPlan(data, null).parts.map((group) => [group.part.id, group.state]);
    // The fixture: Part A holds the first decided feature, Part C a feature being built.
    expect(states()).toEqual([['part-a', 'next'], ['part-b', 'open'], ['part-c', 'building'], ['part-d', 'open'], ['part-e', 'open']]);
    // Every decided feature built: Part A is built.
    expect(states(withBuilt('core', 'calls'))).toEqual([['part-a', 'built'], ['part-b', 'open'], ['part-c', 'building'], ['part-d', 'open'], ['part-e', 'open']]);
    // A part both next and being built reads being built, and no other part is next.
    const both = withFeatures({ core: { started: '2026-11-17' } });
    expect(states(both)).toEqual([['part-a', 'building'], ['part-b', 'open'], ['part-c', 'building'], ['part-d', 'open'], ['part-e', 'open']]);
    // Next moves to the first decided feature that is neither being built nor built.
    const later = withFeatures({ core: { started: '2026-11-17' }, calls: { started: '2026-11-17' }, fork: { verdict: 'take' } });
    expect(states(later)).toEqual([['part-a', 'building'], ['part-b', 'open'], ['part-c', 'building'], ['part-d', 'next'], ['part-e', 'open']]);
  });
});

describe('the line under the title', () => {
  const line = (data = buildData()) => statusLine(buildPlan(data, null).parts);
  const undecided = (...ids: string[]) => Object.fromEntries(ids.map((id) => [id, { verdict: undefined, started: undefined }]));

  /** The fixture with no feature being built. */
  const notStarted = (change: Record<string, Partial<(typeof FEATURES)[number]>> = {}) => withFeatures({ shells: { started: undefined }, ...change });

  it('says the part being built while one is', () => {
    expect(line()).toBe('Part C is being built.');
    expect(line(withBuilt('core', 'calls'))).toBe('Part C is being built.');
  });

  it('starts with the part holding the first decided feature, before anything is built', () => {
    expect(line(notStarted())).toBe('Starting with Part A.');
    expect(line(notStarted(undecided('core', 'calls')))).toBe('Starting with Part C.');
  });

  it('says what is next once something is built', () => {
    expect(line(notStarted(Object.fromEntries(['core', 'calls'].map((id) => [id, { verdict: 'take' as const, built: '2026-11-16' }]))))).toBe('Part C is next.');
  });

  it('says how many features are proposed while none is decided, dropped ones aside', () => {
    expect(line(withFeatures(undecided('core', 'calls', 'shells')))).toBe('19 features proposed, none decided yet.');
    expect(line(withFeatures({ ...undecided('core', 'calls', 'shells'), exec: { verdict: 'drop' } }))).toBe('18 features proposed, none decided yet.');
    const one = { part: { id: 'p', title: 'P', line: '', dependsOn: [] }, state: 'open' as const, counts: { built: 0, building: 0, decided: 0, undecided: 1, dropped: 0 } };
    expect(statusLine([one])).toBe('1 feature proposed, none decided yet.');
  });

  it('says nothing once all that was decided is built', () => {
    expect(line(withBuilt('core', 'calls', 'shells'))).toBe('');
  });

  it('is never moved by a decision: a taken decision is not a decided feature', () => {
    expect(line(buildData({ features: FEATURES.filter((feature) => feature.part !== 'part-a' && feature.id !== 'shells') }))).toBe('16 features proposed, none decided yet.');
    expect(line(buildData({ features: FEATURES.filter((feature) => feature.part !== 'part-a').map((feature) => (feature.id === 'shells' ? { ...feature, started: undefined } : feature)) }))).toBe('Starting with Part C.');
  });
});

describe('a replaced decision', () => {
  const decision = (fields: Partial<Entry>): Entry => ({ id: 'x', kind: 'decision', part: 'p', stage: 's', position: 1, name: 'X?', what: 'Yes.', needs: [], verdict: 'take', by: null, replaces: [], replacedBy: null, started: null, built: null, notes: false, more: false, history: null, ...fields });
  const status = (fields: Partial<Entry>) => entryStatuses([decision(fields)]).get('x');

  it('has the status replaced once its file names the decision that replaced it, and is decided before', () => {
    expect(status({})).toBe('decided');
    expect(status({ replacedBy: 'y' })).toBe('replaced');
    // Not decided, not dropped, and no verdict makes it neither.
    expect(status({ replacedBy: 'y', verdict: 'drop' })).toBe('dropped');
    expect(status({ replaces: ['y'] })).toBe('decided');
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
    expect(byPart(plan).get('part-e')!.rows.find((row) => row.key === 'entry-signed-logs')).toMatchObject({ status: 'decided', replacedBy: null, replaces: ['entry-plain-logs'] });
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
    expect(allRows(plan).filter((row) => row.key === 'entry-plain-logs')).toHaveLength(1);
  });

  it('is named by no dropped decision: a dropped one replaces nothing', () => {
    const decisions = DECISIONS.map((item) => (item.id === 'tamper-proof-logs' ? { ...item, verdict: 'drop' as const } : item));
    const row = buildPlan(buildData({ decisions }), null).parts.flatMap((group) => group.rows).find((item) => item.key === 'entry-tamper-proof-logs');
    expect(row).toMatchObject({ status: 'dropped', replaces: [] });
  });
});

describe('the films', () => {
  const film = (fields: Partial<Film>): Film => ({ id: 'one', title: 'One', line: 'A line.', shows: [], behind: [], playable: true, ...fields });

  it('gives each film the parts of the entries it shows, in the parts\' order, and its rows in the plan\'s order', () => {
    const data = { ...buildData(), films: [film({ id: 'across', shows: ['one-way', 'core', 'security-properly', 'gone-entry'] }), film({ id: 'alone', shows: ['create'] })] };
    const plan = buildPlan(data, null);
    expect(plan.films.map((one) => [one.id, one.parts, one.rows])).toEqual([
      ['across', ['part-a', 'part-d'], ['entry-security-properly', 'entry-core', 'entry-one-way']],
      ['alone', ['part-b'], ['entry-create']],
    ]);
    const rows = new Map(allRows(plan).map((row) => [row.key, row]));
    expect(rows.get('entry-core')?.films).toEqual(['across']);
    expect(rows.get('entry-calls')?.films).toEqual([]);
  });

  it('marks an entry a film is behind on, and no other', () => {
    const plan = buildPlan({ ...buildData(), films: [film({ shows: ['core', 'calls'], behind: ['calls'] })] }, null);
    const rows = new Map(allRows(plan).map((row) => [row.key, row]));
    expect(rows.get('entry-calls')?.filmBehind).toBe(true);
    expect(rows.get('entry-core')?.filmBehind).toBe(false);
  });
});
