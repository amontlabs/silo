import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import {
  changeComment, compareLink, entryMarker, featureFileAt, isDate, labelVerdict, labelVerdicts, readBuildingComment, readChange, readEntryMarker, readSyncedMarker,
  syncedMarker, wantedIssue, withoutMarkers, withSyncedMarker,
} from '../model/index.js';

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

test('labels, claims and dates as the issues already on GitHub hold them', () => {
  assert.deepEqual(labelVerdicts(['verdict:drop', 'Verdict:Take', 'verdict:refine', 'workshop:entry']), ['take', 'drop']);
  assert.equal(readBuildingComment('Being built on mig/workshop.'), 'mig/workshop');
  assert.equal(readBuildingComment('Being built on a branch with spaces.'), null);
  assert.equal(withSyncedMarker(`Text\n\n${entryMarker('fork')}`, 'e'.repeat(40)), `Text\n\n<!-- silo-synced: ${'e'.repeat(40)} -->\n${entryMarker('fork')}`);
  assert.equal(isDate('2026-02-28'), true);
  assert.equal(isDate('2026-02-30'), false);
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
