import { execFileSync } from 'node:child_process';
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { canonicalText } from '../../../tools/plan/model/index.js';
import { afterEach, describe, expect, it } from 'vitest';
import { loadDir } from '../../../tools/plan/read/index.js';
import { buildDataFrom, factsFrom, filmOrder, historiesFrom, historyFromGit, layoutPlanFrom, notesFrom, planDir, readBuildData, readFilms, readHistories, readHistory, printedChanged, unshallow } from './data';
import type { Git } from './data';
import { made } from '../../scripts/films-test-support.mjs';
import { buildData, CORE_NOTES, DECISIONS, emptyPlanFiles, FEATURES, PARTS, planFiles, withLists, ZIG_NOTES } from './fixtures';
import { checkLayout, LayoutError, MAP_HEIGHT, MAP_WIDTH, NO_MAP, SILO_MAP, pageLayout } from './map';
import type { Box, MapLayout, PageLayout, Wire } from './map';
import { EXAMPLE_MAP } from './map-example';
import { MAP_PIXELS, MONO_ADVANCE, SERIF_WIDTHS, shownTitle, stateNeeds, TITLE_FACE, TITLE_STYLE, titleEms, unknownCharacters } from './map-fit';
import { stateWords } from './plan';
import type { Counts, PartState } from './plan';

const SAMPLE = join(dirname(new URL(import.meta.url).pathname), '../../../tools/plan/test/fixtures/sample');

describe('reading the plan files', () => {
  it('reads decisions and features in the order of order.json, with their position, part and needs', () => {
    const data = buildData();
    expect(data.entries).toHaveLength(FEATURES.length + DECISIONS.length);
    expect(data.entries.map((entry) => entry.position)).toEqual(data.entries.map((_, index) => index + 1));
    expect(data.entries.slice(0, 5).map((entry) => entry.id)).toEqual(['security-properly', 'one-host', 'plain-logs', 'core-in-zig', 'intake']);
    const engine = data.entries.find((entry) => entry.id === 'core');
    expect(engine).toMatchObject({ kind: 'feature', part: 'part-a', stage: 'second', verdict: 'take', built: null, needs: [] });
    expect(engine).not.toHaveProperty('suggest');
    expect(data.entries.find((entry) => entry.id === 'create')?.verdict).toBeNull();
  });

  it('reads a decision as a row: its asks is its name, its answer its body, and a decision of the whole product has no part', () => {
    const entries = buildData().entries;
    expect(entries.find((entry) => entry.id === 'core-in-zig')).toMatchObject({ kind: 'decision', part: 'part-a', name: 'Which language is the core written in?', what: 'Zig. It is the core piece.', verdict: 'take', notes: true });
    expect(entries.find((entry) => entry.id === 'security-properly')).toMatchObject({ kind: 'decision', part: null });
    expect(entries.find((entry) => entry.id === 'one-host')).toMatchObject({ what: null, verdict: null });
  });

  it('reads the notes of an entry beside its file, and none when there are none', () => {
    const entries = buildData().entries;
    expect(entries.find((f) => f.id === 'core')?.notes).toBe(true);
    expect(entries.find((f) => f.id === 'calls')?.notes).toBe(false);
    expect(buildData({ notes: { core: '  \n' } }).entries.find((f) => f.id === 'core')?.notes).toBe(false);
    // The text is a chunk of its own: never in the page's data.
    expect(notesFrom(planFiles())).toEqual({ 'core-in-zig': ZIG_NOTES, core: CORE_NOTES });
    expect(notesFrom(planFiles({ notes: { core: '  \n' } }))).toEqual({});
    expect(JSON.stringify(buildData())).not.toContain('The engine owns every computer');
  });

  it('holds no switch for entries\' threads: the page\'s data has none, in the real build and in the fixtures', () => {
    expect(buildDataFrom(emptyPlanFiles(), { map: NO_MAP })).not.toHaveProperty('threads');
    expect(buildData()).not.toHaveProperty('threads');
    expect(buildData()).not.toHaveProperty('decide');
  });
});

describe('what an entry says of the plan\'s lists', () => {
  const files = () => planFiles(withLists());
  const more = (id: string) => buildData(withLists()).entries.find((entry) => entry.id === id)?.more;

  it('marks the entries that have something to show given their verdict, and no other', () => {
    // No verdict: a recommendation, what a yes sets, what it removes. A feature's keeps and surfaces whatever its verdict.
    expect(['create', 'name-a-snapshot', 'retire-scrap', 'bound-a-wait', 'fork-keeps'].map(more)).toEqual([true, true, true, true, true]);
    // Taken: what it added. Its recommendation is not shown any more.
    expect(['what-we-call-things', 'leak-guard'].map(more)).toEqual([true, true]);
    expect(['core', 'calls', 'security-properly', 'one-way', 'reviewer'].map(more)).toEqual([false, false, false, false, false]);
    expect(buildData().entries.some((entry) => entry.more)).toBe(true);
  });

  it('keeps what a taken entry says of a recommendation out of the chunk, and a dropped one\'s sets', () => {
    const facts = factsFrom(files());
    expect(facts.entries.core).toBeUndefined();
    expect(facts.entries['leak-guard']).toEqual({ adds: ['never-leaks', 'ends-up'] });
    const dropped = factsFrom(planFiles({ ...withLists(), decisions: withLists()?.decisions?.map((decision) => (decision.id === 'bound-a-wait' ? { ...decision, verdict: 'drop' as const } : decision)) }));
    expect(dropped.entries['bound-a-wait']).toBeUndefined();
  });

  it('reads what an entry sets as statements: a word, a principle and a property, each with its id', () => {
    const { entries } = factsFrom(files());
    expect(entries['name-a-snapshot'].sets).toEqual([
      { id: 'snapshot', kind: 'word', word: 'snapshot', text: 'A saved moment of a computer.', asks: null },
      { id: 'least-power', kind: 'principle', word: null, text: 'The less sure the trigger, the less it may do, and it asks first.', asks: 'What triggers it, and what does it ask?' },
    ]);
    expect(entries['name-a-snapshot'].suggest).toEqual({ verdict: 'take', why: 'The word is already in use.' });
    expect(entries['bound-a-wait'].sets).toEqual([{ id: 'wait-is-bounded', kind: 'property', word: null, text: 'A wait at the edge ends within its limit.', asks: null }]);
    expect(entries['retire-scrap']).toMatchObject({ removes: ['scrap'], suggest: { verdict: 'drop' } });
    expect(entries['fork-keeps']).toEqual({ keeps: ['never-leaks', 'ends-up'], surfaces: { app: 'Computers, then Fork', cli: 'silo fork <computer>' } });
  });

  it('reads the statements in force, by id, a word by its lower-case id', () => {
    const { statements } = factsFrom(files());
    expect(Object.keys(statements).sort()).toEqual(['ends-up', 'least-power', 'never-leaks', 'scrap', 'twin']);
    expect(statements.twin).toEqual({ id: 'twin', kind: 'word', word: 'twin', text: 'A copy of a computer.', asks: null });
    expect(statements['least-power']).toMatchObject({ kind: 'principle', asks: 'What triggers it?' });
    expect(statements['never-leaks']).toEqual({ id: 'never-leaks', kind: 'property', word: null, text: 'Nothing leaves a computer unasked.', asks: null });
  });

  it('keeps all of it out of the page\'s own data: the first load only learns which entries have something', () => {
    const text = JSON.stringify(buildData(withLists()));
    for (const quiet of ['A saved moment of a computer', 'Nothing leaves a computer unasked', 'silo fork', 'The word is already in use']) expect(text).not.toContain(quiet);
  });
});

