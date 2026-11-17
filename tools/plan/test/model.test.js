import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import {
  areaGroups, areaStates, canonicalFeature, canonicalOrder, canonicalText, changeComment, compareLink, countRows, entryMarker, featureFileAt, featureStatus, isDate, isReady,
  labelVerdict, readChange, readEntryMarker, readSyncedMarker, stateWords, statusLine, syncedMarker, waitingFor, wantedIssue, withoutMarkers,
} from '../model/index.js';

const VERDICT_CHOICES = [undefined, 'take', 'later', 'drop'];
const LIVE_CHOICES = [null];
for (const verdict of [null, 'take', 'later', 'drop']) for (const building of [false, true]) LIVE_CHOICES.push({ verdict, building });

/** The table of the spec, written out: the verdict that counts is the issue's label, else the file's. */
function expected(built, fileVerdict, live) {
  if (built) return 'built';
  const verdict = live?.verdict ?? fileVerdict ?? null;
  if (verdict === 'drop') return 'dropped';
  if (verdict === 'take') return live?.building ? 'building' : 'planned';
  if (verdict === 'later') return 'later';
  return 'undecided';
}

test('status: every combination of built, the file verdict, the issue verdict and the building label', () => {
  let cases = 0;
  for (const built of [undefined, '2026-11-01']) {
    for (const verdict of VERDICT_CHOICES) {
      for (const live of LIVE_CHOICES) {
        const feature = { ...(verdict ? { verdict } : {}), ...(built ? { built } : {}) };
        assert.equal(featureStatus(feature, live), expected(built, verdict, live), JSON.stringify({ feature, live }));
        cases += 1;
      }
    }
  }
  assert.equal(cases, 2 * 4 * 9);
  // A few rows spelled out.
  assert.equal(featureStatus({ verdict: 'take' }, { verdict: 'drop', building: true }), 'dropped');
  assert.equal(featureStatus({ verdict: 'drop' }, { verdict: null, building: true }), 'dropped');
  assert.equal(featureStatus({}, { verdict: null, building: true }), 'undecided');
  assert.equal(featureStatus({ verdict: 'take' }, { verdict: null, building: true }), 'building');
});

test('ready: planned and every need built', () => {
  const statuses = { a: 'built', b: 'planned' };
  const statusOf = (id) => statuses[id];
  assert.equal(isReady({ needs: ['a'] }, 'planned', statusOf), true);
  assert.equal(isReady({ needs: [] }, 'planned', statusOf), true);
  assert.equal(isReady({ needs: ['a', 'b'] }, 'planned', statusOf), false);
  assert.equal(isReady({ needs: ['a'] }, 'building', statusOf), false);
  assert.deepEqual(waitingFor({ needs: ['a', 'b'] }, statusOf), ['b']);
});

test('area counts and states', () => {
  const rows = (...list) => list.map(([status, position]) => ({ status, position }));
  const group = (...list) => ({ rows: rows(...list), counts: countRows(rows(...list)) });
  assert.deepEqual(countRows(rows(['built', 1], ['undecided', 2], ['dropped', 3])), { built: 1, building: 0, planned: 0, later: 0, undecided: 1, dropped: 1 });
  assert.deepEqual(
    areaStates([group(['building', 1], ['planned', 2]), group(['planned', 5]), group(['planned', 3]), group(['built', 4]), group(['later', 6]), group(['built', 7], ['planned', 9])]),
    ['now', 'planned', 'next', 'done', 'open', 'planned'],
  );
  assert.deepEqual(areaStates([group(['undecided', 1])]), ['open']);
  assert.equal(stateWords({ state: 'now', counts: { built: 1, building: 1, planned: 2 } }), 'now · 1/4');
  assert.equal(stateWords({ state: 'planned', counts: { built: 0, building: 0, planned: 3 } }), '3 planned');
  assert.equal(stateWords({ state: 'planned', counts: { built: 2, building: 0, planned: 3 } }), '2/5 built');
  assert.equal(stateWords({ state: 'done', counts: {} }), 'done');
  assert.equal(stateWords({ state: 'open', counts: {} }), '');
});

