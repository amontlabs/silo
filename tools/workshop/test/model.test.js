import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import * as model from '../model/index.js';
import { entryMarker, readEntryMarker, withoutMarkers } from '../model/index.js';

test('the entry marker', () => {
  const body = `Text\n\n${entryMarker('fork')}`;
  assert.equal(readEntryMarker(body), 'fork');
  assert.equal(readEntryMarker(`${body}\nedited after`), null);
  assert.equal(readEntryMarker('Text'), null);
  assert.equal(withoutMarkers(body), 'Text');
  assert.throws(() => entryMarker('Bad Id'));
});

test('the model holds no history comment', () => {
  for (const name of ['changeComment', 'readChange', 'compareLink', 'featureFileAt', 'CHANGE_FIELDS', 'DEFAULT_RAW', 'VERDICTS', 'isDate']) assert.equal(name in model, false, name);
});

test('the model imports nothing outside its folder and no node: module', () => {
  const dir = join(import.meta.dirname, '..', 'model');
  const files = readdirSync(dir).filter((name) => name.endsWith('.js'));
  assert.ok(files.length >= 3);
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
