import { describe, expect, it } from 'vitest';
import { areaGroups } from '../../../tools/plan/model/index.js';
import { parsePlan } from '../../../tools/plan/read/index.js';
import { buildData, FEATURES, issue, liveState, planFiles } from './fixtures';
import { awaitsVerdict, buildPlan, changeLine, changeWords, decisionsAbout, decisionTitle, entryStatus, relativeTime, statusLine, stateWords } from './plan';
import type { AreaGroup, Change } from './plan';

const names = (rows: { name: string }[]) => rows.map((row) => row.name);
const byArea = (plan: ReturnType<typeof buildPlan>) => new Map(plan.areas.map((group) => [group.area.id, group]));
const building = (entry: string) => issue({ entry, verdict: 'take', building: true });
const taken = (entry: string) => issue({ entry, verdict: 'take' });
/** The plan with these entries built, as the file says it. */
const withBuilt = (...ids: string[]) => buildData({ features: FEATURES.map((feature) => (ids.includes(feature.id) ? { ...feature, verdict: 'take' as const, built: '2026-11-16' } : feature)) });

describe('the plan by area', () => {
  it('lists what is not carried over: entries without an area, then every dropped entry', () => {
    const plan = buildPlan(buildData(), liveState({ 13: issue({ entry: 'exec', verdict: 'drop', state: 'closed', reason: 'not_planned' }) }));
    expect(names(plan.left)).toEqual(['A hosted reviewer', 'An earlier desktop stack', 'Run a command']);
    expect(plan.left.map((r) => r.status)).toEqual(['dropped', 'undecided', 'dropped']);
    // A dropped entry still shows in its own area.
    expect(names(byArea(plan).get('computers')!.rows)).toContain('Run a command');
  });

  it('reads the status from the file, and a live verdict label wins over the file\'s verdict', () => {
    const plan = buildPlan(buildData(), liveState({ 10: issue({ entry: 'engine', verdict: 'later' }) }));
    const engine = byArea(plan).get('engine')!;
    // `engine` is taken in its file and later on GitHub; `calls` has only its file's take.
    expect(engine.rows.map((r) => r.status)).toEqual(['later', 'planned']);
    expect(engine.counts).toEqual({ built: 0, building: 0, planned: 1, later: 1, undecided: 0, dropped: 0 });
    expect(buildPlan(buildData(), null).areas[0].rows.map((r) => r.status)).toEqual(['planned', 'planned']);
    // A live drop beats a file take too.
    const dropped = buildPlan(buildData(), liveState({ 11: issue({ entry: 'calls', verdict: 'drop' }) }));
    expect(byArea(dropped).get('engine')!.rows.map((r) => r.status)).toEqual(['planned', 'dropped']);
  });

  it('is built only when the file says so, whatever the issue says', () => {
    const closed = issue({ entry: 'fork', verdict: 'take', state: 'closed', reason: 'completed', closedAt: '2026-11-16T00:00:00Z' });
    expect(byArea(buildPlan(buildData(), liveState({ 14: closed }))).get('forks')!.counts.built).toBe(0);
    const plan = buildPlan(withBuilt('fork'), liveState({ 14: issue({ entry: 'fork', verdict: 'later' }) }));
    const fork = byArea(plan).get('forks')!;
    expect(fork.counts.built).toBe(1);
    expect(fork.rows[0]).toMatchObject({ status: 'built', built: '2026-11-16' });
    expect(buildPlan(withBuilt('fork'), null).areas[4].rows[0].status).toBe('built');
  });

  it('marks a planned entry whose issue says building, and only a planned one', () => {
    const feature = (verdict: 'take' | 'later' | 'drop' | null, built: string | null = null) => ({ verdict, built });
    expect(entryStatus(feature('take'), issue({ verdict: 'take', building: true }))).toBe('building');
    expect(entryStatus(feature('take'), issue({ building: true }))).toBe('building');
    expect(entryStatus(feature(null), issue({ building: true }))).toBe('undecided');
    expect(entryStatus(feature('take'), issue({ verdict: 'later', building: true }))).toBe('later');
    expect(entryStatus(feature('take', '2026-11-16'), issue({ verdict: 'take', building: true }))).toBe('built');
    expect(entryStatus(feature('take'), undefined)).toBe('planned');
  });

  it('sorts suggestions by votes with dropped ones last, and lists the decisions of the file newest first', () => {
    const plan = buildPlan(buildData(), liveState({
      32: issue({ kind: 'suggestion', title: 'Dropped idea', up: 50, verdict: 'drop', state: 'closed', reason: 'not_planned', author: 'x' }),
    }));
    expect(names(plan.suggestions)).toEqual(['Share a base computer', 'Start a task from Slack', 'Dropped idea']);
    expect(plan.decided.map((r) => r.key)).toEqual(['decided-engine-in-zig', 'decided-merge-main-only', 'decided-security-properly']);
    expect(names(plan.decided)).toEqual(['The engine is written in Zig', 'Merging changes main only', 'Security is built properly']);
    expect(plan.decided[0]).toMatchObject({ kind: 'decided', status: 'planned', date: '2026-11-16', about: ['engine'], what: 'The engine is written in Zig. It is the core piece.' });
    expect(buildPlan(buildData(), null).decided).toHaveLength(3);
    expect(decisionTitle('GitHub is the storage: issues. Agents post as a bot.')).toBe('GitHub is the storage');
  });

  it('groups the areas with the model\'s areaGroups: rows, counts, states and the first band', () => {
    const plan = buildPlan(buildData(), liveState());
    const statuses = new Map(plan.areas.flatMap((group) => group.rows).map((row) => [row.key.slice('entry-'.length), row.status]));
    const model = areaGroups(parsePlan(planFiles()).plan, (id: string) => statuses.get(id) ?? 'undecided');
    const shape = (groups: { area: { id: string }; rows: { id?: string; key?: string }[]; counts: unknown; state: string; first: boolean; number: number }[]) =>
      groups.map((group) => [group.area.id, group.number, group.rows.map((row) => row.id ?? row.key?.slice('entry-'.length)), group.counts, group.state, group.first]);
    expect(shape(plan.areas)).toEqual(shape(model));
  });

  it('attaches the decisions about an entry\'s area, never by its id', () => {
    const plan = buildPlan(buildData(), null);
    const rows = new Map(plan.areas.flatMap((group) => group.rows).map((r) => [r.key, r]));
    expect(rows.get('entry-engine')!.decisions.map((d) => d.id)).toEqual(['engine-in-zig']);
    expect(rows.get('entry-calls')!.decisions.map((d) => d.id)).toEqual(['engine-in-zig']);
    expect(rows.get('entry-fork')!.decisions.map((d) => d.id)).toEqual(['merge-main-only']);
    expect(rows.get('entry-diff')!.decisions.map((d) => d.id)).toEqual(['merge-main-only']);
    expect(rows.get('entry-exec')!.decisions).toEqual([]);
    const data = buildData();
    expect(decisionsAbout(data.decisions, { area: null })).toEqual([]);
    // `about` holds area ids: one equal to a feature's id says nothing about that feature.
    const byId = [{ id: 'd', date: '2026-11-16', text: 'T.', about: ['fork'] }];
    expect(decisionsAbout(byId, { area: 'forks' })).toEqual([]);
    expect(decisionsAbout(byId, { area: 'fork' })).toEqual(byId);
  });

  it('counts people\'s comments as the issue\'s comments less its history lines', () => {
    const plan = buildPlan(buildData(), liveState());
    const computers = byArea(plan).get('computers')!.rows[0];
    expect(computers).toMatchObject({ comments: 2, updates: 3, up: 12, issue: 12 });
    expect(buildPlan(buildData(), liveState({ 12: issue({ entry: 'computers', comments: 1, updates: 4 }) })).areas[1].rows[0]).toMatchObject({ comments: 0, updates: 1 });
  });

  it('puts the latest items on their entries, newest first as given, and skips one of another issue', () => {
    const at = '2026-11-16T10:00:00Z';
    const plan = buildPlan(buildData(), liveState({}, [
      { n: 12, entry: 'computers', author: 'mara-k', bot: false, createdAt: at, text: 'Hello' },
      { n: 99, entry: 'exec', author: 'x', bot: false, createdAt: at, text: 'Wrong issue' },
      { n: 10, entry: 'engine', author: 'silo-workshop[bot]', bot: true, createdAt: at, change: { fields: ['verdict'], verdict: 'take', before: 'a'.repeat(40), commit: 'b'.repeat(40) } as Change },
      { n: 5, entry: 'nothing', author: 'x', bot: false, createdAt: at, text: 'Gone' },
    ]));
    expect(plan.latest.map((item) => [item.row.key, item.text, item.change?.verdict ?? null])).toEqual([['entry-computers', 'Hello', null], ['entry-engine', null, 'take']]);
  });

  it('waits for a verdict only on undecided rows with an issue', () => {
    const plan = buildPlan(buildData(), liveState());
    expect(plan.areas[1].rows.map(awaitsVerdict)).toEqual([true, true]);
    expect(plan.areas[0].rows.map(awaitsVerdict)).toEqual([false, false]);
    expect(buildPlan(buildData(), null).areas[1].rows.map(awaitsVerdict)).toEqual([false, false]);
  });
});

