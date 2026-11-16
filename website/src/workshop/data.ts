// The plan as the repository holds it: docs/product/features.jsonl, areas.json, stages.json and DECISIONS.md.
// Read and checked at build time; a malformed line stops the build with its file and line.
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { checkLayout, LayoutError } from './map.ts';
import type { Area, BuildData, DecidedLine, Feature, Stage, Verdict } from './plan.ts';

export const FEATURES_FILE = 'docs/product/features.jsonl';
export const STAGES_FILE = 'docs/product/stages.json';
export const AREAS_FILE = 'docs/product/areas.json';
/** Old things not carried over: the only entries without an area. */
export const GONE_STAGE = 'gone';
export const DECISIONS_FILE = 'docs/product/DECISIONS.md';

const VERDICTS = new Set(['take', 'refine', 'later', 'drop']);
const RECORDING = /^[A-Za-z0-9][A-Za-z0-9._-]*\.(mp4|webm|mov|m4v)$/;
const DECISION_LINE = /^- (\d{4}-\d{2}-\d{2}) · (.+?) · (.+)$/;

export class PlanFileError extends Error {}

function fail(file: string, line: number | null, problem: string): never {
  throw new PlanFileError(`${file}${line === null ? '' : `:${line}`}: ${problem}`);
}

function text(value: unknown): value is string {
  return typeof value === 'string' && value.trim() !== '';
}

export function parseStages(source: string, file = STAGES_FILE): Stage[] {
  let raw: unknown;
  try {
    raw = JSON.parse(source);
  } catch (error) {
    fail(file, null, `not valid JSON (${(error as Error).message})`);
  }
  if (!Array.isArray(raw)) fail(file, null, 'expected a list of stages');
  const seen = new Set<string>();
  return raw.map((item: unknown, index) => {
    const stage = item as Record<string, unknown> | null;
    const where = `stage ${index + 1}`;
    if (!stage || typeof stage !== 'object') fail(file, null, `${where} is not an object`);
    if (!text(stage.id)) fail(file, null, `${where} has no id`);
    if (!text(stage.title)) fail(file, null, `${where} (${stage.id}) has no title`);
    if (stage.line !== undefined && typeof stage.line !== 'string') fail(file, null, `${where} (${stage.id}) has a line that is not text`);
    if (seen.has(stage.id)) fail(file, null, `${where} repeats the id ${stage.id}`);
    seen.add(stage.id);
    return { id: stage.id, title: stage.title, line: typeof stage.line === 'string' ? stage.line : '' };
  });
}

/** The line of `"id": "<id>"` in a JSON file, the `nth` time it appears; null when it cannot be found. */
function lineOfId(source: string, id: string, nth = 1): number | null {
  const pattern = new RegExp(`"id"\\s*:\\s*${JSON.stringify(id).replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}`, 'g');
  let seen = 0;
  for (const match of source.matchAll(pattern)) {
    seen += 1;
    if (seen === nth) return source.slice(0, match.index).split('\n').length;
  }
  return null;
}

export function parseAreas(source: string, file = AREAS_FILE): Area[] {
  let raw: unknown;
  try {
    raw = JSON.parse(source);
  } catch (error) {
    fail(file, null, `not valid JSON (${(error as Error).message})`);
  }
  if (!Array.isArray(raw)) fail(file, null, 'expected a list of areas');
  const seen = new Set<string>();
  return raw.map((item: unknown, index) => {
    const area = item as Record<string, unknown> | null;
    const where = `area ${index + 1}`;
    if (!area || typeof area !== 'object') fail(file, null, `${where} is not an object`);
    if (!text(area.id)) fail(file, null, `${where} has no id`);
    const line = lineOfId(source, area.id, seen.has(area.id) ? 2 : 1);
    if (seen.has(area.id)) fail(file, line, `repeats the area id ${area.id}`);
    if (!text(area.title)) fail(file, line, `area ${area.id} has no title`);
    if (!text(area.line)) fail(file, line, `area ${area.id} has no line`);
    seen.add(area.id);
    return { id: area.id, title: area.title, line: area.line };
  });
}