describe('a plan that holds no part', () => {
  it('reads with no part and no feature, and keeps the decisions of the whole product', () => {
    const data = buildDataFrom(emptyPlanFiles(), { map: NO_MAP });
    expect(data).toMatchObject({ parts: [], map: NO_MAP });
    expect(data.entries.map((entry) => entry.id)).toEqual(['security-properly', 'one-host']);
    expect(notesFrom(emptyPlanFiles())).toEqual({});
  });

  it('reads a part with no feature: nothing stops the build', () => {
    const data = buildDataFrom(planFiles({ features: FEATURES.filter((feature) => feature.part !== 'part-b') }), { map: EXAMPLE_MAP });
    expect(data.parts.map((part) => part.id)).toContain('part-b');
  });
});

describe('a plan with an error', () => {
  it('stops with the file and the fix when a feature is missing from order.json', () => {
    const files = planFiles();
    files.set('part-a/features/stray.json', canonicalText({ name: 'Stray', what: 'Not in the order.' }));
    let message = '';
    try {
      buildDataFrom(files, { map: EXAMPLE_MAP });
    } catch (error) {
      message = (error as Error).message;
    }
    expect(message).toContain('plan/');
    expect(message).toContain('stray');
    expect(message).toContain('Fix:');
  });

  it('stops on a part the map has no place for; a layout that places nothing draws no map and takes any plan', () => {
    const files = planFiles({ parts: [...PARTS, { id: 'billing', title: 'Billing', line: 'Money.' }] });
    expect(() => buildDataFrom(files, { map: EXAMPLE_MAP })).toThrow('The map has no place for the part "billing" of plan/parts.json. Add it to website/src/workshop/map-example.ts.');
    expect(() => buildDataFrom(files)).toThrow('The map has no place for the part "part-a" of plan/parts.json.');
    expect(buildDataFrom(files, { map: NO_MAP }).map).toBe(NO_MAP);
  });
});

