// The seam reading on made-up pictures, and what the player's soft shapes must keep doing. The reading itself, in
// the three engines, is `npm run film-seams`, which needs a built site and their browsers.
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { LIMIT, seamStrengths } from './film-seams.mjs';

const W = 60;
const H = 40;
/** A picture of a soft shape's edge: levels falling smoothly from the left, with `mark` set at one column. */
function picture(mark) {
  const gray = [];
  for (let y = 0; y < H; y += 1) for (let x = 0; x < W; x += 1) gray.push(mark && x === mark.x ? mark.level : 230 - x * 0.5);
  return gray;
}

test('a smooth falloff has no seam on a line across it', () => {
  const [d] = seamStrengths(picture(null), W, H, { xs: [30], ys: [] });
  assert.ok(Math.abs(d) < 1, `read ${d}`);
});

test('a dark column reads negative and a light one positive, by how far it stands out', () => {
  const dark = seamStrengths(picture({ x: 30, level: 230 - 15 - 30 * 0.5 }), W, H, { xs: [30], ys: [] });
  assert.ok(dark[0] < -LIMIT, `read ${dark[0]}`);
  const light = seamStrengths(picture({ x: 30, level: 230 + 4 - 30 * 0.5 }), W, H, { xs: [30], ys: [] });
  assert.ok(light[0] > LIMIT * 0.8 && light[0] < LIMIT * 1.6, `read ${light[0]}`);
});

test('a line only part of the way along a shape is read by what most of it does, and one near the edge is skipped', () => {
  const gray = picture(null);
  for (let y = 0; y < 5; y += 1) gray[y * W + 30] -= 40;
  const [d] = seamStrengths(gray, W, H, { xs: [30], ys: [] });
  assert.ok(Math.abs(d) < 1, `read ${d}`);
  assert.deepEqual(seamStrengths(picture(null), W, H, { xs: [2, W - 2], ys: [1] }), []);
});

test("the player's soft shapes are not drawn with crispEdges, and are snapped after every frame", () => {
  const kit = readFileSync(new URL('../../films/player/kit.js', import.meta.url), 'utf8');
  const soft = kit.slice(kit.indexOf('function soft('), kit.indexOf('function shade('));
  assert.ok(soft.includes('function soft('));
  assert.ok(!/crispEdges/.test(soft.replace(/\/\/.*$/gm, '')), 'WebKit rounds a crisp tile outwards, so neighbours overlap');
  const show = kit.slice(kit.indexOf('function show()'));
  assert.ok(show.slice(0, 200).includes('snapSofts()'), 'the lines the tiles meet along are put on whole device pixels at every frame');
});
