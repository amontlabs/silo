// An example of a map layout, not Silo's plan. Its five parts and their names are made up; they show how the parts
// of plan/parts.json become places and wires on the map, and every kind of thing a layout can hold: a tall part with
// a glyph, a part inside another, copies stacked behind a part, a wide bar, zones, text that is not a part, and
// wires with arrows, labels and a cross. The tests draw the fixture plan (fixtures.ts) with it; the page never shows
// it. The real parts go in SILO_MAP in map.ts.
// What each thing rests on is one of the fixture's parts, never one of its decisions: tests change those decisions for
// their own reasons (taken, replaced, dropped), and the map's stops must not fire there. The tests of the stops give
// the example's wires the rests each case needs.
import type { MapLayout } from './map';

export const EXAMPLE_MAP: MapLayout = {
  file: 'website/src/workshop/map-example.ts',
  places: {
    'part-a': {
      full: [64, 114, 176, 360], fold: [40, 38, 176, 114], tall: true, rests: ['part:part-a'],
      glyph: {
        viewBox: '0 0 60 40',
        at: [16, 70, 60],
        paths: [{ d: 'M.5 .5h59v39h-59z' }, { d: 'M8 31 22 17l10 9 18-16', kind: 'core' }, { d: 'M46 28h6v6h-6z', kind: 'solid' }],
      },
    },
    'part-b': { full: [472, 114, 312, 128], fold: [412, 34, 388, 38] },
    'part-c': { full: [488, 174, 120, 52], fold: [520, 41, 104, 24], inner: true, ty: 7, sy: 29 },
    'part-d': { full: [472, 322, 152, 64], fold: [412, 84, 120, 30] },
    // One wide, low bar under the whole drawing.
    'part-e': { full: [24, 578, 1088, 36], fold: [944, 96, 168, 30], wide: true, ty: 8, sy: 10 },
  },
  zones: [
    { label: 'ZONE ONE', full: [24, 18, 840, 536], fold: [16, 10, 856, 160], corners: true, rests: ['part:part-a'] },
    { label: 'ZONE TWO', full: [440, 90, 360, 416], fold: [400, 22, 412, 138], rounded: true, rests: ['part:part-b'] },
  ],
  notes: [{ title: 'Outside', line: 'not a part', full: [946, 160, 150, 40], fold: [952, 44, 150, 36], rests: ['part:part-b'] }],
  ghosts: [
    { of: 'part-d', full: [484, 334, 152, 64], fold: [412, 84, 120, 30], rests: ['part:part-d'] },
    { of: 'part-d', full: [478, 328, 152, 64], fold: [412, 84, 120, 30] },
  ],
  wires: {
    full: [
      { of: 'part-b', d: 'M240 178H465', arrows: [[472, 178, 'r']], label: [356, 170, 'uses', 'middle'], rests: ['part:part-b'] },
      { of: 'part-d', d: 'M548 242V315', arrows: [[548, 322, 'd']], label: [557, 287, 'leads to'], rests: ['part:part-d'] },
      { of: 'part-d', d: 'M636 360H700', cross: [708, 360], label: [720, 364, 'stops'], rests: ['part:part-d'] },
      { of: 'part-b', d: 'M784 178H932', arrows: [[939, 178, 'r']], rests: ['part:part-b'] },
    ],
    fold: [
      { of: 'part-b', d: 'M216 53H405', arrows: [[412, 53, 'r']] },
      { of: 'part-d', d: 'M472 72V77', arrows: [[472, 84, 'd']] },
      { of: 'part-b', d: 'M800 53H945', arrows: [[952, 53, 'r']] },
    ],
  },
};