export function parseFeatures(
  source: string,
  stages: Stage[],
  areas: Area[],
  recordingExists: (name: string) => boolean = () => true,
  file = FEATURES_FILE,
): Feature[] {
  const stageIds = new Set(stages.map((stage) => stage.id));
  const areaIds = new Set(areas.map((area) => area.id));
  const ids = new Set<string>();
  const features: Feature[] = [];
  source.split('\n').forEach((raw, index) => {
    const line = index + 1;
    if (!raw.trim()) return;
    let value: unknown;
    try {
      value = JSON.parse(raw);
    } catch (error) {
      fail(file, line, `not valid JSON (${(error as Error).message})`);
    }
    if (!value || typeof value !== 'object' || Array.isArray(value)) fail(file, line, 'expected an object');
    const entry = value as Record<string, unknown>;
    if (!Number.isInteger(entry.n)) fail(file, line, 'missing or invalid "n"');
    for (const field of ['id', 'name', 'stage'] as const) if (!text(entry[field])) fail(file, line, `missing "${field}"`);
    const id = entry.id as string;
    if (ids.has(id)) fail(file, line, `repeats the id ${id}`);
    ids.add(id);
    if (!stageIds.has(entry.stage as string)) fail(file, line, `unknown stage "${entry.stage}"`);
    if (entry.area !== undefined && entry.area !== null && !text(entry.area)) fail(file, line, '"area" is not text');
    const area = text(entry.area) ? entry.area : null;
    if (area !== null && !areaIds.has(area)) fail(file, line, `unknown area "${area}"`);
    if (area === null && entry.stage !== GONE_STAGE) fail(file, line, `missing "area" (only entries of the stage "${GONE_STAGE}" have none)`);
    if (typeof entry.what !== 'string') fail(file, line, 'missing "what"');
    if (entry.verdict !== null && entry.verdict !== undefined && !(typeof entry.verdict === 'string' && VERDICTS.has(entry.verdict))) {
      fail(file, line, `unknown verdict ${JSON.stringify(entry.verdict)}`);
    }
    for (const field of ['suggest', 'why'] as const) {
      if (entry[field] !== null && entry[field] !== undefined && typeof entry[field] !== 'string') fail(file, line, `"${field}" is not text`);
    }
    if (entry.issue !== null && entry.issue !== undefined && !(Number.isInteger(entry.issue) && (entry.issue as number) > 0)) {
      fail(file, line, '"issue" is not an issue number');
    }
    let recordings: string[] = [];
    if (entry.recordings !== undefined) {
      if (!Array.isArray(entry.recordings)) fail(file, line, '"recordings" is not a list');
      recordings = entry.recordings.map((name: unknown) => {
        if (typeof name !== 'string' || !RECORDING.test(name)) fail(file, line, `recording ${JSON.stringify(name)} is not a plain video file name`);
        if (!recordingExists(name)) fail(file, line, `recording ${name} is not in website/public/workshop/recordings`);
        return name;
      });
    }
    features.push({
      n: entry.n as number,
      id,
      stage: entry.stage as string,
      area,
      name: entry.name as string,
      what: entry.what,
      suggest: text(entry.suggest) ? entry.suggest : null,
      why: text(entry.why) ? entry.why : null,
      verdict: (entry.verdict ?? null) as Verdict | null,
      issue: (entry.issue ?? null) as number | null,
      recordings,
    });
  });
  return features.sort((a, b) => a.n - b.n);
}

/** Markdown links in the repository's own files keep only their label. */
function stripLinks(value: string): string {
  return value.replace(/\[([^\]]+)\]\([^)\s]*\)/g, '$1');
}

export function parseDecisions(markdown: string, file = DECISIONS_FILE): DecidedLine[] {
  const lines: DecidedLine[] = [];
  markdown.split('\n').forEach((raw, index) => {
    const source = raw.trimEnd();
    if (!source.startsWith('- ')) return;
    const match = DECISION_LINE.exec(source);
    if (!match) fail(file, index + 1, 'expected "- YYYY-MM-DD · who · text"');
    if (Number.isNaN(Date.parse(`${match[1]}T00:00:00Z`))) fail(file, index + 1, `invalid date ${match[1]}`);
    let body = match[3];
    let issue: number | null = null;
    const ref = / \(#(\d+)\)$/.exec(body);
    if (ref) {
      issue = Number(ref[1]);
      body = body.slice(0, ref.index);
    }
    lines.push({ index: lines.length, date: match[1], who: match[2], text: stripLinks(body), issue });
  });
  // Newest first; lines of the same day keep the file's order.
  return lines.sort((a, b) => (a.date === b.date ? a.index - b.index : a.date < b.date ? 1 : -1));
}

/** Every area holds at least one entry. */
export function checkAreasUsed(areas: Area[], features: Feature[], source = '', file = AREAS_FILE): void {
  const used = new Set(features.map((feature) => feature.area));
  for (const area of areas) {
    if (!used.has(area.id)) fail(file, lineOfId(source, area.id), `area ${area.id} has no entry in ${FEATURES_FILE}`);
  }
}

/** Reads the four files under `repository` and returns the parsed plan, or throws with file and line. */
export function readBuildData(repository: string, recordingsDir: string): BuildData {
  const read = (file: string) => {
    try {
      return readFileSync(join(repository, file), 'utf8');
    } catch {
      fail(file, null, 'cannot be read');
    }
  };
  const stages = parseStages(read(STAGES_FILE));
  const areasSource = read(AREAS_FILE);
  const areas = parseAreas(areasSource);
  const features = parseFeatures(read(FEATURES_FILE), stages, areas, (name) => existsSync(join(recordingsDir, name)));
  checkAreasUsed(areas, features, areasSource);
  try {
    checkLayout(areas.map((area) => area.id));
  } catch (error) {
    if (error instanceof LayoutError) throw new PlanFileError(error.message);
    throw error;
  }
  return { stages, areas, features, decided: parseDecisions(read(DECISIONS_FILE)) };
}
