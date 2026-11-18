import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { canonicalText } from '../../../tools/plan/model/index.js';
import { describe, expect, it } from 'vitest';
import { buildDataFrom, notesFrom, readBuildData } from './data';
import { AREAS, buildData, DECISIONS, ENGINE_NOTES, FEATURES, planFiles } from './fixtures';
import { checkLayout, LAYOUT, LayoutError, MAP_HEIGHT, OUTSIDE, ZONES } from './map';
import type { Box } from './map';

describe('reading the plan files', () => {
  it('reads features in build order, with their position, area, needs and suggestion', () => {
    const data = buildData();
    expect(data.features).toHaveLength(FEATURES.length);
    expect(data.features.map((feature) => feature.position)).toEqual(data.features.map((_, index) => index + 1));
    expect(data.features.slice(0, 3).map((feature) => feature.id)).toEqual(['intake', 'engine', 'calls']);
    const engine = data.features[1];
    expect(engine).toMatchObject({ area: 'engine', stage: 'shell', verdict: 'take', built: null, needs: [], suggest: { verdict: 'take', why: 'The core piece.' } });
    expect(data.features.find((feature) => feature.id === 'calls')?.suggest).toEqual({ verdict: 'take', why: null });
    expect(data.features.find((feature) => feature.id === 'computers')?.verdict).toBeNull();
  });

  it('reads each entry\'s area; only entries of the stage gone have none', () => {
    const features = buildData().features;
    expect(features.find((f) => f.id === 'fork')?.area).toBe('forks');
    expect(features.filter((f) => f.area === null).map((f) => f.id)).toEqual(['reviewer', 'old-desktop']);
  });

  it('reads the notes of a feature beside its file, and none when there are none', () => {
    const features = buildData().features;
    expect(features.find((f) => f.id === 'engine')?.notes).toBe(true);
    expect(features.find((f) => f.id === 'calls')?.notes).toBe(false);
    expect(buildData({ notes: { engine: '  \n' } }).features[0].notes).toBe(false);
    // The text is a chunk of its own: never in the page's data.
    expect(notesFrom(planFiles())).toEqual({ engine: ENGINE_NOTES });
    expect(notesFrom(planFiles({ notes: { engine: '  \n' } }))).toEqual({});
    expect(JSON.stringify(buildData())).not.toContain('The engine owns every computer');
  });

  it('reads the areas and stages as the files give them', () => {
    const data = buildData();
    expect(data.areas.map((area) => area.id)).toEqual(AREAS.map((area) => area.id));
    expect(data.areas[0]).toEqual({ id: 'engine', title: 'Engine', line: 'The core. Everything else talks to it.' });
    expect(data.stages.map((stage) => stage.id)).toContain('gone');
  });

  it('reads the decisions newest first, those of one day in the file\'s order, with what they are about', () => {
    const decisions = buildData().decisions;
    expect(decisions.map((decision) => decision.id)).toEqual(['engine-in-zig', 'merge-main-only', 'security-properly']);
    expect(decisions.map((decision) => decision.date)).toEqual(['2026-11-16', '2026-11-16', '2026-11-15']);
    expect(decisions[0].about).toEqual(['engine']);
    expect(decisions[2].about).toBeNull();
    const same = buildData({ decisions: [...DECISIONS].reverse() }).decisions.map((decision) => decision.id);
    expect(same).toEqual(['merge-main-only', 'engine-in-zig', 'security-properly']);
  });

  it('keeps no recordings, issue numbers or entry numbers', () => {
    const feature = buildData().features[0] as unknown as Record<string, unknown>;
    expect(Object.keys(feature).sort()).toEqual(['area', 'built', 'id', 'name', 'needs', 'notes', 'position', 'stage', 'suggest', 'verdict', 'what']);
  });
});

