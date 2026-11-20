import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { canonicalText } from '../../../tools/plan/model/index.js';
import { describe, expect, it } from 'vitest';
import { loadDir } from '../../../tools/plan/read/index.js';
import { buildDataFrom, notesFrom, planDir, readBuildData } from './data';
import { buildData, CORE_NOTES, DECISIONS, emptyPlanFiles, FEATURES, PARTS, planFiles, ZIG_NOTES } from './fixtures';
import { checkLayout, LayoutError, MAP_HEIGHT, SILO_MAP } from './map';
import type { Box, MapLayout } from './map';
import { EXAMPLE_MAP } from './map-example';

const SAMPLE = join(dirname(new URL(import.meta.url).pathname), '../../../tools/plan/test/fixtures/sample');

describe('reading the plan files', () => {
  it('reads decisions and features in the order of order.json, with their position, part and needs', () => {
    const data = buildData();
    expect(data.entries).toHaveLength(FEATURES.length + DECISIONS.length);
    expect(data.entries.map((entry) => entry.position)).toEqual(data.entries.map((_, index) => index + 1));
    expect(data.entries.slice(0, 5).map((entry) => entry.id)).toEqual(['security-properly', 'one-host', 'plain-logs', 'core-in-zig', 'intake']);
    const engine = data.entries.find((entry) => entry.id === 'core');
    expect(engine).toMatchObject({ kind: 'feature', part: 'part-a', stage: 'second', verdict: 'take', built: null, needs: [] });
    expect(engine).not.toHaveProperty('suggest');
    expect(data.entries.find((entry) => entry.id === 'create')?.verdict).toBeNull();
  });

  it('reads a decision as a row: its asks is its name, its answer its body, and a decision of the whole product has no part', () => {
    const entries = buildData().entries;
    expect(entries.find((entry) => entry.id === 'core-in-zig')).toMatchObject({ kind: 'decision', part: 'part-a', name: 'Which language is the core written in?', what: 'Zig. It is the core piece.', verdict: 'take', notes: true });
    expect(entries.find((entry) => entry.id === 'security-properly')).toMatchObject({ kind: 'decision', part: null });
    expect(entries.find((entry) => entry.id === 'one-host')).toMatchObject({ what: null, verdict: null });
  });

  it('reads the notes of an entry beside its file, and none when there are none', () => {
    const entries = buildData().entries;
    expect(entries.find((f) => f.id === 'core')?.notes).toBe(true);
    expect(entries.find((f) => f.id === 'calls')?.notes).toBe(false);
    expect(buildData({ notes: { core: '  \n' } }).entries.find((f) => f.id === 'core')?.notes).toBe(false);
    // The text is a chunk of its own: never in the page's data.
    expect(notesFrom(planFiles())).toEqual({ 'core-in-zig': ZIG_NOTES, core: CORE_NOTES });
    expect(notesFrom(planFiles({ notes: { core: '  \n' } }))).toEqual({});
    expect(JSON.stringify(buildData())).not.toContain('The engine owns every computer');
  });

  it('turns entries\' threads off in the real build, and on in the fixtures', () => {
    expect(buildDataFrom(emptyPlanFiles())).toMatchObject({ threads: false });
    expect(buildData()).toMatchObject({ threads: true });
    expect(buildData()).not.toHaveProperty('decide');
  });
});

describe('a plan that holds no part', () => {
  it('reads with no part and no feature, and keeps the decisions of the whole product', () => {
    const data = buildDataFrom(emptyPlanFiles());
    expect(data).toMatchObject({ parts: [], map: SILO_MAP });
    expect(data.entries.map((entry) => entry.id)).toEqual(['security-properly', 'one-host']);
    expect(notesFrom(emptyPlanFiles())).toEqual({});
  });

  it('reads a part with no feature: nothing stops the build', () => {
    const data = buildDataFrom(planFiles({ features: FEATURES.filter((feature) => feature.part !== 'part-b') }));
    expect(data.parts.map((part) => part.id)).toContain('part-b');
  });
});

describe('a plan with an error', () => {
  it('stops with the file and the fix when a feature is missing from order.json', () => {
    const files = planFiles();
    files.set('part-a/features/stray.json', canonicalText({ name: 'Stray', what: 'Not in the order.' }));
    let message = '';
    try {
      buildDataFrom(files, { map: EXAMPLE_MAP });
    } catch (error) {
      message = (error as Error).message;
    }
    expect(message).toContain('plan/');
    expect(message).toContain('stray');
    expect(message).toContain('Fix:');
  });

  it('stops on a part the example map has no place for; Silo\'s own map, empty, draws no map and takes any plan', () => {
    const files = planFiles({ parts: [...PARTS, { id: 'billing', title: 'Billing', line: 'Money.' }] });
    expect(() => buildDataFrom(files, { map: EXAMPLE_MAP })).toThrow('The map has no place for the part "billing" of plan/parts.json. Add it to website/src/workshop/map.ts.');
    expect(buildDataFrom(files).map).toBe(SILO_MAP);
  });
});