test('area groups from a plan, and the status line', () => {
  const plan = {
    areas: [{ id: 'engine', title: 'Engine', line: '' }, { id: 'forks', title: 'Forks', line: '' }],
    stages: [{ id: 's1', features: ['a', 'b'] }, { id: 's2', features: ['c'] }, { id: 's3', features: [] }, { id: 's4', features: [] }, { id: 's5', features: ['d'] }],
    features: { a: { id: 'a', area: 'engine', stage: 's1', position: 1 }, b: { id: 'b', area: 'forks', stage: 's1', position: 2 }, c: { id: 'c', area: 'forks', stage: 's2', position: 3 }, d: { id: 'd', area: 'engine', stage: 's5', position: 4 } },
  };
  const statuses = { a: 'planned', b: 'planned', c: 'undecided', d: 'undecided' };
  const groups = areaGroups(plan, (id) => statuses[id]);
  assert.deepEqual(groups.map((g) => [g.area.id, g.number, g.rows.map((r) => r.id), g.state, g.first]), [['engine', 1, ['a', 'd'], 'next', true], ['forks', 2, ['b', 'c'], 'planned', true]]);
  assert.equal(statusLine(groups), 'Starting with the Engine.');
  statuses.a = 'building';
  assert.equal(statusLine(areaGroups(plan, (id) => statuses[id])), 'Building Engine. Forks is next.');
  statuses.a = 'built';
  assert.equal(statusLine(areaGroups(plan, (id) => statuses[id])), 'Forks is next.');
  statuses.b = 'undecided';
  assert.equal(statusLine(areaGroups(plan, (id) => statuses[id])), '');
});

test('entry and synced markers', () => {
  const sha = 'a'.repeat(40);
  const body = `Text\n\n${syncedMarker(sha)}\n${entryMarker('fork')}`;
  assert.equal(readEntryMarker(body), 'fork');
  assert.equal(readEntryMarker(`${body}\nedited after`), null);
  assert.equal(readEntryMarker('Text'), null);
  assert.equal(readSyncedMarker(body), sha);
  assert.equal(readSyncedMarker('none'), null);
  assert.equal(withoutMarkers(body), 'Text');
  assert.throws(() => entryMarker('Bad Id'));
  assert.throws(() => syncedMarker('abc'));
  assert.equal(labelVerdict('verdict:take'), 'take');
  assert.equal(labelVerdict('verdict:refine'), null);
  assert.equal(labelVerdict('workshop:entry'), null);
});

test('history comment: written and read back, nothing from the plan in it', () => {
  const change = { commit: 'b'.repeat(40), before: 'c'.repeat(40), fields: ['what', 'verdict', 'notes'], verdict: 'take' };
  const text = changeComment(change);
  assert.equal(text.split('\n')[0], 'Updated in bbbbbbb: what, verdict, notes');
  assert.equal(text.split('\n')[1], `https://github.com/amontlabs/silo/compare/${'c'.repeat(40)}...${'b'.repeat(40)}`);
  assert.deepEqual(readChange(text), change);
  assert.deepEqual(readChange(changeComment({ ...change, fields: ['verdict'], verdict: null })), { ...change, fields: ['verdict'], verdict: null });
  assert.deepEqual(readChange(changeComment({ commit: change.commit, before: change.before, fields: ['built'], built: '2026-12-04' })).built, '2026-12-04');
  assert.equal(readChange('a plain comment'), null);
  assert.equal(readChange('<!-- silo-change: {"commit":"x"} -->'), null);
  assert.equal(readChange(`<!-- silo-change: ${JSON.stringify({ ...change, fields: ['title'] })} -->`), null);
  assert.equal(readChange(`<!-- silo-change: ${JSON.stringify({ ...change, verdict: 'refine' })} -->`), null);
  assert.throws(() => changeComment({ ...change, fields: [] }));
});

test('the links of a change: the comparison on GitHub and a feature file at a commit', () => {
  const change = { commit: 'b'.repeat(40), before: 'c'.repeat(40) };
  assert.equal(compareLink(change, { repo: 'o/r' }), `https://github.com/o/r/compare/${'c'.repeat(40)}...${'b'.repeat(40)}`);
  assert.equal(featureFileAt('b'.repeat(40), 'fork'), `https://raw.githubusercontent.com/amontlabs/silo/${'b'.repeat(40)}/plan/features/fork.json`);
  assert.throws(() => compareLink({ ...change, before: '../x' }));
  assert.throws(() => featureFileAt('b'.repeat(40), '../fork'));
  assert.throws(() => featureFileAt('main', 'fork'));
});