describe('history words', () => {
  const change = (fields: string[], extra: Partial<Change> = {}) => ({ fields, before: 'a'.repeat(40), commit: 'b'.repeat(40), ...extra }) as Change;

  it('name each change in its phrase, never a raw key, joined by a middle dot', () => {
    expect(changeWords(change(['verdict'], { verdict: 'take' }))).toEqual(['Taken']);
    expect(changeWords(change(['verdict'], { verdict: 'drop' }))).toEqual(['Dropped']);
    expect(changeWords(change(['verdict'], { verdict: 'later' }))).toEqual(['Later']);
    expect(changeWords(change(['verdict']))).toEqual(['Verdict cleared']);
    expect(changeWords(change(['built'], { built: '2026-11-16' }))).toEqual(['Built']);
    expect(changeWords(change(['name', 'what']))).toEqual(['Text changed']);
    expect(changeWords(change(['notes']))).toEqual(['Notes edited']);
    expect(changeWords(change(['name']))).toEqual(['Text changed']);
    expect(changeWords(change(['needs']))).toEqual(['Needs changed']);
    expect(changeWords(change(['area']))).toEqual(['Moved to another part']);
    expect(changeWords(change(['suggest']))).toEqual(['Suggestion changed']);
    expect(changeWords(change(['surfaces']))).toEqual(['Surfaces changed']);
    expect(changeWords(change(['suggested_in']))).toEqual([]);
    expect(changeWords(change(['needs', 'verdict'], { verdict: 'take' }))).toEqual(['Needs changed', 'Taken']);
    const every = change(['name', 'what', 'area', 'needs', 'surfaces', 'suggest', 'suggested_in', 'verdict', 'built', 'notes'], { verdict: 'take', built: '2026-11-16' });
    expect(changeLine(every)).toBe('Text changed · Moved to another part · Needs changed · Surfaces changed · Suggestion changed · Taken · Built · Notes edited');
    expect(changeLine(every)).not.toMatch(/[a-z]+_[a-z]+|\b(name|what|area|needs|surfaces|suggest|verdict|built|notes)\b/);
  });

  it('says how long ago', () => {
    const now = Date.parse('2026-11-17T12:00:00Z');
    expect(relativeTime('2026-11-17T11:59:00Z', now)).toBe('just now');
    expect(relativeTime('2026-11-17T10:00:00Z', now)).toBe('2 hours ago');
    expect(relativeTime('2026-11-16T10:00:00Z', now)).toBe('yesterday');
    expect(relativeTime('2026-11-10T10:00:00Z', now)).toBe('7 days ago');
  });
});