describe('a plan with an error', () => {
  it('stops with the file and the fix when a feature is missing from order.json', () => {
    const files = planFiles();
    files.set('features/stray.json', canonicalText({ name: 'Stray', what: 'Not in the order.', area: 'engine', needs: [] }));
    let message = '';
    try {
      buildDataFrom(files);
    } catch (error) {
      message = (error as Error).message;
    }
    expect(message).toContain('plan/');
    expect(message).toContain('stray');
    expect(message).toContain('Fix:');
  });

  it('stops with the file and the fix when a feature names an unknown area', () => {
    const features = FEATURES.map((feature) => (feature.id === 'exec' ? { ...feature, area: 'nowhere' } : feature));
    let message = '';
    try {
      buildData({ features });
    } catch (error) {
      message = (error as Error).message;
    }
    expect(message).toContain('plan/features/exec.json');
    expect(message).toContain('nowhere');
    expect(message).toContain('Fix:');
  });

  it('stops on an area with no feature, with what to do', () => {
    const features = FEATURES.filter((feature) => feature.area !== 'sharing').map((feature) => feature);
    expect(() => buildData({ features })).toThrow(/plan\/areas\.json: sharing: has no feature\. Fix: /);
  });

  it('stops on an area the map has no place for, and on a place for an area the plan has not', () => {
    const files = planFiles({ features: [...FEATURES, { id: 'pay', stage: 'extras', area: 'billing', name: 'Pay', what: 'Billing.' }] });
    files.set('areas.json', canonicalText([...AREAS, { id: 'billing', title: 'Billing', line: 'Money.' }]));
    expect(() => buildDataFrom(files)).toThrow('The map has no place for the area "billing" of plan/areas.json. Add it to website/src/workshop/map.ts.');
  });

  it('reports every problem of the files at once', () => {
    const files = planFiles();
    files.set('features/engine.json', '{');
    files.set('areas.json', '[');
    let message = '';
    try {
      buildDataFrom(files);
    } catch (error) {
      message = (error as Error).message;
    }
    expect(message.split('\n').length).toBeGreaterThanOrEqual(2);
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
      const data = readBuildData(root);
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

  it('places exactly the areas of the plan', () => {
    expect(() => checkLayout(ids)).not.toThrow();
    expect(() => checkLayout([...ids, 'billing'])).toThrow(LayoutError);
    expect(() => checkLayout(ids.filter((id) => id !== 'trust'))).toThrow('website/src/workshop/map.ts places the area "trust", which plan/areas.json does not have.');
    const { tasks: _tasks, ...withoutTasks } = LAYOUT;
    expect(() => checkLayout(Object.keys(withoutTasks), withoutTasks)).toThrow('website/src/workshop/map.ts draws a wire for the area "tasks", which plan/areas.json does not have.');
  });

  it('keeps every part inside the map, full and folded', () => {
    for (const [id, place] of Object.entries(LAYOUT)) {
      const [x, y, w, h] = place.full;
      const [cx, cy, cw, ch] = place.fold;
      expect([id, x + w <= 1136 && y + h <= MAP_HEIGHT && cx + cw <= 1136 && cy + ch <= 180]).toEqual([id, true]);
    }
  });

  it('gives the stage in the stylesheet the height of the full map, which clips what goes past it', () => {
    const css = readFileSync(join(dirname(new URL(import.meta.url).pathname), 'workshop.css'), 'utf8');
    expect(css).toContain(`.ws-stage { --u: calc(100cqw / 1136); position: relative; height: calc(${MAP_HEIGHT} * var(--u));`);
  });

  it('leaves room under every part for its focus ring, which is drawn 4 px outside the box', () => {
    const tight = Object.entries(LAYOUT).filter(([, place]) => place.full[1] + place.full[3] > MAP_HEIGHT - 5).map(([id]) => id);
    expect(tight).toEqual([]);
  });

  it('keeps Groundwork clear of every other part, the GitHub text and the zones\' labels, full and folded', () => {
    const hit = (a: Box, b: Box) => a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3];
    // A zone's label sits on its top edge, 14 units in, about 120 wide and 16 high.
    const label = (zone: Box): Box => [zone[0] + 14, zone[1] - 8, 120, 16];
    for (const view of ['full', 'fold'] as const) {
      const ground = LAYOUT.ground[view];
      const others: [string, Box][] = [
        ...Object.entries(LAYOUT).filter(([id]) => id !== 'ground').map(([id, place]) => [id, place[view]] as [string, Box]),
        ['GitHub text', OUTSIDE[view]],
        ...ZONES.map((zone) => [`label of ${zone.label}`, label(zone[view])] as [string, Box]),
      ];
      expect(others.filter(([, box]) => hit(ground, box)).map(([name]) => `${view}: ${name}`)).toEqual([]);
    }
  });
});
