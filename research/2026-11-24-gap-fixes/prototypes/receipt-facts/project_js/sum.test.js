const test = require('node:test');
const assert = require('node:assert');
const { sum } = require('./sum.js');
test('empty', () => assert.strictEqual(sum([]), 0));
test('three', () => assert.strictEqual(sum([1, 2, 3]), 6));
