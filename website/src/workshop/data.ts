// The plan as the repository holds it: docs/product/features.jsonl, stages.json and DECISIONS.md.
// Read and checked at build time; a malformed line stops the build with its file and line.
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import type { BuildData, DecidedLine, Feature, Stage, Verdict } from './plan.ts';

export const FEATURES_FILE = 'docs/product/features.jsonl';
export const STAGES_FILE = 'docs/product/stages.json';
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

export function parseFeatures(
  source: string,
  stages: Stage[],
  recordingExists: (name: string) => boolean = () => true,
  file = FEATURES_FILE,
): Feature[] {
  const stageIds = new Set(stages.map((stage) => stage.id));
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

/** Reads the three files under `repository` and returns the parsed plan, or throws with file and line. */
export function readBuildData(repository: string, recordingsDir: string): BuildData {
  const read = (file: string) => {
    try {
      return readFileSync(join(repository, file), 'utf8');
    } catch {
      fail(file, null, 'cannot be read');
    }
  };
  const stages = parseStages(read(STAGES_FILE));
  return {
    stages,
    features: parseFeatures(read(FEATURES_FILE), stages, (name) => existsSync(join(recordingsDir, name))),
    decided: parseDecisions(read(DECISIONS_FILE)),
  };
}
