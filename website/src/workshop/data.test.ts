import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { checkAreasUsed, parseAreas, parseDecisions, parseFeatures, parseStages, readBuildData } from './data';
import { areasText, buildData, decisionsText, featuresText, stagesText } from './fixtures';
import { checkLayout, LAYOUT, LayoutError } from './map';

const stages = parseStages(stagesText);
const areas = parseAreas(areasText);
const entry = (fields: Record<string, unknown>) => JSON.stringify({ n: 1, id: 'a', stage: 'shell', area: 'engine', name: 'A', what: 'W', verdict: null, ...fields });

describe('reading the product files', () => {
  it('reads decided lines newest first, with their issue and without Markdown links', () => {
    const lines = buildData().decided;
    expect(lines.map((line) => [line.date, line.issue])).toEqual([['2026-11-16', 40], ['2026-11-16', null], ['2026-11-15', null]]);
    expect(lines[0].text).toBe('The engine is written in Zig. It is the core piece');
    expect(lines[1].who).toBe("orchestrator, at the owner's word");
    expect(lines[1].text).toBe('Merging changes main only. See the list.');
  });

  it('reads each entry\'s area; only entries of the stage gone have none', () => {
    const features = buildData().features;
    expect(features.find((f) => f.id === 'fork')?.area).toBe('forks');
    expect(features.filter((f) => f.area === null).map((f) => f.id)).toEqual(['reviewer', 'old-desktop']);
  });

  it('fails on a malformed line with its file and line', () => {
    expect(() => parseFeatures(`${entry({})}\n{"n": 2,`, stages, areas)).toThrow(/^docs\/product\/features\.jsonl:2: not valid JSON/);
    expect(() => parseFeatures(`${entry({})}\n\n${entry({ n: 3, id: 'b', name: '' })}`, stages, areas)).toThrow('docs/product/features.jsonl:3: missing "name"');
    expect(() => parseFeatures(entry({ stage: 'nowhere' }), stages, areas)).toThrow('features.jsonl:1: unknown stage "nowhere"');
    expect(() => parseFeatures(entry({ verdict: 'maybe' }), stages, areas)).toThrow('features.jsonl:1: unknown verdict "maybe"');
    expect(() => parseFeatures(entry({ what: undefined }), stages, areas)).toThrow('features.jsonl:1: missing "what"');
    expect(() => parseFeatures(entry({ issue: -3 }), stages, areas)).toThrow('features.jsonl:1: "issue" is not an issue number');
    expect(() => parseFeatures(`${entry({})}\n${entry({ n: 2 })}`, stages, areas)).toThrow('features.jsonl:2: repeats the id a');
    expect(() => parseFeatures(entry({ recordings: ['../x.mp4'] }), stages, areas)).toThrow('features.jsonl:1: recording "../x.mp4" is not a plain video file name');
    expect(() => parseFeatures(entry({ recordings: ['x.mp4'] }), stages, areas, () => false)).toThrow('features.jsonl:1: recording x.mp4 is not in website/public/workshop/recordings');
    expect(() => parseStages('[{"id":"a"}]')).toThrow('docs/product/stages.json: stage 1 (a) has no title');
    expect(() => parseStages('{')).toThrow(/^docs\/product\/stages\.json: not valid JSON/);
    expect(() => parseDecisions('# D\n\n- 2026-11-16 owner text')).toThrow('docs/product/DECISIONS.md:3: expected "- YYYY-MM-DD · who · text"');
  });

  it('stops on an unknown area, a missing area, a repeated area and an area with no entry', () => {
    expect(() => parseFeatures(`${entry({})}\n${entry({ n: 2, id: 'b', area: 'nowhere' })}`, stages, areas)).toThrow('docs/product/features.jsonl:2: unknown area "nowhere"');
    expect(() => parseFeatures(entry({ area: undefined }), stages, areas)).toThrow('features.jsonl:1: missing "area" (only entries of the stage "gone" have none)');
    expect(() => parseFeatures(entry({ area: 3 }), stages, areas)).toThrow('features.jsonl:1: "area" is not text');
    expect(parseFeatures(entry({ stage: 'gone', area: undefined }), stages, areas)[0].area).toBeNull();
    const repeated = '[\n {"id": "a", "title": "A", "line": "L"},\n {"id": "b", "title": "B", "line": "L"},\n {"id": "a", "title": "A2", "line": "L"}\n]';
    expect(() => parseAreas(repeated)).toThrow('docs/product/areas.json:4: repeats the area id a');
    expect(() => parseAreas('[{"id": "a", "title": "A"}]')).toThrow('docs/product/areas.json:1: area a has no line');
    expect(() => parseAreas('{')).toThrow(/^docs\/product\/areas\.json: not valid JSON/);
    const features = parseFeatures(entry({}), stages, areas);
    expect(() => checkAreasUsed(areas, features, areasText)).toThrow('docs/product/areas.json:8: area computers has no entry in docs/product/features.jsonl');
  });

  it('reads the four files from a repository folder and checks the map', () => {
    const root = mkdtempSync(join(tmpdir(), 'workshop-data-'));
    mkdirSync(join(root, 'docs/product'), { recursive: true });
    writeFileSync(join(root, 'docs/product/features.jsonl'), featuresText);
    writeFileSync(join(root, 'docs/product/stages.json'), stagesText);
    writeFileSync(join(root, 'docs/product/areas.json'), areasText);
    writeFileSync(join(root, 'docs/product/DECISIONS.md'), decisionsText);
    mkdirSync(join(root, 'recordings'));
    expect(() => readBuildData(root, join(root, 'recordings'))).toThrow('recording create.mp4 is not in');
    writeFileSync(join(root, 'recordings/create.mp4'), '');
    const data = readBuildData(root, join(root, 'recordings'));
    expect(data.features).toHaveLength(20);
    expect(data.areas).toHaveLength(16);
    expect(data.decided).toHaveLength(3);
    // An area the map does not place stops the build with a plain message.
    const more = JSON.parse(areasText);
    writeFileSync(join(root, 'docs/product/areas.json'), JSON.stringify([...more, { id: 'billing', title: 'Billing', line: 'L' }]));
    writeFileSync(join(root, 'docs/product/features.jsonl'), `${featuresText}\n${entry({ n: 30, id: 'pay', area: 'billing' })}`);
    expect(() => readBuildData(root, join(root, 'recordings'))).toThrow('The map has no place for the area "billing" of docs/product/areas.json. Add it to website/src/workshop/map.ts.');
    expect(() => readBuildData(join(root, 'missing'), root)).toThrow('docs/product/stages.json: cannot be read');
  });
});

describe('the map layout', () => {
  it('places exactly the areas of the plan', () => {
    expect(() => checkLayout(areas.map((area) => area.id))).not.toThrow();
    expect(() => checkLayout([...areas.map((area) => area.id), 'billing'])).toThrow(LayoutError);
    expect(() => checkLayout(areas.map((area) => area.id).filter((id) => id !== 'trust'))).toThrow('website/src/workshop/map.ts places the area "trust", which docs/product/areas.json does not have.');
    const { tasks: _tasks, ...withoutTasks } = LAYOUT;
    expect(() => checkLayout(Object.keys(withoutTasks), withoutTasks)).toThrow('website/src/workshop/map.ts draws a wire for the area "tasks", which docs/product/areas.json does not have.');
  });

  it('keeps every part inside the map, full and folded', () => {
    for (const [id, place] of Object.entries(LAYOUT)) {
      const [x, y, w, h] = place.full;
      const [cx, cy, cw, ch] = place.fold;
      expect([id, x + w <= 1136 && y + h <= 572 && cx + cw <= 1136 && cy + ch <= 180]).toEqual([id, true]);
    }
  });
});
