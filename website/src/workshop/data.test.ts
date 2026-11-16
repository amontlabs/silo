import { mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { parseDecisions, parseFeatures, parseStages, readBuildData } from './data';
import { buildData, decisionsText, featuresText, stagesText } from './fixtures';

const stages = parseStages(stagesText);
const entry = (fields: Record<string, unknown>) => JSON.stringify({ n: 1, id: 'a', stage: 'shell', name: 'A', what: 'W', verdict: null, ...fields });

describe('reading the product files', () => {
  it('reads decided lines newest first, with their issue and without Markdown links', () => {
    const lines = buildData().decided;
    expect(lines.map((line) => [line.date, line.issue])).toEqual([['2026-11-16', 40], ['2026-11-16', null], ['2026-11-15', null]]);
    expect(lines[0].text).toBe('The engine is written in Zig. It is the core piece');
    expect(lines[1].who).toBe("orchestrator, at the owner's word");
    expect(lines[1].text).toBe('Merging changes main only. See the list.');
  });

  it('fails on a malformed line with its file and line', () => {
    expect(() => parseFeatures(`${entry({})}\n{"n": 2,`, stages)).toThrow(/^docs\/product\/features\.jsonl:2: not valid JSON/);
    expect(() => parseFeatures(`${entry({})}\n\n${entry({ n: 3, id: 'b', name: '' })}`, stages)).toThrow('docs/product/features.jsonl:3: missing "name"');
    expect(() => parseFeatures(entry({ stage: 'nowhere' }), stages)).toThrow('features.jsonl:1: unknown stage "nowhere"');
    expect(() => parseFeatures(entry({ verdict: 'maybe' }), stages)).toThrow('features.jsonl:1: unknown verdict "maybe"');
    expect(() => parseFeatures(entry({ what: undefined }), stages)).toThrow('features.jsonl:1: missing "what"');
    expect(() => parseFeatures(entry({ issue: -3 }), stages)).toThrow('features.jsonl:1: "issue" is not an issue number');
    expect(() => parseFeatures(`${entry({})}\n${entry({ n: 2 })}`, stages)).toThrow('features.jsonl:2: repeats the id a');
    expect(() => parseFeatures(entry({ recordings: ['../x.mp4'] }), stages)).toThrow('features.jsonl:1: recording "../x.mp4" is not a plain video file name');
    expect(() => parseFeatures(entry({ recordings: ['x.mp4'] }), stages, () => false)).toThrow('features.jsonl:1: recording x.mp4 is not in website/public/workshop/recordings');
    expect(() => parseStages('[{"id":"a"}]')).toThrow('docs/product/stages.json: stage 1 (a) has no title');
    expect(() => parseStages('{')).toThrow(/^docs\/product\/stages\.json: not valid JSON/);
    expect(() => parseDecisions('# D\n\n- 2026-11-16 owner text')).toThrow('docs/product/DECISIONS.md:3: expected "- YYYY-MM-DD · who · text"');
  });

  it('reads the three files from a repository folder', () => {
    const root = mkdtempSync(join(tmpdir(), 'workshop-data-'));
    mkdirSync(join(root, 'docs/product'), { recursive: true });
    writeFileSync(join(root, 'docs/product/features.jsonl'), featuresText);
    writeFileSync(join(root, 'docs/product/stages.json'), stagesText);
    writeFileSync(join(root, 'docs/product/DECISIONS.md'), decisionsText);
    mkdirSync(join(root, 'recordings'));
    expect(() => readBuildData(root, join(root, 'recordings'))).toThrow('recording create.mp4 is not in');
    writeFileSync(join(root, 'recordings/create.mp4'), '');
    const data = readBuildData(root, join(root, 'recordings'));
    expect(data.features).toHaveLength(8);
    expect(data.decided).toHaveLength(3);
    expect(() => readBuildData(join(root, 'missing'), root)).toThrow('docs/product/stages.json: cannot be read');
  });
});
