// The motion sheet's checks of what a move saw, without a browser.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { unseen } from './motion-sheet.mjs';

const selectors = { opened: '.wa-row', more: { selector: '.ws-more', text: 'x' } };
const box = [0, 10, 100, 36, 1];

test('a move that collected no frame of the screen fails', () => {
  assert.deepEqual(unseen({ frames: 0, rows: [{ t: 5, opened: box }], t0: 1, selectors, expect: [] }), ['no frame of the screen was collected']);
});

test('a move with no sample after its action fails, and so does each box it expects, named', () => {
  assert.deepEqual(unseen({ frames: 3, rows: [], t0: 1, selectors, expect: ['opened'] }), [
    'no sample was taken after the action',
    'opened (.wa-row) is not found in 0 of 0 samples',
  ]);
  assert.deepEqual(unseen({ frames: 3, rows: [{ t: 0, opened: box }, { t: 2, opened: null }], t0: 1, selectors, expect: ['opened', 'more'] }), [
    'opened (.wa-row) is not found in 1 of 2 samples',
    'more (.ws-more) is not found in 2 of 2 samples',
  ]);
});

test('a move that saw every box it expects, with frames, has nothing unseen', () => {
  assert.deepEqual(unseen({ frames: 3, rows: [{ t: 0, opened: box }, { t: 2, opened: box }], t0: 1, selectors, expect: ['opened'] }), []);
});