describe('reading plan/ from a repository folder', () => {
  it('reads the folder, and stops when it cannot be read', () => {
    const root = mkdtempSync(join(tmpdir(), 'workshop-data-'));
    try {
      for (const [name, text] of planFiles()) {
        mkdirSync(dirname(join(root, 'plan', name)), { recursive: true });
        writeFileSync(join(root, 'plan', name), text);
      }
      const data = readBuildData(root, EXAMPLE_MAP);
      expect(data.entries).toHaveLength(FEATURES.length + DECISIONS.length);
      expect(data.parts).toHaveLength(PARTS.length);
      expect(data.films).toEqual([]);
      expect(() => readBuildData(join(root, 'missing'))).toThrow('cannot be read');
      // Made-up films: tiny, and another one beside it.
      const { put } = made(root, join(REPOSITORY, 'films', 'player', 'page.html'));
      cpSync(join(root, 'films', 'tiny'), join(root, 'films', 'another'), { recursive: true });
      put('another/film.json', JSON.stringify({ id: 'another', title: 'Another' }));
      expect(readBuildData(root, EXAMPLE_MAP).films.map(({ id, title, playable }) => ({ id, title, playable }))).toEqual([{ id: 'another', title: 'Another', playable: true }, { id: 'tiny', title: expect.any(String), playable: true }]);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  it('reads the plan named by WORKSHOP_PLAN instead: the plan tool\'s sample', () => {
    expect(planDir('/repo', {})).toBe('/repo/plan');
    expect(planDir('/repo', { WORKSHOP_PLAN: SAMPLE })).toBe(SAMPLE);
    const data = buildDataFrom(loadDir(SAMPLE), { map: NO_MAP });
    expect(data.parts.map((part) => part.id)).toEqual(['engine', 'record']);
    expect(data.entries.filter((entry) => entry.part === null).map((entry) => entry.id)).toEqual(['the-design', 'nothing-without-a-reason']);
  });
});

const REPOSITORY = join(dirname(new URL(import.meta.url).pathname), '../../..');

describe('the history of the entries', () => {
  const step = (commit: string, date: string, path: string, before: unknown, after: unknown) => ({
    commit, date, files: { [path]: { before: before === null ? null : JSON.stringify(before), after: JSON.stringify(after) } },
  });
  const file = { asks: 'Which?', answer: 'This one.' };
  const steps = [
    step('a'.repeat(40), '2026-11-01T10:00:00Z', 'decisions/which.json', null, { asks: 'Which?' }),
    step('b'.repeat(40), '2026-11-02T10:00:00Z', 'decisions/which.json', { asks: 'Which?' }, file),
    step('c'.repeat(40), '2026-11-03T10:00:00Z', 'decisions/which.json', file, { ...file, verdict: 'take' }),
  ];
  /** A git that reads from a list of answers and starts no process. */
  const fakeGit = (reads: { steps: typeof steps; shallow: boolean }[], unshallow: Git['unshallow'] = () => {}) => {
    const calls: string[] = [];
    let read = 0;
    const git: Git = {
      readSteps: () => { calls.push('read'); return reads[Math.min(read++, reads.length - 1)]; },
      unshallow: (root) => { calls.push('unshallow'); unshallow(root); },
    };
    return { git, calls };
  };

  describe('on real git', () => {
    const run = (cwd: string, ...args: string[]) => execFileSync('git', ['-C', cwd, ...args], { encoding: 'utf8', env: { ...process.env, GIT_TERMINAL_PROMPT: '0' } }).trim();
    const commits = (cwd: string) => Number(run(cwd, 'rev-list', '--count', 'HEAD'));
    const made: string[] = [];
    afterEach(() => { for (const dir of made.splice(0)) rmSync(dir, { recursive: true, force: true }); });

    /** A local repository of five commits on a branch other than the default one, and a clone of it made as Vercel makes one: shallow, with no remote. */
    function vercelClone() {
      const dir = mkdtempSync(join(tmpdir(), 'workshop-clone-'));
      made.push(dir);
      const origin = join(dir, 'origin');
      mkdirSync(origin);
      run(origin, 'init', '-q', '-b', 'main');
      run(origin, 'config', 'user.email', 'a@b.c');
      run(origin, 'config', 'user.name', 'a');
      run(origin, 'config', 'commit.gpgsign', 'false');
      run(origin, 'commit', '-q', '--allow-empty', '-m', 'first');
      run(origin, 'checkout', '-q', '-b', 'preview');
      for (let i = 2; i <= 5; i++) run(origin, 'commit', '-q', '--allow-empty', '-m', `commit ${i}`);
      const clone = join(dir, 'clone');
      execFileSync('git', ['clone', '-q', '--depth=3', '--branch', 'preview', `file://${origin}`, clone]);
      run(clone, 'remote', 'remove', 'origin');
      return { origin, clone, address: `file://${origin}` };
    }

    it('fetches the whole history of the commit being built from the public address when there is no origin', () => {
      const { clone, address } = vercelClone();
      const head = run(clone, 'rev-parse', 'HEAD');
      expect(commits(clone)).toBe(3);
      unshallow(clone, address);
      expect(commits(clone)).toBe(5);
      expect(run(clone, 'rev-parse', 'HEAD')).toBe(head);
      expect(run(clone, 'rev-parse', '--is-shallow-repository')).toBe('false');
      expect(run(clone, 'status', '--porcelain')).toBe('');
    });

    it('goes on shallow when neither origin nor the public address can be fetched', () => {
      const { clone, origin } = vercelClone();
      rmSync(origin, { recursive: true, force: true });
      expect(() => unshallow(clone, `file://${origin}`)).toThrow();
      expect(commits(clone)).toBe(3);
    });
  });

  it('turns the steps of git into each entry\'s changes, newest first', () => {
    const { git, calls } = fakeGit([{ steps, shallow: false }]);
    const source = historyFromGit('/repo', git);
    expect(calls).toEqual(['read']);
    expect(source.shallow).toBe(false);
    expect(source.changes.get('which')?.map((change) => [change.commit[0], change.what])).toEqual([['c', ['taken']], ['b', ['text']], ['a', ['added']]]);
  });

  it('fetches a shallow clone whole from origin once, and reads again', () => {
    const { git, calls } = fakeGit([{ steps: steps.slice(2), shallow: true }, { steps, shallow: false }]);
    const source = historyFromGit('/repo', git);
    expect(calls).toEqual(['read', 'unshallow', 'read']);
    expect(source.shallow).toBe(false);
    expect(source.changes.get('which')).toHaveLength(3);
  });

  it('goes on with the clone it has when the fetch fails, or the clone stays shallow, and says so', () => {
    const failing = fakeGit([{ steps: steps.slice(2), shallow: true }], () => { throw new Error('no network'); });
    expect(historyFromGit('/repo', failing.git)).toMatchObject({ shallow: true });
    expect(failing.calls).toEqual(['read', 'unshallow', 'read']);
    const still = fakeGit([{ steps: steps.slice(2), shallow: true }]);
    const source = historyFromGit('/repo', still.git);
    expect(source.shallow).toBe(true);
    expect(still.calls.filter((call) => call === 'unshallow')).toHaveLength(1);
    expect(buildDataFrom(emptyPlanFiles(), { map: NO_MAP, history: source }).shallow).toBe(true);
    expect(buildDataFrom(emptyPlanFiles(), { map: NO_MAP }).shallow).toBe(false);
  });

  it('gives a plan read from another folder no history and no map: the history is the caller\'s, or none', () => {
    expect(readHistory(REPOSITORY)).not.toBeNull();
    const before = process.env.WORKSHOP_PLAN;
    process.env.WORKSHOP_PLAN = SAMPLE;
    try {
      expect(readHistory(REPOSITORY)).toBeNull();
      const data = readBuildData(REPOSITORY);
      expect(data.entries.every((entry) => entry.history === null)).toBe(true);
      expect(data.shallow).toBe(false);
      expect(data.map).toBe(NO_MAP);
      expect(readHistories(REPOSITORY)).toEqual({});
    } finally {
      if (before === undefined) delete process.env.WORKSHOP_PLAN;
      else process.env.WORKSHOP_PLAN = before;
    }
  });

  it('gives an entry of the real repository its history: main-and-its-agents has changes, in the build and in the chunk', () => {
    const entry = readBuildData(REPOSITORY).entries.find((item) => item.id === 'main-and-its-agents');
    expect(entry?.history).not.toBeNull();
    expect(entry?.history?.count).toBeGreaterThan(0);
    const changes = readHistories(REPOSITORY)['main-and-its-agents'];
    expect(changes.length).toBe(entry?.history?.count);
    expect(changes.at(-1)?.what).toEqual(['added']);
    expect(changes[0]).toMatchObject({ date: entry?.history?.date, what: entry?.history?.what });
    expect(changes[0].commit).toMatch(/^[0-9a-f]{40}$/);
  });

  it('holds in the chunk only the entries of the plan that have a change', () => {
    const files = planFiles();
    const source = { shallow: false, changes: new Map([['core', [{ commit: 'a'.repeat(40), date: '2026-11-01T10:00:00Z', what: ['added' as const] }]], ['gone', [{ commit: 'b'.repeat(40), date: '2026-11-01T10:00:00Z', what: ['added' as const] }]]]) };
    expect(Object.keys(historiesFrom(files, source))).toEqual(['core']);
    expect(buildDataFrom(files, { map: EXAMPLE_MAP, history: source }).entries.filter((entry) => entry.history).map((entry) => entry.id)).toEqual(['core']);
    expect(historiesFrom(files, null)).toEqual({});
  });
});

describe('the map layout', () => {
  const ids = PARTS.map((part) => part.id);
  // The fixture plan as the build gives it to the check, with only the parts named.
  const plan = (partIds: string[] = ids, options?: Parameters<typeof planFiles>[0]) => {
    const read = layoutPlanFrom(planFiles(options));
    return { ...read, parts: partIds.map((id) => read.parts.find((part) => part.id === id) ?? { id, title: id }) };
  };

  it('draws the fixture plan with the example, and Silo\'s plan with its own map', () => {
    expect(buildData().map).toEqual(pageLayout(EXAMPLE_MAP, plan()));
    expect(Object.keys(EXAMPLE_MAP.places)).toEqual(ids);
    // The real plan passes every check of Silo's map: its parts, what each wire rests on, and its titles.
    expect(readBuildData(REPOSITORY).map).toEqual(pageLayout(SILO_MAP, layoutPlanFrom(loadDir(join(REPOSITORY, 'plan')))));
    expect(() => checkLayout(plan(['engine', 'record']))).toThrow(LayoutError);
    // A layout that places nothing takes every plan, and the page shows the tiles alone.
    expect(() => checkLayout(plan(['engine', 'record']), NO_MAP)).not.toThrow();
  });

  it('places exactly the parts of the plan', () => {
    expect(() => checkLayout(plan(), EXAMPLE_MAP)).not.toThrow();
    expect(() => checkLayout(plan([...ids, 'billing']), EXAMPLE_MAP)).toThrow(LayoutError);
    expect(() => checkLayout(plan(ids.filter((id) => id !== 'part-c')), EXAMPLE_MAP)).toThrow('website/src/workshop/map-example.ts places the part "part-c", which plan/parts.json does not have.');
    const without = (id: string): MapLayout => ({ ...EXAMPLE_MAP, places: Object.fromEntries(Object.entries(EXAMPLE_MAP.places).filter(([place]) => place !== id)) });
    expect(() => checkLayout(plan(ids.filter((id) => id !== 'part-b')), without('part-b'))).toThrow('website/src/workshop/map-example.ts draws a wire for the part "part-b", which plan/parts.json does not have.');
    const noWires = { ...without('part-d'), wires: EXAMPLE_MAP.wires.filter((wire) => wire.of !== 'part-d') };
    expect(() => checkLayout(plan(ids.filter((id) => id !== 'part-d')), noWires)).toThrow('website/src/workshop/map-example.ts stacks a copy behind the part "part-d", which plan/parts.json does not have.');
  });

  describe('what a wire or a zone rests on', () => {
    const LOOK = 'Fix: look at the wire, then name what it rests on now.';
    // The example with the first wire, "uses", resting on `rests`; the build's own path, so its message.
    const uses = (rests: string[] | undefined): MapLayout => ({ ...EXAMPLE_MAP, wires: EXAMPLE_MAP.wires.map((wire, index) => (index === 0 ? { ...wire, rests } : wire)) });
    const build = (layout: MapLayout, options?: Parameters<typeof planFiles>[0]) => () => buildDataFrom(planFiles(options), { map: layout });

    it('keeps what each thing rests on out of the page\'s data', () => {
      expect(JSON.stringify(buildData().map)).not.toContain('rests');
      expect(buildData().map).not.toHaveProperty('file');
      expect(pageLayout(NO_MAP, plan())).toBe(NO_MAP);
      // Of what a thing rests on, the page gets only whether it is not decided.
      const keys = new Set<string>();
      const walk = (value: unknown) => {
        if (Array.isArray(value)) value.forEach(walk);
        else if (value && typeof value === 'object') for (const [key, inner] of Object.entries(value)) { keys.add(key); walk(inner); }
      };
      walk(buildData().map);
      walk(readBuildData(REPOSITORY).map);
      expect(keys.has('rests')).toBe(false);
      expect(keys.has('file')).toBe(false);
      expect(keys.has('undecided')).toBe(true);
    });

    it('names in each message the file of the layout it checked', () => {
      expect(build(uses(undefined))).toThrow('website/src/workshop/map-example.ts: the wire "uses" rests on nothing.');
      const real = layoutPlanFrom(loadDir(join(REPOSITORY, 'plan')));
      const silo = { ...SILO_MAP, zones: SILO_MAP.zones.map((zone) => ({ ...zone, rests: [] })) };
      expect(() => checkLayout(real, silo)).toThrow('website/src/workshop/map.ts: the zone "YOUR MAC" rests on nothing.');
      const { file: _, ...unnamed } = uses(undefined);
      expect(() => checkLayout(plan(), unnamed)).toThrow('the map\'s layout: the wire "uses" rests on nothing.');
    });

    it('passes a taken decision, a taken or built feature, a word of the list and a part', () => {
      expect(build(uses(['core-in-zig', 'part:part-a']))).not.toThrow();
      expect(build(uses(['core']))).not.toThrow();
      const built = FEATURES.map((feature) => (feature.id === 'calls' ? { ...feature, built: '2026-11-01' } : feature));
      expect(build(uses(['calls']), { features: built })).not.toThrow();
      expect(build(uses(['word:twin']), withLists())).not.toThrow();
      // A replaced decision was taken; the one that replaced it is in force.
      expect(build(uses(['signed-logs']))).not.toThrow();
    });

    it('stops on a wire of the full map that rests on nothing, named by its word or, with none, by its path', () => {
      expect(build(uses(undefined))).toThrow('website/src/workshop/map-example.ts: the wire "uses" rests on nothing. Fix: give it `rests`: the decisions and features of the plan it draws, words as word:<id>, or part:<id> for what only a part\'s line says.');
      expect(build(uses([]))).toThrow('the wire "uses" rests on nothing.');
      // The example's last wire, with its word taken off and what it rests on too.
      const bare = { ...EXAMPLE_MAP, wires: EXAMPLE_MAP.wires.map((wire, index, wires) => (index === wires.length - 1 ? { ...wire, label: undefined, rests: undefined } : wire)) };
      expect(build(bare)).toThrow('website/src/workshop/map-example.ts: the wire with no word, drawn "M784 178H932" rests on nothing.');
    });

    it('stops on a zone that rests on nothing', () => {
      const zones = EXAMPLE_MAP.zones.map((zone, index) => (index === 1 ? { ...zone, rests: [] } : zone));
      expect(build({ ...EXAMPLE_MAP, zones })).toThrow('website/src/workshop/map-example.ts: the zone "ZONE TWO" rests on nothing.');
    });

    it('stops on an id that is no decision or feature of the plan', () => {
      expect(build(uses(['no-such-decision']))).toThrow(`website/src/workshop/map-example.ts: the wire "uses" rests on "no-such-decision", which is no decision or feature of the plan. ${LOOK}`);
      expect(build(uses(['part-a']))).toThrow(`the wire "uses" rests on "part-a", which is no decision or feature of the plan. ${LOOK}`);
    });

    it('stops on an open decision: no answer says what to draw', () => {
      expect(build(uses(['one-host']))).toThrow(`website/src/workshop/map-example.ts: the wire "uses" rests on the decision "one-host", which is open: nobody has answered it, so nothing says what to draw. ${LOOK}`);
    });

    it('stops on a dropped decision, on a replaced one with the decision that replaced it, and on a dropped feature', () => {
      expect(build(uses(['old-format']))).toThrow(`website/src/workshop/map-example.ts: the wire "uses" rests on the decision "old-format", which was dropped. ${LOOK}`);
      expect(build(uses(['plain-logs']))).toThrow(`website/src/workshop/map-example.ts: the wire "uses" rests on the decision "plain-logs", which was replaced by "signed-logs". ${LOOK}`);
      expect(build(uses(['reviewer']))).toThrow(`website/src/workshop/map-example.ts: the wire "uses" rests on the feature "reviewer", which was dropped. ${LOOK}`);
    });

    it('stops on a word that is not in the list, and on a part that does not exist', () => {
      expect(build(uses(['word:twin']))).toThrow(`website/src/workshop/map-example.ts: the wire "uses" rests on the word "word:twin", which plan/words.json does not have. ${LOOK}`);
      expect(build(uses(['part:billing']))).toThrow(`website/src/workshop/map-example.ts: the wire "uses" rests on the part "part:billing", which plan/parts.json does not have. ${LOOK}`);
    });

    it('reads the rests of a zone, a place, a text and a copy when they have some, and names each problem once', () => {
      const layout: MapLayout = {
        ...EXAMPLE_MAP,
        zones: EXAMPLE_MAP.zones.map((zone, index) => (index === 0 ? { ...zone, rests: ['one-host'] } : zone)),
        places: { ...EXAMPLE_MAP.places, 'part-a': { ...EXAMPLE_MAP.places['part-a'], rests: ['old-format'] } },
        notes: EXAMPLE_MAP.notes.map((note, index) => (index === 0 ? { ...note, rests: ['part:billing'] } : note)),
        ghosts: EXAMPLE_MAP.ghosts.map((ghost, index) => (index === 1 ? { ...ghost, rests: ['word:scrap'] } : ghost)),
      };
      let message = '';
      try {
        build(layout)();
      } catch (error) {
        message = (error as Error).message;
      }
      expect(message.split('\n')).toEqual([
        'website/src/workshop/map-example.ts: the zone "ZONE ONE" rests on the decision "one-host", which is open: nobody has answered it, so nothing says what to draw. Fix: look at the zone, then name what it rests on now.',
        'website/src/workshop/map-example.ts: the place of the part "part-a" rests on the decision "old-format", which was dropped. Fix: look at the place, then name what it rests on now.',
        'website/src/workshop/map-example.ts: the text "Outside" rests on the part "part:billing", which plan/parts.json does not have. Fix: look at the text, then name what it rests on now.',
        'website/src/workshop/map-example.ts: copy 2 behind the part "part-d" rests on the word "word:scrap", which plan/words.json does not have. Fix: look at the copy, then name what it rests on now.',
      ]);
    });
  });

  describe('what is not decided', () => {
    // The example with the first wire, "uses", resting on `rests`, as the page gets it from the fixture plan.
    const uses = (rests: string[]): MapLayout => ({ ...EXAMPLE_MAP, wires: EXAMPLE_MAP.wires.map((wire, index) => (index === 0 ? { ...wire, rests } : wire)) });
    const drawn = (layout: MapLayout, options?: Parameters<typeof planFiles>[0]) => buildDataFrom(planFiles(options), { map: layout }).map;
    const taking = (id: string) => ({ decisions: DECISIONS.map((decision) => (decision.id === id ? { ...decision, verdict: 'take' as const } : decision)) });
    const takingFeature = (id: string, built?: string, started?: string) => ({ features: FEATURES.map((feature) => (feature.id === id ? { ...feature, verdict: 'take' as const, ...(started ? { started } : {}), ...(built ? { built } : {}) } : feature)) });
    // The page's layout with every mark of not decided taken off: what is left is the drawing itself.
    const bare = (layout: PageLayout) => JSON.parse(JSON.stringify(layout, (key, value) => (key === 'undecided' ? undefined : value)));

    it('draws the example\'s last wire, which rests on a feature with no verdict, as not decided, and the others as decided', () => {
      expect(buildData().map.wires.map((wire) => wire.undecided === true)).toEqual([false, false, false, true]);
      expect(buildData().map.key).toEqual(EXAMPLE_MAP.key);
    });

    it('builds a wire that rests on an answered decision, and draws it as not decided until the decision is taken, with the same layout', () => {
      const proposed = drawn(uses(['part:part-b', 'one-way']));
      expect(proposed.wires[0].undecided).toBe(true);
      const taken = drawn(uses(['part:part-b', 'one-way']), taking('one-way'));
      expect(taken.wires[0]).not.toHaveProperty('undecided');
      expect(bare(taken)).toEqual(bare(proposed));
    });

    it('treats a decision with only `sets` and no verdict as answered', () => {
      expect(drawn(uses(['bound-a-wait']), withLists()).wires[0].undecided).toBe(true);
    });

    it('builds a wire that rests on a feature with no verdict, and draws it as decided once the feature is taken, being built or built, with the same layout', () => {
      const proposed = drawn(EXAMPLE_MAP);
      for (const options of [takingFeature('export'), takingFeature('export', undefined, '2026-11-16'), takingFeature('export', '2026-11-16')]) {
        const taken = drawn(EXAMPLE_MAP, options);
        expect(taken.wires.every((wire) => wire.undecided === undefined)).toBe(true);
        expect(bare(taken)).toEqual(bare(proposed));
      }
    });

    it('takes no mark of not decided from a layout as written: pageLayout alone derives it from `rests`', () => {
      const wire: Wire = {
        of: 'part-b', d: 'M240 178H465', rests: ['part:part-b'],
        // @ts-expect-error A layout as written has no `undecided`: a mark written by hand would stay after the owner takes the decision.
        undecided: true,
      };
      // @ts-expect-error A zone as written has no `undecided` either.
      const zone: MapLayout['zones'][number] = { ...EXAMPLE_MAP.zones[0], undecided: true };
      // @ts-expect-error Nor a text.
      const note: MapLayout['notes'][number] = { ...EXAMPLE_MAP.notes[0], undecided: true };
      // @ts-expect-error The layout as the page gets it holds no `rests`.
      const page: PageLayout['wires'][number] = { of: 'part-b', d: 'M0 0H1', rests: ['part:part-b'] };
      // @ts-expect-error Nor the layout's file.
      const file: PageLayout = { ...NO_MAP, file: 'website/src/workshop/map.ts' };
      expect(page).toBeTruthy();
      expect(file).toBeTruthy();
      // A mark that got past the types anyway is dropped: what the wire, the zone and the text rest on is in force.
      const map = drawn({ ...EXAMPLE_MAP, wires: [wire, ...EXAMPLE_MAP.wires.slice(1)], zones: [zone, EXAMPLE_MAP.zones[1]], notes: [note, EXAMPLE_MAP.notes[1]] });
      expect(map.wires[0]).not.toHaveProperty('undecided');
      expect(map.zones[0]).not.toHaveProperty('undecided');
      expect(map.notes[0]).not.toHaveProperty('undecided');
    });

    it('drops a mark carried into a layout through a variable, which the type checker lets pass', () => {
      // Built apart, then put in a layout: no excess-property check sees `undecided` here, so this type-checks.
      const wire = { ...EXAMPLE_MAP.wires[0], undecided: true };
      const zone = { ...EXAMPLE_MAP.zones[0], undecided: true };
      const note = { ...EXAMPLE_MAP.notes[0], undecided: true };
      const layout: MapLayout = { ...EXAMPLE_MAP, wires: [wire, ...EXAMPLE_MAP.wires.slice(1)], zones: [zone, EXAMPLE_MAP.zones[1]], notes: [note, EXAMPLE_MAP.notes[1]] };
      // Each rests only on what is in force: a part.
      expect([wire.rests, zone.rests, note.rests]).toEqual([['part:part-b'], ['part:part-a'], ['part:part-b']]);
      const map = pageLayout(layout, plan());
      expect(map.wires[0]).not.toHaveProperty('undecided');
      expect(map.zones[0]).not.toHaveProperty('undecided');
      expect(map.notes[0]).not.toHaveProperty('undecided');
      expect(map.wires[0]).toEqual(pageLayout(EXAMPLE_MAP, plan()).wires[0]);
    });

    it('draws a thing as not decided when at least one id it rests on is proposed', () => {
      expect(drawn(uses(['core-in-zig', 'one-way', 'part:part-a'])).wires[0].undecided).toBe(true);
      expect(drawn(uses(['core-in-zig', 'part:part-a'])).wires[0]).not.toHaveProperty('undecided');
    });

    it('marks a zone and a text that rest on a proposal, and leaves a place and a copy that rest on one unmarked', () => {
      const layout: MapLayout = {
        ...EXAMPLE_MAP,
        zones: EXAMPLE_MAP.zones.map((zone, index) => (index === 1 ? { ...zone, rests: ['export'] } : zone)),
        places: { ...EXAMPLE_MAP.places, 'part-a': { ...EXAMPLE_MAP.places['part-a'], rests: ['one-way'] } },
        notes: EXAMPLE_MAP.notes.map((note, index) => (index === 0 ? { ...note, rests: ['one-way'] } : note)),
        ghosts: EXAMPLE_MAP.ghosts.map((ghost, index) => (index === 0 ? { ...ghost, rests: ['export'] } : ghost)),
      };
      const map = drawn(layout);
      expect(map.zones.map((zone) => zone.undecided === true)).toEqual([false, true]);
      // A part's box and a copy behind it are drawn by the part's state alone: the page gets no mark and no rests for them.
      expect(map.places['part-a']).not.toHaveProperty('undecided');
      expect(map.places['part-a']).not.toHaveProperty('rests');
      expect(map.places['part-b']).not.toHaveProperty('undecided');
      // The second text has no rests: it is drawn as decided.
      expect(map.notes.map((note) => note.undecided === true)).toEqual([true, false]);
      for (const ghost of map.ghosts) {
        expect(ghost).not.toHaveProperty('undecided');
        expect(ghost).not.toHaveProperty('rests');
      }
    });

    it('draws on Silo\'s map the channels, SSH, the servers and clouds and your phone as not decided, and the way to the internet as decided', () => {
      const map = readBuildData(REPOSITORY).map;
      // Each thing must exist first: a missing one would give the `undefined` that a decided thing gives.
      const wires = (word: string) => map.wires.filter((one) => one.label?.[2] === word);
      const wire = (word: string, count = 1) => {
        expect(wires(word)).toHaveLength(count);
        return wires(word);
      };
      expect(wire('requests')[0].undecided).toBeUndefined();
      expect(wire('calls')[0].undecided).toBeUndefined();
      expect(wire('SSH')[0].undecided).toBe(true);
      // The first "runs" is the engine to the computers; the second goes to a server or a cloud.
      expect(wire('runs', 2).map((one) => one.undecided)).toEqual([undefined, true]);
      const note = (title: string) => {
        const found = map.notes.filter((one) => one.title === title);
        expect(found).toHaveLength(1);
        return found[0];
      };
      expect(note('The internet').undecided).toBeUndefined();
      expect(note('The boundary\'s channels').undecided).toBe(true);
      expect(note('Your terminal').undecided).toBe(true);
      expect(note('A server or a cloud').undecided).toBeUndefined();
      // Your phone is a zone like the Mac, outside it, and rests on features with no verdict: its outline is dashed, the Mac's
      // is solid. Neither it nor its text has a folded place: both leave the folded map.
      const zone = (label: string) => {
        const found = map.zones.filter((one) => one.label === label);
        expect(found).toHaveLength(1);
        return found[0];
      };
      const phone = zone('YOUR PHONE');
      const mac = zone('YOUR MAC');
      expect(phone.undecided).toBe(true);
      expect(mac.undecided).toBeUndefined();
      expect(phone.corners).toBe(mac.corners);
      expect(phone).not.toHaveProperty('fold');
      expect(mac).toHaveProperty('fold');
      // It stands right of the Mac, and holds a text of the same name as the part: what you see is shown in both.
      expect(phone.full[0]).toBeGreaterThanOrEqual(mac.full[0] + mac.full[2]);
      expect(phone.full[0] + phone.full[2]).toBeLessThanOrEqual(MAP_WIDTH);
      const inside = note('What you see');
      expect(inside.line).toBe('alerts and asks');
      expect(inside.undecided).toBe(true);
      expect(inside).not.toHaveProperty('fold');
      expect(inside.full[0]).toBeGreaterThan(phone.full[0]);
      expect(inside.full[1]).toBeGreaterThan(phone.full[1]);
      expect(inside.full[0] + inside.full[2]).toBeLessThan(phone.full[0] + phone.full[2]);
      expect(inside.full[1] + inside.full[3]).toBeLessThan(phone.full[1] + phone.full[3]);
      expect(map.notes.some((one) => one.title === 'Your phone')).toBe(false);
      // No wire joins What you see to the phone: the app's wires are the way to the engine and the one that shows it.
      expect(map.wires.filter((one) => one.of === 'views').map((one) => one.label?.[2])).toEqual([undefined, 'shown']);
      // How it is checked has the Mac's width exactly: its left and right edges are the Mac's.
      expect(map.places.checking.full).toEqual([24, 566, 936, 48]);
      expect([mac.full[0], mac.full[0] + mac.full[2]]).toEqual([24, 960]);
    });
  });

  describe('a title in its box', () => {
    const titled = (title: string, id = 'part-c') => ({ parts: PARTS.map((part) => (part.id === id ? { ...part, title } : part)) });

    it('stops on a title wider than its box, on the full map and on the fold, with the room it needs and the room it has', () => {
      let message = '';
      try {
        buildDataFrom(planFiles(titled('A part with a title far too long for it')), { map: EXAMPLE_MAP });
      } catch (error) {
        message = (error as Error).message;
      }
      const lines = message.split('\n');
      expect(lines).toHaveLength(2);
      const [full, fold] = lines.map((line) => /^website\/src\/workshop\/map-example\.ts: the title of the part "part-c", "A part with a title far too long for it", needs a box (\d+) map units wide on the (full|folded) map, and its box there is (\d+)\. Fix: widen its box in website\/src\/workshop\/map-example\.ts, or shorten the title in plan\/parts\.json\.$/.exec(line));
      expect([full?.[2], full?.[3], fold?.[2], fold?.[3]]).toEqual(['full', '120', 'folded', '104']);
      expect(Number(full?.[1])).toBeGreaterThan(120);
      expect(Number(fold?.[1])).toBeGreaterThan(104);
    });

    it('passes a title that fits, and stops on one only the fold is too narrow for', () => {
      expect(() => buildDataFrom(planFiles(titled('Part C, longer')), { map: EXAMPLE_MAP })).not.toThrow();
      // Part D is 152 units wide on the full map and 120 on the fold, where its title is set smaller: this one fits only the first.
      let message = '';
      try {
        buildDataFrom(planFiles(titled('Part D, a bit longer', 'part-d')), { map: EXAMPLE_MAP });
      } catch (error) {
        message = (error as Error).message;
      }
      expect(message).toBe('website/src/workshop/map-example.ts: the title of the part "part-d", "Part D, a bit longer", needs a box 122 map units wide on the folded map, and its box there is 120. Fix: widen its box in website/src/workshop/map-example.ts, or shorten the title in plan/parts.json.');
    });

    it('stops on a character the width table lacks, naming it and the command that prints its width', () => {
      expect(() => buildDataFrom(planFiles(titled('Café')), { map: EXAMPLE_MAP })).toThrow('The title of the part "part-c", "Café", has "é", which the width table of website/src/workshop/map-fit.ts lacks, so the build cannot tell whether it fits its box. Fix: run `node website/scripts/font-widths.mjs website/public/fonts/ibm-plex-serif-400-normal.woff2 \'é\'` and add the width it prints for each to SERIF_WIDTHS, or use another character in plan/parts.json.');
      // The command, as the message gives it, prints that width; a character the font has no glyph for is named, and fails.
      const printed = JSON.parse(execFileSync('node', ['website/scripts/font-widths.mjs', TITLE_FACE.file, 'é'], { cwd: REPOSITORY, encoding: 'utf8' }));
      expect(printed['é']).toBe(536);
      expect(() => execFileSync('node', ['website/scripts/font-widths.mjs', TITLE_FACE.file, '漢'], { cwd: REPOSITORY, stdio: 'pipe' })).toThrow('has no glyph for "漢"');
    });

    it('stops on a part named like a property every object has, with no place, as on any part with no place', () => {
      const files = planFiles({ parts: [...PARTS, { id: 'constructor', title: 'Constructor', line: 'A part named like a property of every object.' }] });
      expect(() => buildDataFrom(files, { map: EXAMPLE_MAP })).toThrow('The map has no place for the part "constructor" of plan/parts.json. Add it to website/src/workshop/map-example.ts.');
      for (const id of ['toString', '__proto__', 'hasOwnProperty']) {
        expect(() => checkLayout({ ...plan(), parts: [...plan().parts, { id, title: id }] }, EXAMPLE_MAP)).toThrow(`The map has no place for the part "${id}" of plan/parts.json.`);
      }
    });

    it('measures a title as the page shows it: a run of white space as one space, none at the ends', () => {
      const spaced = `The${' '.repeat(25)}engine`;
      expect(shownTitle(`  ${spaced}\t\n`)).toBe('The engine');
      expect(titleEms(spaced)).toBe(titleEms('The engine'));
      // Part D's fold is 120 units wide: 25 spaces more, measured, would need about 72 more.
      expect(() => buildDataFrom(planFiles(titled(spaced, 'part-d')), { map: EXAMPLE_MAP })).not.toThrow();
      // A no-break space is not white space the page collapses.
      expect(shownTitle('The\u00a0\u00a0engine')).toBe('The\u00a0\u00a0engine');
    });

    it('stops on a title that fits its box but runs under the part drawn inside it, on the fold', () => {
      const real = layoutPlanFrom(loadDir(join(REPOSITORY, 'plan')));
      expect(() => checkLayout(real, SILO_MAP)).not.toThrow();
      const renamed = { ...real, parts: real.parts.map((part) => (part.id === 'computers' ? { ...part, title: 'Virtual computers' } : part)) };
      let message = '';
      try {
        checkLayout(renamed, SILO_MAP);
      } catch (error) {
        message = (error as Error).message;
      }
      // Computers is 216 units wide on the fold, and Agent programs begins 84 units in, across the title's line.
      expect(message).toMatch(/^website\/src\/workshop\/map\.ts: the title of the part "computers", "Virtual computers", runs under the part "agent-programs", drawn inside its box on the folded map: the title needs (\d+) map units from the box's left edge, and "agent-programs" begins 84 units in\. Fix: move "agent-programs" right or off the title's line in website\/src\/workshop\/map\.ts, or shorten the title in plan\/parts\.json\.$/);
      expect(Number(/needs (\d+)/.exec(message)?.[1])).toBeGreaterThan(84);
    });

    it('has every character of the real plan\'s titles in its width table, and the table is the font file\'s', async () => {
      const titles = (JSON.parse(readFileSync(join(REPOSITORY, 'plan/parts.json'), 'utf8')) as { title: string }[]).map((part) => part.title);
      expect(titles.flatMap((title) => unknownCharacters(title).map((char) => `${title}: ${char}`))).toEqual([]);
      const { advanceWidths } = await import('../../scripts/font-widths.mjs');
      const font = advanceWidths(readFileSync(join(REPOSITORY, 'website/public/fonts/ibm-plex-serif-400-normal.woff2')));
      expect(Object.fromEntries(Object.keys(SERIF_WIDTHS).map((char) => [char, font[char]]))).toEqual(SERIF_WIDTHS);
      const mono = advanceWidths(readFileSync(join(REPOSITORY, 'website/public/fonts/ibm-plex-mono-400-normal.woff2')));
      expect(new Set(Object.entries(mono).filter(([char]) => char >= ' ' && char <= '~').map(([, width]) => width))).toEqual(new Set([MONO_ADVANCE]));
    });

    it('fits the longest state line under every title of Silo\'s map, for two-digit counts, at both ends of the map\'s widths', () => {
      const counts = (built: number, decided: number): Counts => ({ built, building: 0, decided, undecided: 0, dropped: 0 });
      const lines = (['built', 'building', 'next', 'decided', 'open'] as PartState[]).flatMap((state) => [counts(99, 0), counts(0, 99), counts(10, 89)].map((c) => stateWords({ state, counts: c })));
      const longest = lines.reduce((a, b) => (b.length > a.length ? b : a));
      // The font is monospaced: "being built" and "99/99 built" are as wide.
      expect(new Set(lines.filter((line) => line.length === longest.length))).toEqual(new Set(['10/99 built', '99/99 built', 'being built']));
      const short = Object.entries(SILO_MAP.places).flatMap(([id, place]) => Object.values(MAP_PIXELS).map((px) => [id, px, stateNeeds(longest, place, px), place.full[2]] as const).filter(([, , need, has]) => need > has));
      expect(short).toEqual([]);
    });
  });

  for (const [name, layout] of [['Silo\'s map', SILO_MAP], ['the example', EXAMPLE_MAP]] as const) {
    it(`keeps every part of ${name} inside the map, full and folded`, () => {
      for (const [id, place] of Object.entries(layout.places)) {
        const [x, y, w, h] = place.full;
        const [cx, cy, cw, ch] = place.fold;
        // The full map's bottom edge leaves room for the focus ring, which is drawn 4 px outside the box.
        expect([id, x + w <= 1136 && y + h <= MAP_HEIGHT - 5 && cx + cw <= 1136 && cy + ch <= 180]).toEqual([id, true]);
      }
    });

    it(`moves no part of ${name} across another, or across the text that is not a part, while it folds`, () => {
      // Every box moves from its full place to its folded one in the same time with the same easing: at each step t
      // of the move it sits at full + t * (fold - full). A part inside another overlaps that one by design.
      const at = (from: Box, to: Box, t: number) => from.map((value, index) => value + t * (to[index] - value)) as Box;
      const hit = (a: Box, b: Box) => a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3];
      const boxes = [
        ...Object.entries(layout.places).filter(([, place]) => !place.inner).map(([id, place]) => [id, place.full, place.fold] as const),
        // A text with no folded place does not move: it fades out where it stands.
        ...layout.notes.flatMap((note) => (note.fold ? [[`the text ${note.title}`, note.full, note.fold] as const] : [])),
      ];
      const crossings = new Set<string>();
      for (let step = 0; step <= 100; step += 1) {
        const t = step / 100;
        boxes.forEach(([a, af, ac], index) => boxes.slice(index + 1).forEach(([b, bf, bc]) => { if (hit(at(af, ac, t), at(bf, bc, t))) crossings.add(`${a} and ${b}`); }));
      }
      expect([...crossings]).toEqual([]);
    });

    it(`keeps the parts of ${name} clear of each other, of the text that is not a part and of the zones' labels, full and folded`, () => {
      const hit = (a: Box, b: Box) => a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3];
      // A zone's label sits on its top edge, 14 units in, about 120 wide and 16 high.
      const label = (zone: Box): Box => [zone[0] + 14, zone[1] - 8, 120, 16];
      for (const view of ['full', 'fold'] as const) {
        // A part drawn inside another overlaps that one by design, and only that one.
        const parts = Object.entries(layout.places).filter(([, place]) => !place.inner).map(([id, place]) => [id, place[view]] as [string, Box]);
        const others: [string, Box][] = [
          ...layout.notes.flatMap((note) => { const box = note[view]; return box ? [[`the text ${note.title}`, box] as [string, Box]] : []; }),
          // A zone with no folded place does not move: it fades out where it stands.
          ...layout.zones.flatMap((zone) => { const box = zone[view]; return box ? [[`label of ${zone.label}`, label(box)] as [string, Box]] : []; }),
        ];
        const clashes = parts.flatMap(([id, box], index) => [...parts.slice(index + 1), ...others].filter(([, other]) => hit(box, other)).map(([name]) => `${view}: ${id} and ${name}`));
        expect(clashes).toEqual([]);
      }
    });
  }

  describe('what the stylesheet holds that the map\'s checks rely on', () => {
    const here = dirname(new URL(import.meta.url).pathname);
    const workshop = readFileSync(join(here, 'workshop.css'), 'utf8');
    // The declarations of the last one-line rule for `selector`, by property.
    const rule = (selector: string): Record<string, string> => {
      const line = workshop.split('\n').filter((text) => text.startsWith(`${selector} {`)).at(-1) ?? '';
      const body = line.slice(line.indexOf('{') + 1, line.lastIndexOf('}'));
      return Object.fromEntries(body.split(';').map((declaration) => declaration.trim()).filter(Boolean).map((declaration) => [declaration.slice(0, declaration.indexOf(':')).trim(), declaration.slice(declaration.indexOf(':') + 1).trim()]));
    };
    const size = (value: number) => `max(${TITLE_STYLE.floorPx}px, calc(${value} * var(--u)))`;

    it('sets the titles in the face the width table was made from', () => {
      const site = readFileSync(join(here, '../site.css'), 'utf8');
      expect(/--serif:\s*"([^"]+)"/.exec(site)?.[1]).toBe(TITLE_FACE.family);
      const fonts = readFileSync(join(REPOSITORY, 'website/public/fonts/fonts.css'), 'utf8');
      const faces = fonts.split('@font-face').filter((face) => face.includes(`'${TITLE_FACE.family}'`) && /font-style:\s*normal/.test(face) && /font-weight:\s*400/.test(face));
      expect(faces.map((face) => /url\(\.\/([^)]+)\)/.exec(face)?.[1])).toEqual([TITLE_FACE.file.split('/').at(-1)]);
      // The title's rule sets weight and style back to normal, and no letter spacing.
      expect(rule('.ws-p-ttl').font).toBe(`${size(TITLE_STYLE.full.size)}/${TITLE_STYLE.lineHeight} var(--serif)`);
      expect(Object.keys(rule('.ws-p-ttl'))).not.toContain('letter-spacing');
    });

    it('gives the titles the offsets, sizes and tops the width check uses, full and folded', () => {
      expect(rule('.ws-p-ttl')).toMatchObject({ left: `calc(${TITLE_STYLE.full.left} * var(--u))`, top: `calc(var(--ty, ${TITLE_STYLE.top}) * var(--u))` });
      expect(rule('.ws-part.inner .ws-p-ttl')).toEqual({ left: `calc(${TITLE_STYLE.inner.left} * var(--u))`, 'font-size': size(TITLE_STYLE.inner.size) });
      expect(rule('.ws-stage.fold .ws-p-ttl')).toMatchObject({ left: `calc(${TITLE_STYLE.fold.left} * var(--u))`, top: `calc((var(--ch) * var(--u) - 2px - ${TITLE_STYLE.lineHeight}em) / 2)`, 'font-size': size(TITLE_STYLE.fold.size) });
      expect(rule('.ws-stage.fold .ws-part.inner .ws-p-ttl')).toEqual({ left: `calc(${TITLE_STYLE.foldInner.left} * var(--u))`, 'font-size': size(TITLE_STYLE.foldInner.size) });
      expect(rule('.ws-stage.fold .ws-part.tall .ws-p-ttl')).toMatchObject({ top: `calc(${TITLE_STYLE.foldTallTop} * var(--u))` });
    });

    it('keeps a part drawn inside another above it, the other focused or not, and its own focus ring above its neighbours', () => {
      const z = (selector: string) => Number(rule(selector)['z-index']);
      expect(z('.ws .ws-part')).toBeLessThan(z('.ws .ws-part:focus-visible'));
      expect(z('.ws .ws-part.inner')).toBeGreaterThan(z('.ws .ws-part:focus-visible'));
      expect(z('.ws .ws-part.inner:focus-visible')).toBeGreaterThan(z('.ws .ws-part.inner'));
    });
  });

  it('gives the stage in the stylesheet the height of the full map, which clips what goes past it', () => {
    const css = readFileSync(join(dirname(new URL(import.meta.url).pathname), 'workshop.css'), 'utf8');
    expect(css).toContain(`.ws-stage { --u: calc(100cqw / 1136); position: relative; height: calc(${MAP_HEIGHT} * var(--u));`);
  });
});

