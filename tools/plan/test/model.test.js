import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import * as model from '../model/index.js';
import {
  CHECKS, DECISION_KEYS, DECISION_STATUSES, ENTRY_KINDS, FEATURE_KEYS, FEATURE_STATUSES, ID_PATTERN, PROPERTY_KINDS, STATEMENT_KINDS, VERDICTS,
  canonicalDecision, canonicalFeature, canonicalOrder, canonicalPart, canonicalProperty, canonicalSetItem, canonicalText, canonicalWord,
  entryStatus, isDate, isDone, listKind, waitingFor, wordId,
} from '../model/index.js';
import { basePlan, canonicalFile } from './helpers.js';

test('the names of the model', () => {
  assert.deepEqual(VERDICTS, ['take', 'drop']);
  assert.deepEqual(ENTRY_KINDS, ['decision', 'feature']);
  assert.deepEqual(STATEMENT_KINDS, ['word', 'principle', 'property']);
  assert.deepEqual(PROPERTY_KINDS, ['safety', 'liveness', 'measured']);
  assert.deepEqual(CHECKS, ['types', 'model', 'simulator', 'hostile test', 'live check', 'benchmark', 'test', 'assertion']);
  for (const id of ['a', '0', 'linux-vm-engine', 'a'.repeat(63)]) assert.match(id, ID_PATTERN);
  for (const id of ['', '-a', 'A', 'a_b', 'a b', 'a'.repeat(64), 'a/b']) assert.doesNotMatch(id, ID_PATTERN);
  // Nothing of the old model or of GitHub is left.
  for (const name of ['GONE_STAGE', 'STARTER_STAGES', 'LABELS', 'DEFAULT_REPO', 'DEFAULT_SITE', 'PLAN_FILES', 'AREA_KEYS', 'featureStatus', 'canonicalAreas', 'entryMarker']) {
    assert.equal(name in model, false, name);
  }
});

test("a word's id: lower case, a hyphen for each space", () => {
  assert.equal(wordId('computer'), 'computer');
  assert.equal(wordId('Memory snapshot'), 'memory-snapshot');
  assert.equal(wordId('VM engine room'), 'vm-engine-room');
});

test('the list a sets item goes to', () => {
  assert.equal(listKind('words'), 'word');
  assert.equal(listKind('principles'), 'principle');
  assert.equal(listKind('properties'), 'property');
  assert.equal(listKind('record/properties'), 'property');
  for (const list of ['record/words', 'Record/properties', 'a/b/properties', '/properties', 'decisions', undefined, 3]) assert.equal(listKind(list), null, String(list));
});

const DECISION_CASES = [
  [{ verdict: 'drop', answer: 'A' }, 'dropped'],
  [{ verdict: 'take', answer: 'A' }, 'taken'],
  [{ verdict: 'take' }, 'taken'],
  [{ answer: 'A' }, 'answered'],
  [{ answer: '' }, 'answered'],
  [{}, 'open'],
];

test('the status of a decision, first match wins', () => {
  for (const [keys, status] of DECISION_CASES) assert.equal(entryStatus({ kind: 'decision', asks: 'Q', ...keys }, {}), status, JSON.stringify(keys));
  assert.deepEqual(DECISION_STATUSES, ['dropped', 'taken', 'answered', 'open']);
});

test('a decision that puts forward only statements is answered: a sets stands for its answer, an empty one does not', () => {
  const sets = [{ in: 'words', word: 'fork', means: 'A copy of a computer.' }];
  assert.equal(entryStatus({ kind: 'decision', asks: 'Q', sets }, {}), 'answered');
  assert.equal(entryStatus({ kind: 'decision', asks: 'Q', sets: [] }, {}), 'open');
  assert.equal(entryStatus({ kind: 'decision', asks: 'Q', sets, verdict: 'drop' }, {}), 'dropped');
});

/** The table of the contract, written out for a feature. */
function expectedFeature(built, verdict, needsDone) {
  if (built) return 'built';
  if (verdict === 'drop') return 'dropped';
  if (verdict === 'take') return needsDone ? 'ready' : 'taken';
  return 'undecided';
}

test('the status of a feature: every combination of built, the verdict and the needs', () => {
  const entries = {
    'taken-decision': { kind: 'decision', verdict: 'take' },
    'answered-decision': { kind: 'decision', answer: 'A' },
    'built-feature': { kind: 'feature', verdict: 'take', built: '2026-11-01' },
    'taken-feature': { kind: 'feature', verdict: 'take' },
  };
  const needsChoices = [[[], true], [['taken-decision'], true], [['taken-decision', 'built-feature'], true], [['answered-decision'], false], [['taken-feature'], false], [['missing'], false], [['constructor'], false], [['built-feature', 'taken-feature'], false]];
  let cases = 0;
  for (const built of [undefined, '2026-11-02']) {
    for (const verdict of [undefined, 'take', 'drop']) {
      for (const [needs, done] of needsChoices) {
        const feature = { kind: 'feature', name: 'N', what: 'W', needs, ...(verdict ? { verdict } : {}), ...(built ? { built } : {}) };
        assert.equal(entryStatus(feature, entries), expectedFeature(built, verdict, done), JSON.stringify(feature));
        cases += 1;
      }
    }
  }
  assert.equal(cases, 2 * 3 * needsChoices.length);
  assert.equal(entryStatus({ kind: 'feature', verdict: 'take' }, entries), 'ready');
  assert.deepEqual(FEATURE_STATUSES, ['built', 'dropped', 'ready', 'taken', 'undecided']);
});