describe('reading plan/ from a repository folder', () => {
  it('reads the folder, and stops when it cannot be read', () => {
    const root = mkdtempSync(join(tmpdir(), 'workshop-data-'));
    try {
      for (const [name, text] of planFiles()) {
        mkdirSync(dirname(join(root, 'plan', name)), { recursive: true });
        writeFileSync(join(root, 'plan', name), text);
      }
      const data = readBuildData(root, EXAMPLE_MAP);
      expect(data.entries).toHaveLength(FEATURES.length + DECISIONS.length);
      expect(data.parts).toHaveLength(PARTS.length);
      expect(data).toMatchObject({ threads: false });
      expect(() => readBuildData(join(root, 'missing'))).toThrow('cannot be read');
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });

  it('reads the plan named by WORKSHOP_PLAN instead: the plan tool\'s sample', () => {
    expect(planDir('/repo', {})).toBe('/repo/plan');
    expect(planDir('/repo', { WORKSHOP_PLAN: SAMPLE })).toBe(SAMPLE);
    const data = buildDataFrom(loadDir(SAMPLE));
    expect(data.parts.map((part) => part.id)).toEqual(['engine', 'record']);
    expect(data.entries.filter((entry) => entry.part === null).map((entry) => entry.id)).toEqual(['the-design', 'nothing-without-a-reason']);
  });
});

describe('the map layout', () => {
  const ids = PARTS.map((part) => part.id);

  it('draws the fixture plan with the example, and Silo\'s plan with its own map', () => {
    expect(buildData().map).toBe(EXAMPLE_MAP);
    expect(Object.keys(EXAMPLE_MAP.places)).toEqual(ids);
    // Silo's own map is empty: every plan passes, and the page shows the tiles alone.
    expect(() => checkLayout(['engine', 'record'])).not.toThrow();
  });

  it('places exactly the parts of the plan', () => {
    expect(() => checkLayout(ids, EXAMPLE_MAP)).not.toThrow();
    expect(() => checkLayout([...ids, 'billing'], EXAMPLE_MAP)).toThrow(LayoutError);
    expect(() => checkLayout(ids.filter((id) => id !== 'part-c'), EXAMPLE_MAP)).toThrow('website/src/workshop/map.ts places the part "part-c", which plan/parts.json does not have.');
    const without = (id: string): MapLayout => ({ ...EXAMPLE_MAP, places: Object.fromEntries(Object.entries(EXAMPLE_MAP.places).filter(([place]) => place !== id)) });
    expect(() => checkLayout(ids.filter((id) => id !== 'part-b'), without('part-b'))).toThrow('website/src/workshop/map.ts draws a wire for the part "part-b", which plan/parts.json does not have.');
    const noWires = { ...without('part-d'), wires: { full: EXAMPLE_MAP.wires.full.filter((wire) => wire.of !== 'part-d'), fold: EXAMPLE_MAP.wires.fold.filter((wire) => wire.of !== 'part-d') } };
    expect(() => checkLayout(ids.filter((id) => id !== 'part-d'), noWires)).toThrow('website/src/workshop/map.ts stacks a copy behind the part "part-d", which plan/parts.json does not have.');
  });

  for (const [name, layout] of [['Silo\'s map', SILO_MAP], ['the example', EXAMPLE_MAP]] as const) {
    it(`keeps every part of ${name} inside the map, full and folded`, () => {
      for (const [id, place] of Object.entries(layout.places)) {
        const [x, y, w, h] = place.full;
        const [cx, cy, cw, ch] = place.fold;
        // The full map's bottom edge leaves room for the focus ring, which is drawn 4 px outside the box.
        expect([id, x + w <= 1136 && y + h <= MAP_HEIGHT - 5 && cx + cw <= 1136 && cy + ch <= 180]).toEqual([id, true]);
      }
    });

    it(`keeps the parts of ${name} clear of each other, of the text that is not a part and of the zones' labels, full and folded`, () => {
      const hit = (a: Box, b: Box) => a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3];
      // A zone's label sits on its top edge, 14 units in, about 120 wide and 16 high.
      const label = (zone: Box): Box => [zone[0] + 14, zone[1] - 8, 120, 16];
      for (const view of ['full', 'fold'] as const) {
        // A part drawn inside another overlaps that one by design, and only that one.
        const parts = Object.entries(layout.places).filter(([, place]) => !place.inner).map(([id, place]) => [id, place[view]] as [string, Box]);
        const others: [string, Box][] = [
          ...layout.notes.map((note) => [`the text ${note.title}`, note[view]] as [string, Box]),
          ...layout.zones.map((zone) => [`label of ${zone.label}`, label(zone[view])] as [string, Box]),
        ];
        const clashes = parts.flatMap(([id, box], index) => [...parts.slice(index + 1), ...others].filter(([, other]) => hit(box, other)).map(([name]) => `${view}: ${id} and ${name}`));
        expect(clashes).toEqual([]);
      }
    });
  }

  it('gives the stage in the stylesheet the height of the full map, which clips what goes past it', () => {
    const css = readFileSync(join(dirname(new URL(import.meta.url).pathname), 'workshop.css'), 'utf8');
    expect(css).toContain(`.ws-stage { --u: calc(100cqw / 1136); position: relative; height: calc(${MAP_HEIGHT} * var(--u));`);
  });
});
