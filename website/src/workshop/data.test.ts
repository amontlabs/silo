import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { canonicalText } from '../../../tools/plan/model/index.js';
import { describe, expect, it } from 'vitest';
import { buildDataFrom, notesFrom, readBuildData } from './data';
import { AREAS, buildData, DECISIONS, emptyPlanFiles, CORE_NOTES, FEATURES, planFiles } from './fixtures';
import { checkLayout, LayoutError, MAP_HEIGHT, SILO_MAP } from './map';
import type { Box, MapLayout } from './map';
import { EXAMPLE_MAP } from './map-example';

describe('reading the plan files', () => {
  it('reads features in build order, with their position, area, needs and suggestion', () => {
    const data = buildData();
    expect(data.features).toHaveLength(FEATURES.length);
    expect(data.features.map((feature) => feature.position)).toEqual(data.features.map((_, index) => index + 1));
    expect(data.features.slice(0, 3).map((feature) => feature.id)).toEqual(['intake', 'core', 'calls']);
    const engine = data.features[1];
    expect(engine).toMatchObject({ area: 'part-a', stage: 'second', verdict: 'take', built: null, needs: [], suggest: { verdict: 'take', why: 'The core piece.' } });
    expect(data.features.find((feature) => feature.id === 'calls')?.suggest).toEqual({ verdict: 'take', why: null });
    expect(data.features.find((feature) => feature.id === 'create')?.verdict).toBeNull();
  });

  it('reads the notes of a feature beside its file, and none when there are none', () => {
    const features = buildData().features;
    expect(features.find((f) => f.id === 'core')?.notes).toBe(true);
    expect(features.find((f) => f.id === 'calls')?.notes).toBe(false);
    expect(buildData({ notes: { core: '  \n' } }).features[0].notes).toBe(false);
    // The text is a chunk of its own: never in the page's data.
    expect(notesFrom(planFiles())).toEqual({ core: CORE_NOTES });
    expect(notesFrom(planFiles({ notes: { core: '  \n' } }))).toEqual({});
    expect(JSON.stringify(buildData())).not.toContain('The engine owns every computer');
  });

  it('reads the decisions newest first, those of one day in the file\'s order, with what they are about', () => {
    const decisions = buildData().decisions;
    expect(decisions.map((decision) => decision.id)).toEqual(['core-in-zig', 'one-way', 'security-properly']);
    expect(decisions.map((decision) => decision.date)).toEqual(['2026-11-16', '2026-11-16', '2026-11-15']);
    expect(decisions[0].about).toEqual(['part-a']);
    expect(decisions[2].about).toBeNull();
    const same = buildData({ decisions: [...DECISIONS].reverse() }).decisions.map((decision) => decision.id);
    expect(same).toEqual(['one-way', 'core-in-zig', 'security-properly']);
  });
});

describe('a plan that holds no part', () => {
  it('reads with no area, stage or feature, and keeps its decisions', () => {
    const data = buildDataFrom(emptyPlanFiles());
    expect(data).toMatchObject({ areas: [], stages: [], features: [], map: SILO_MAP });
    expect(data.decisions).toHaveLength(DECISIONS.length);
    expect(notesFrom(emptyPlanFiles())).toEqual({});
  });
});

describe('a plan with an error', () => {
  it('stops with the file and the fix when a feature is missing from order.json', () => {
    const files = planFiles();
    files.set('features/stray.json', canonicalText({ name: 'Stray', what: 'Not in the order.', area: 'part-a', needs: [] }));
    let message = '';
    try {
      buildDataFrom(files, EXAMPLE_MAP);
    } catch (error) {
      message = (error as Error).message;
    }
    expect(message).toContain('plan/');
    expect(message).toContain('stray');
    expect(message).toContain('Fix:');
  });

  it('stops on an area the map has no place for, and on a place for an area the plan has not', () => {
    const files = planFiles({ features: [...FEATURES, { id: 'pay', stage: 'twelfth', area: 'billing', name: 'Pay', what: 'Billing.' }] });
    files.set('areas.json', canonicalText([...AREAS, { id: 'billing', title: 'Billing', line: 'Money.' }]));
    expect(() => buildDataFrom(files, EXAMPLE_MAP)).toThrow('The map has no place for the area "billing" of plan/areas.json. Add it to website/src/workshop/map.ts.');
    // Silo's own map holds no part yet, so a plan with parts needs each of them added to it.
    expect(() => buildDataFrom(planFiles())).toThrow('The map has no place for the area "part-a" of plan/areas.json.');
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
      expect(data.features).toHaveLength(FEATURES.length);
      expect(data.areas).toHaveLength(AREAS.length);
      expect(data.decisions).toHaveLength(DECISIONS.length);
      expect(() => readBuildData(join(root, 'missing'))).toThrow('plan/: cannot be read');
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  });
});

describe('the map layout', () => {
  const ids = AREAS.map((area) => area.id);

  it('draws the fixture plan with the example, and Silo\'s plan with its own map', () => {
    expect(buildData().map).toBe(EXAMPLE_MAP);
    expect(Object.keys(EXAMPLE_MAP.places)).toEqual(ids);
    expect(() => checkLayout(JSON.parse(readFileSync(join(dirname(new URL(import.meta.url).pathname), '../../../plan/areas.json'), 'utf8')).map((area: { id: string }) => area.id))).not.toThrow();
  });

  it('places exactly the areas of the plan', () => {
    expect(() => checkLayout(ids, EXAMPLE_MAP)).not.toThrow();
    expect(() => checkLayout([...ids, 'billing'], EXAMPLE_MAP)).toThrow(LayoutError);
    expect(() => checkLayout(ids.filter((id) => id !== 'part-c'), EXAMPLE_MAP)).toThrow('website/src/workshop/map.ts places the area "part-c", which plan/areas.json does not have.');
    const without = (id: string): MapLayout => ({ ...EXAMPLE_MAP, places: Object.fromEntries(Object.entries(EXAMPLE_MAP.places).filter(([place]) => place !== id)) });
    expect(() => checkLayout(ids.filter((id) => id !== 'part-b'), without('part-b'))).toThrow('website/src/workshop/map.ts draws a wire for the area "part-b", which plan/areas.json does not have.');
    const noWires = { ...without('part-d'), wires: { full: EXAMPLE_MAP.wires.full.filter((wire) => wire.of !== 'part-d'), fold: EXAMPLE_MAP.wires.fold.filter((wire) => wire.of !== 'part-d') } };
    expect(() => checkLayout(ids.filter((id) => id !== 'part-d'), noWires)).toThrow('website/src/workshop/map.ts stacks a copy behind the area "part-d", which plan/areas.json does not have.');
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