test('done and what an entry waits for', () => {
  assert.equal(isDone({ kind: 'decision', verdict: 'take' }), true);
  assert.equal(isDone({ kind: 'decision', answer: 'A' }), false);
  assert.equal(isDone({ kind: 'feature', verdict: 'take' }), false);
  assert.equal(isDone({ kind: 'feature', verdict: 'take', built: '2026-11-01' }), true);
  assert.equal(isDone(undefined), false);
  const entries = { a: { kind: 'decision', verdict: 'take' }, b: { kind: 'feature', verdict: 'take' } };
  assert.deepEqual(waitingFor({ kind: 'feature', needs: ['a', 'b', 'c'] }, entries), ['b', 'c']);
  assert.deepEqual(waitingFor({ kind: 'decision' }, entries), []);
});

test('canonical form: key order of each kind, two spaces, final newline', () => {
  assert.deepEqual(Object.keys(canonicalWord({ not: ['w'], means: 'M', word: 'W' })), ['word', 'means', 'not']);
  assert.deepEqual(Object.keys(canonicalProperty({ at: 'f', check: 'types', from: 'p', kind: 'safety', says: 'S', id: 'i' })), ['id', 'says', 'kind', 'from', 'check', 'at']);
  assert.deepEqual(Object.keys(canonicalPart({ line: 'l', title: 't', id: 'i' })), ['id', 'title', 'line']);
  const decision = canonicalDecision({
    verdict: 'drop', suggest: { why: 'Y', verdict: 'drop' }, removes: ['x'], sets: [{ why: 'w', id: 'p', in: 'principles', asks: 'a', says: 's', known_as: 'k' }], answer: 'A', asks: 'Q',
  });
  assert.deepEqual(Object.keys(decision), ['asks', 'answer', 'sets', 'removes', 'suggest', 'verdict']);
  assert.deepEqual(Object.keys(decision.sets[0]), ['in', 'id', 'says', 'asks', 'known_as', 'why']);
  assert.deepEqual(Object.keys(decision.suggest), ['verdict', 'why']);
  const feature = canonicalFeature({ built: '2026-12-01', verdict: 'take', keeps: ['k'], adds: ['a'], needs: [], what: 'W', name: 'N', surfaces: { cli: 'c', app: 'a' } });
  assert.deepEqual(Object.keys(feature), ['name', 'what', 'needs', 'adds', 'keeps', 'surfaces', 'verdict', 'built']);
  assert.deepEqual(Object.keys(feature.surfaces), ['app', 'cli']);
  assert.deepEqual(Object.keys(canonicalOrder({ stages: [{ entries: [], line: 'l', title: 't', id: 'i' }] }).stages[0]), ['id', 'title', 'line', 'entries']);
  assert.equal(canonicalText({ a: [1] }), '{\n  "a": [\n    1\n  ]\n}\n');
  assert.deepEqual(DECISION_KEYS.slice(0, 2), ['asks', 'answer']);
  assert.deepEqual(FEATURE_KEYS.at(-1), 'built');
});

test('canonical form of a sets item: in, then the keys of the kind its list holds', () => {
  assert.deepEqual(Object.keys(canonicalSetItem({ means: 'M', word: 'W', in: 'words' })), ['in', 'word', 'means']);
  assert.deepEqual(Object.keys(canonicalSetItem({ check: 'types', kind: 'safety', says: 'S', id: 'i', in: 'record/properties' })), ['in', 'id', 'says', 'kind', 'check']);
  // A list that is not one: `in` first, the rest as it was, for the check to report.
  assert.deepEqual(Object.keys(canonicalSetItem({ b: 1, a: 2, in: 'nowhere' })), ['in', 'b', 'a']);
});

test('canonical form keeps unknown keys after the known ones, and leaves what is not an object', () => {
  assert.deepEqual(Object.keys(canonicalFeature({ extra: 1, name: 'N' })), ['name', 'extra']);
  assert.deepEqual(canonicalDecision([1]), [1]);
  assert.equal(canonicalOrder(null), null);
  assert.deepEqual(canonicalDecision({ asks: 'Q', sets: 'x', suggest: 3 }), { asks: 'Q', sets: 'x', suggest: 3 });
});

test('every JSON file of the sample tree is in canonical form', () => {
  const files = basePlan();
  let count = 0;
  for (const [name, value] of Object.entries(files)) {
    if (!name.endsWith('.json')) continue;
    const text = readFileSync(join(import.meta.dirname, 'fixtures', 'sample', name), 'utf8');
    assert.equal(canonicalText(canonicalFile(name)(value)), text, name);
    count += 1;
  }
  assert.equal(count, 13);
});

test('dates', () => {
  assert.equal(isDate('2026-02-28'), true);
  assert.equal(isDate('2026-02-30'), false);
  assert.equal(isDate('2026-2-3'), false);
  assert.equal(isDate(20261201), false);
  // Every real calendar day, whatever its year; a year below 100 is not read as 19xx.
  assert.equal(isDate('0099-01-01'), true);
  assert.equal(isDate('0001-12-31'), true);
  assert.equal(isDate('0004-02-29'), true);
  assert.equal(isDate('0001-02-29'), false);
  assert.equal(isDate('0099-02-29'), false);
  assert.equal(isDate('9999-12-31'), true);
});

test('the model imports nothing outside its folder and no node: module', () => {
  const dir = join(import.meta.dirname, '..', 'model');
  const files = readdirSync(dir).filter((name) => name.endsWith('.js'));
  assert.ok(files.length >= 4);
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
