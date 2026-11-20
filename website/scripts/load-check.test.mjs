// The load check's choice of places, from the page's data, without a browser.
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { placesOf } from './load-check.mjs';

const hashes = (chosen) => chosen.places.map((place) => place.hash);

test('a plan whose first part has no row visits the first part that has rows, and the whole product', () => {
  const chosen = placesOf({
    parts: [{ id: 'empty' }, { id: 'full' }],
    entries: [{ part: 'full' }, { part: null }],
    threads: false,
  });
  assert.equal(chosen.tile, 'empty');
  assert.equal(chosen.part.hash, '#full');
  assert.equal(chosen.product.hash, '#product');
  assert.equal(chosen.rows, chosen.part);
  assert.deepEqual(hashes(chosen), ['', '#full', '#product', '#suggested']);
  assert.ok(chosen.places.every((place) => place.hash !== '#empty'));
});

test('a plan with rows only in the whole product visits the whole product as its place with rows', () => {
  const chosen = placesOf({
    parts: [{ id: 'a' }, { id: 'b' }],
    entries: [{ part: null }, { part: null }],
    threads: true,
  });
  assert.equal(chosen.tile, 'a');
  assert.equal(chosen.part, null);
  assert.equal(chosen.rows, chosen.product);
  assert.deepEqual(hashes(chosen), ['', '#product', '#latest', '#suggested']);
});

test('a plan with no part and no row visits only the plain view and the suggestions', () => {
  const chosen = placesOf({ parts: [], entries: [], threads: false });
  assert.equal(chosen.tile, null);
  assert.equal(chosen.part, null);
  assert.equal(chosen.product, null);
  assert.equal(chosen.rows, null);
  assert.deepEqual(hashes(chosen), ['', '#suggested']);
});