describe('area states', () => {
  it('says the few words on a part', () => {
    const words = (state: AreaGroup['state'], counts: Partial<AreaGroup['counts']>) => stateWords({ state, counts: { built: 0, building: 0, planned: 0, later: 0, undecided: 0, dropped: 0, ...counts } });
    expect(words('done', { built: 4 })).toBe('done');
    expect(words('now', { built: 2, building: 2, planned: 3 })).toBe('now · 2/7');
    expect(words('next', { planned: 3 })).toBe('next');
    expect(words('planned', { planned: 4 })).toBe('4 planned');
    expect(words('planned', { built: 1, planned: 4 })).toBe('1/5 built');
    expect(words('open', { undecided: 4 })).toBe('');
  });
});

describe('the line under the title', () => {
  const line = (extra: Parameters<typeof liveState>[0], data = buildData()) => statusLine(buildPlan(data, liveState(extra)).areas);
  const later = (entry: string) => issue({ entry, verdict: 'later' });

  it('starts with the next area before anything is built', () => {
    expect(line({})).toBe('Starting with the Engine.');
    expect(line({ 10: later('engine'), 11: later('calls') })).toBe('Starting with Your setup.');
  });

  it('names what is being built, and what is next', () => {
    expect(line({ 10: building('engine') })).toBe('Building Engine. Your setup is next.');
    expect(line({ 10: building('engine'), 15: building('shells') })).toBe('Building Engine and Your setup.');
    expect(line({ 10: building('engine'), 15: building('shells'), 14: building('fork') })).toBe('Building Engine, Forks and Your setup.');
  });

  it('says what is next once something is built', () => {
    expect(line({}, withBuilt('engine', 'calls'))).toBe('Your setup is next.');
  });

  it('says nothing when nothing is next or building', () => {
    expect(line({}, withBuilt('engine', 'calls', 'shells'))).toBe('');
    expect(line({ 10: later('engine'), 11: later('calls'), 15: later('shells') })).toBe('');
  });

  it('follows the area order when two are next candidates', () => {
    expect(line({ 13: taken('exec') })).toBe('Starting with the Engine.');
  });
});