describe('the films', () => {
  const entry = (fields: Record<string, unknown>) => JSON.stringify({ name: 'Fork', what: 'Copy a computer.', ...fields });

  it('come in the order films/README.md gives, and the real repository\'s are all there, each with its title, line and entries', () => {
    expect(filmOrder('# Films\n\n## The films, in the order they are watched\n\n| Film | Kind |\n|---|---|\n| `b` | x |\n| `a` | y |\n\n## Next\n| `c` | z |\n')).toEqual(['b', 'a']);
    const films = readFilms(REPOSITORY, false);
    expect(films.map((film) => film.id)).toEqual(filmOrder(readFileSync(join(REPOSITORY, 'films', 'README.md'), 'utf8')));
    expect(films).toHaveLength(5);
    for (const film of films) {
      expect(film.title).not.toBe('');
      expect(film.shows.length).toBeGreaterThan(0);
    }
  });

  it('are behind on an entry whose printed part changed after them, or that was dropped or replaced; never for a take alone', () => {
    const set = { in: 'words', id: 'twin', word: 'twin', means: 'A copy.' };
    expect(printedChanged(entry({}), entry({ suggest: { verdict: 'take' } }))).toBe(false);
    expect(printedChanged(entry({}), entry({ what: 'Copy a whole computer.' }))).toBe(true);
    expect(printedChanged(entry({ name: 'Fork' }), entry({ name: 'Forks' }))).toBe(true);
    expect(printedChanged(entry({ asks: 'A?' }), entry({ asks: 'B?' }))).toBe(true);
    expect(printedChanged(entry({ answer: 'Yes.' }), entry({ answer: 'No.' }))).toBe(true);
    expect(printedChanged(entry({ sets: [set] }), entry({ sets: [{ ...set, means: 'Another copy.' }] }))).toBe(true);
    expect(printedChanged(entry({ sets: [] }), entry({ sets: [set] }))).toBe(true);
    expect(printedChanged(entry({ removes: ['a'] }), entry({ removes: ['b'] }))).toBe(true);
    expect(printedChanged(entry({ keeps: ['a'] }), entry({ keeps: ['a', 'b'] }))).toBe(true);
    expect(printedChanged(entry({ surfaces: { cli: 'silo fork' } }), entry({ surfaces: { cli: 'silo copy' } }))).toBe(true);
    expect(printedChanged(entry({}), entry({ verdict: 'drop' }))).toBe(true);
    expect(printedChanged(entry({ verdict: 'take' }), entry({ verdict: 'take', replaced_by: 'other' }))).toBe(true);
    expect(printedChanged(entry({}), null)).toBe(true);
    expect(printedChanged(null, entry({}))).toBe(false);
    // A take writes the ids of `sets` as `adds` and deletes `sets` and `removes`: the same statements stand.
    expect(printedChanged(entry({ sets: [set], removes: ['scrap'] }), entry({ adds: ['twin'], verdict: 'take' }))).toBe(false);
    expect(printedChanged(entry({ sets: [set] }), entry({ adds: ['twin', 'more'], verdict: 'take' }))).toBe(true);
    // A word of `sets` has no id: it is named by its word, as the lists name it.
    expect(printedChanged(entry({ sets: [{ in: 'words', word: 'Pause point', means: 'A stop.' }] }), entry({ adds: ['pause-point'], verdict: 'take' }))).toBe(false);
  });

  it('find each entry at the film\'s commit by its id, two files with the same bytes included, wherever they moved since', () => {
    const root = mkdtempSync(join(tmpdir(), 'workshop-films-'));
    const git = (args: string[]) => execFileSync('git', ['-C', root, ...args], { env: { ...process.env, GIT_AUTHOR_NAME: 't', GIT_AUTHOR_EMAIL: 't@t', GIT_COMMITTER_NAME: 't', GIT_COMMITTER_EMAIL: 't@t' }, stdio: 'ignore' });
    const write = (path: string, text: string) => {
      mkdirSync(dirname(join(root, path)), { recursive: true });
      writeFileSync(join(root, path), text);
    };
    try {
      git(['init', '-q', '-b', 'main']);
      write('plan/p/decisions/twin-a.json', entry({ asks: 'Same?' }));
      write('plan/p/decisions/twin-b.json', entry({ asks: 'Same?' }));
      write('films/one/film.json', JSON.stringify({ id: 'one', title: 'One', line: 'L.', shows: ['twin-a', 'twin-b'] }));
      git(['add', '.']);
      git(['commit', '-q', '-m', 'the film']);
      mkdirSync(join(root, 'plan/q/decisions'), { recursive: true });
      git(['mv', 'plan/p/decisions/twin-a.json', 'plan/q/decisions/twin-a.json']);
      git(['mv', 'plan/p/decisions/twin-b.json', 'plan/q/decisions/twin-b.json']);
      git(['commit', '-q', '-m', 'moved']);
      expect(readFilms(root, false)[0].behind).toEqual([]);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  it('compare what an entry says now with what it said at the film\'s last commit: a change undone does not count, a moved file is the same entry, a new one counts', () => {
    const root = mkdtempSync(join(tmpdir(), 'workshop-films-'));
    const git = (args: string[]) => execFileSync('git', ['-C', root, ...args], { env: { ...process.env, GIT_AUTHOR_NAME: 't', GIT_AUTHOR_EMAIL: 't@t', GIT_COMMITTER_NAME: 't', GIT_COMMITTER_EMAIL: 't@t' }, stdio: 'ignore' });
    const write = (path: string, text: string) => {
      mkdirSync(dirname(join(root, path)), { recursive: true });
      writeFileSync(join(root, path), text);
    };
    try {
      git(['init', '-q', '-b', 'main']);
      for (const id of ['undone', 'changed', 'moved', 'taken']) write(`plan/p/features/${id}.json`, entry({ name: id }));
      // A film shows statements too: `least-power` is a principle, an entry at neither end.
      write('films/one/film.json', JSON.stringify({ id: 'one', title: 'One', line: 'L.', shows: ['undone', 'changed', 'moved', 'taken', 'new', 'least-power'] }));
      git(['add', '.']);
      git(['commit', '-q', '-m', 'the film']);
      // On a side branch, a change made and undone; merged.
      git(['checkout', '-q', '-b', 'side']);
      write('plan/p/features/undone.json', entry({ name: 'undone, for a while' }));
      git(['commit', '-q', '-am', 'change']);
      write('plan/p/features/undone.json', entry({ name: 'undone' }));
      git(['commit', '-q', '-am', 'undo']);
      git(['checkout', '-q', 'main']);
      git(['merge', '-q', '--no-ff', '-m', 'merge', 'side']);
      write('plan/p/features/changed.json', entry({ name: 'changed since' }));
      mkdirSync(join(root, 'plan/q/features'), { recursive: true });
      git(['mv', 'plan/p/features/moved.json', 'plan/q/features/moved.json']);
      write('plan/p/features/taken.json', entry({ name: 'taken', verdict: 'take' }));
      write('plan/p/features/new.json', entry({ name: 'new' }));
      git(['add', '.']);
      git(['commit', '-q', '-am', 'after']);
      expect(readFilms(root, false)).toEqual([{ id: 'one', title: 'One', line: 'L.', shows: ['undone', 'changed', 'moved', 'taken', 'new', 'least-power'], behind: ['changed', 'new'] }]);
      expect(readFilms(root, true)[0].behind).toEqual([]);
      // A commit git cannot read says nothing.
      expect(readFilms(root, false, () => 'f'.repeat(40))[0].behind).toEqual([]);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });


});