test('wanted issue: title, body and state', () => {
  const synced = 'd'.repeat(40);
  const open = wantedIssue({ id: 'fork', name: 'Fork', what: 'Copy.', area: 'forks', suggested_in: 12 }, { synced, site: 'https://x.test' });
  assert.equal(open.title, 'Fork');
  assert.equal(open.body, `Copy.\n\nOn the Workshop: https://x.test/workshop#forks/fork\n\nSuggested in #12\n\n<!-- silo-synced: ${synced} -->\n<!-- silo-entry: fork -->`);
  assert.equal(readEntryMarker(open.body), 'fork');
  assert.deepEqual([open.state, open.stateReason], ['open', null]);
  const built = wantedIssue({ id: 'fork', name: 'Fork', what: 'Copy.', area: 'forks', verdict: 'take', built: '2026-12-01' }, { synced });
  assert.deepEqual([built.state, built.stateReason], ['closed', 'completed']);
  assert.ok(!built.body.includes('Suggested in'));
  const dropped = wantedIssue({ id: 'old', name: 'Old', what: 'Gone.', verdict: 'drop' }, { synced });
  assert.deepEqual([dropped.state, dropped.stateReason], ['closed', 'not_planned']);
  assert.ok(dropped.body.includes('/workshop#left/old'));
  assert.equal(wantedIssue({ id: 'x', name: 'X', what: 'W', verdict: 'later' }, { synced }).state, 'open');
});

test('canonical form: key order, two spaces, final newline', () => {
  const feature = { built: '2026-12-01', verdict: 'take', needs: [], what: 'W', name: 'N', suggest: { why: 'Y', verdict: 'take' }, surfaces: { cli: 'c', app: 'a' } };
  assert.deepEqual(Object.keys(canonicalFeature(feature)), ['name', 'what', 'needs', 'surfaces', 'suggest', 'verdict', 'built']);
  assert.deepEqual(Object.keys(canonicalFeature(feature).surfaces), ['app', 'cli']);
  assert.deepEqual(Object.keys(canonicalFeature(feature).suggest), ['verdict', 'why']);
  assert.deepEqual(Object.keys(canonicalOrder({ stages: [{ features: [], line: 'l', title: 't', id: 'i' }] }).stages[0]), ['id', 'title', 'line', 'features']);
  assert.equal(canonicalText({ a: [1] }), '{\n  "a": [\n    1\n  ]\n}\n');
  // Unknown keys are kept, after the known ones, for the check to report.
  assert.deepEqual(Object.keys(canonicalFeature({ extra: 1, name: 'N' })), ['name', 'extra']);
});

test('dates', () => {
  assert.equal(isDate('2026-02-28'), true);
  assert.equal(isDate('2026-02-30'), false);
  assert.equal(isDate('2026-2-3'), false);
  assert.equal(isDate(20261201), false);
});

test('the model imports nothing outside its folder and no node: module', () => {
  const dir = join(import.meta.dirname, '..', 'model');
  const files = readdirSync(dir).filter((name) => name.endsWith('.js'));
  assert.ok(files.length >= 5);
  for (const name of files) {
    // Comments may name types with import('./x.js'); only code counts.
    const text = readFileSync(join(dir, name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '').replace(/(^|[^:])\/\/.*$/gm, '$1');
    assert.doesNotMatch(text, /\brequire\s*\(/, `${name} uses require`);
    assert.doesNotMatch(text, /\bimport\s*\(/, `${name} imports dynamically`);
    assert.doesNotMatch(text, /\bprocess\./, `${name} reads process`);
    for (const match of text.matchAll(/^\s*(?:import|export)\b[^'"]*?\bfrom\s*['"]([^'"]+)['"]|^\s*import\s*['"]([^'"]+)['"]/gm)) {
      const spec = match[1] ?? match[2];
      assert.match(spec, /^\.\/[a-z-]+\.js$/, `${name} imports ${spec}`);
    }
  }
});
