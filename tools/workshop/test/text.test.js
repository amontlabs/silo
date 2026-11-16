import assert from 'node:assert/strict';
import { test } from 'node:test';
import { appendReference, decisionTitle, listItems, referencedIssues } from '../src/decisions.js';
import { setField } from '../src/jsonl.js';

test('setField replaces a value in place or appends the field, in the line’s own style', () => {
  assert.equal(setField('{"a": 1, "verdict": null, "z": [1, {"b": "}"}]}', 'verdict', 'take'), '{"a": 1, "verdict": "take", "z": [1, {"b": "}"}]}');
  assert.equal(setField('{"a": 1, "b": "x"}', 'issue', 12), '{"a": 1, "b": "x", "issue": 12}');
  assert.equal(setField('{"a":1,"b":"x"}', 'issue', 12), '{"a":1,"b":"x","issue":12}');
  assert.equal(setField('{"a": "\\"é\\u00e9\\"", "issue": 3}', 'issue', 4), '{"a": "\\"é\\u00e9\\"", "issue": 4}');
  assert.equal(setField('{}', 'issue', 1), '{"issue":1}');
  assert.equal(setField('{"what": "a, \\"verdict\\": x", "verdict": "take"}', 'verdict', 'drop'), '{"what": "a, \\"verdict\\": x", "verdict": "drop"}');
});

test('decision items: references, continuation lines, CRLF and fences', () => {
  const lines = ['# D', '', '- one (#4)', '- two', '  continued', '* three\r', '```', '- not an item', '```', '-not an item', '  - nested'];
  const items = listItems(lines);
  assert.deepEqual(items.map((i) => [i.start, i.end, i.text, i.issue]), [
    [2, 2, 'one', 4],
    [3, 4, 'two continued', null],
    [5, 5, 'three', null],
  ]);
  assert.deepEqual(appendReference(lines, items[1], 9).slice(3, 5), ['- two', '  continued (#9)']);
  assert.equal(appendReference(lines, items[2], 10)[5], '* three (#10)\r');
  assert.deepEqual([...referencedIssues('a (#1) b (#22)\n')], [1, 22]);
});

test('decision titles are cut at 100 characters on a word', () => {
  assert.equal(decisionTitle('short'), 'short');
  const long = `2026-11-16 · owner · ${'word '.repeat(40)}`.trim();
  const title = decisionTitle(long);
  assert.ok(title.length <= 100);
  assert.ok(title.endsWith('word…'));
  assert.ok(long.startsWith(title.slice(0, -1)));
  assert.equal(decisionTitle('x'.repeat(150)).length, 100);
});
