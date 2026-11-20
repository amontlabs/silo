// The plan as the repository holds it, in plan/: read with tools/plan/read and checked with tools/plan/check
// at build time. Any error stops the build with the file, the field and the fix.
import { existsSync } from 'node:fs';
import { join } from 'node:path';
import { checkFiles, formatProblem } from '../../../tools/plan/check/index.js';
import { loadDir, parsePlan } from '../../../tools/plan/read/index.js';
import { checkLayout, LayoutError, SILO_MAP } from './map.ts';
import type { MapLayout } from './map.ts';
import { RESERVED_ADDRESSES } from './routes.ts';
import type { BuildData, Entry, Notes, Verdict } from './plan.ts';

export const PLAN_DIR = 'plan';

export class PlanFileError extends Error {}

interface RawEntry {
  id: string;
  kind: 'decision' | 'feature';
  part: string | null;
  stage: string;
  position: number;
  notes?: string;
  name?: string;
  what?: string;
  asks?: string;
  answer?: string;
  needs?: string[];
  suggest?: { verdict?: string; why?: string };
  verdict?: string;
  built?: string;
}

interface RawPlan {
  parts: { id: string; title: string; line: string }[];
  stages: { id: string; title: string; line: string; entries: string[] }[];
  entries: Record<string, RawEntry>;
}

/** The map, and what the page may do beyond showing the plan: the owner's verdicts and entries' threads (both off by default). */
export interface BuildOptions {
  map?: MapLayout;
  decide?: boolean;
  threads?: boolean;
}

const hasNotes = (raw: RawEntry) => typeof raw.notes === 'string' && raw.notes.trim() !== '';

const ordered = (plan: RawPlan): RawEntry[] => plan.stages.flatMap((stage) => stage.entries).map((id) => plan.entries[id]).filter(Boolean);

/** The notes of the entries that have some, by id: the chunk the page loads when a person reaches "Notes". */
export function toNotes(plan: RawPlan): Notes {
  const notes: Notes = {};
  for (const raw of ordered(plan)) if (hasNotes(raw)) notes[raw.id] = raw.notes as string;
  return notes;
}

/** The plan object of tools/plan/read, as the page holds it: decisions and features in the order of order.json. */
export function toBuildData(plan: RawPlan, { map = SILO_MAP, decide = false, threads = false }: BuildOptions = {}): BuildData {
  const entries: Entry[] = ordered(plan).map((raw) => ({
    id: raw.id,
    kind: raw.kind,
    part: raw.part,
    stage: raw.stage,
    position: raw.position,
    name: (raw.kind === 'decision' ? raw.asks : raw.name) ?? '',
    what: (raw.kind === 'decision' ? raw.answer : raw.what) ?? null,
    needs: Array.isArray(raw.needs) ? raw.needs : [],
    suggest: raw.suggest ? { verdict: (raw.suggest.verdict ?? null) as Verdict | null, why: raw.suggest.why ?? null } : null,
    verdict: (raw.verdict ?? null) as Verdict | null,
    built: raw.built ?? null,
    notes: hasNotes(raw),
  }));
  return {
    parts: plan.parts.map((part) => ({ id: part.id, title: part.title, line: part.line })),
    stages: plan.stages.map((stage) => ({ id: stage.id, title: stage.title, line: stage.line })),
    entries,
    map,
    decide,
    threads,
  };
}

/** Checks the files of a plan, or throws with every problem. `exists` says whether a repository path exists. */
function checkedPlan(files: Map<string, string>, exists?: (path: string) => boolean): RawPlan {
  const { errors } = checkFiles(files, exists ? { exists } : {});
  if (errors.length) throw new PlanFileError(errors.map(formatProblem).join('\n'));
  return parsePlan(files).plan as unknown as RawPlan;
}

/** Checks the files of a plan and gives their notes, or throws with every problem. */
export function notesFrom(files: Map<string, string>): Notes {
  return toNotes(checkedPlan(files));
}

/** Checks the files of a plan and gives the page's data, or throws with every problem. */
export function buildDataFrom(files: Map<string, string>, options: BuildOptions = {}, exists?: (path: string) => boolean): BuildData {
  const data = toBuildData(checkedPlan(files, exists), options);
  for (const part of data.parts) {
    if ((RESERVED_ADDRESSES as readonly string[]).includes(part.id)) {
      throw new PlanFileError(`plan/parts.json: the part "${part.id}" takes an address the Workshop keeps for its own page (#${part.id}). Fix: give the part another id; the Workshop keeps ${RESERVED_ADDRESSES.join(', ')}.`);
    }
  }
  try {
    checkLayout(data.parts.map((part) => part.id), data.map);
  } catch (error) {
    if (error instanceof LayoutError) throw new PlanFileError(error.message);
    throw error;
  }
  return data;
}

/** The plan's folder: `WORKSHOP_PLAN` when set (a fixture plan, for a build that proves the page), else plan/ under `repository`. */
export function planDir(repository: string, env: Record<string, string | undefined> = process.env): string {
  return env.WORKSHOP_PLAN || join(repository, PLAN_DIR);
}

function planFilesIn(dir: string): Map<string, string> {
  try {
    return loadDir(dir);
  } catch {
    throw new PlanFileError(`${dir}: cannot be read`);
  }
}

/** Reads the plan of `repository` and returns the page's data for the real build: no verdicts, no threads. */
export function readBuildData(repository: string, map: MapLayout = SILO_MAP): BuildData {
  return buildDataFrom(planFilesIn(planDir(repository)), { map }, (path) => existsSync(join(repository, path)));
}

/** Reads the plan of `repository` and returns its notes, or throws. */
export function readNotes(repository: string): Notes {
  return notesFrom(planFilesIn(planDir(repository)));
}
