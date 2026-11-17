// The plan as the repository holds it, in plan/: read with tools/plan/read and checked with tools/plan/check
// at build time. Any error stops the build with the file, the field and the fix.
import { join } from 'node:path';
import { checkFiles, formatProblem } from '../../../tools/plan/check/index.js';
import { loadDir, parsePlan } from '../../../tools/plan/read/index.js';
import { checkLayout, LayoutError } from './map.ts';
import type { BuildData, Decision, Feature, Notes, Verdict } from './plan.ts';

export const PLAN_DIR = 'plan';

export class PlanFileError extends Error {}

interface RawFeature {
  name: string;
  what: string;
  area?: string;
  needs?: string[];
  suggest?: { verdict?: string; why?: string };
  verdict?: string;
  built?: string;
  stage: string;
  position: number;
  notes: string | null;
}

interface RawPlan {
  areas: { id: string; title: string; line: string }[];
  stages: { id: string; title: string; line: string; features: string[] }[];
  features: Record<string, RawFeature>;
  decisions: { id: string; date: string; text: string; about?: string[] }[];
}

const hasNotes = (raw: RawFeature) => typeof raw.notes === 'string' && raw.notes.trim() !== '';

/** The notes of the features that have some, by id: the chunk the page loads when a person reaches "Notes". */
export function toNotes(plan: RawPlan): Notes {
  const notes: Notes = {};
  for (const [id, raw] of Object.entries(plan.features)) if (hasNotes(raw)) notes[id] = raw.notes as string;
  return notes;
}

/** The plan object of tools/plan/read, as the page holds it: features in build order, decisions newest first. */
export function toBuildData(plan: RawPlan): BuildData {
  const features: Feature[] = plan.stages.flatMap((stage) => stage.features).map((id) => {
    const raw = plan.features[id];
    return {
      id,
      name: raw.name,
      what: raw.what,
      area: raw.area ?? null,
      needs: Array.isArray(raw.needs) ? raw.needs : [],
      stage: raw.stage,
      position: raw.position,
      suggest: raw.suggest ? { verdict: (raw.suggest.verdict ?? null) as Verdict | null, why: raw.suggest.why ?? null } : null,
      verdict: (raw.verdict ?? null) as Verdict | null,
      built: raw.built ?? null,
      notes: hasNotes(raw),
    };
  });
  // Newest first; decisions of the same day keep the file's order.
  const decisions: Decision[] = plan.decisions
    .map((decision, index) => ({ decision, index }))
    .sort((a, b) => (a.decision.date === b.decision.date ? a.index - b.index : a.decision.date < b.decision.date ? 1 : -1))
    .map(({ decision }) => ({ id: decision.id, date: decision.date, text: decision.text, about: decision.about ?? null }));
  return {
    areas: plan.areas.map((area) => ({ id: area.id, title: area.title, line: area.line })),
    stages: plan.stages.map((stage) => ({ id: stage.id, title: stage.title, line: stage.line })),
    features,
    decisions,
  };
}

/** Checks the files of a plan, or throws with every problem. */
function checkedPlan(files: Map<string, string>): RawPlan {
  const { errors } = checkFiles(files);
  if (errors.length) throw new PlanFileError(errors.map(formatProblem).join('\n'));
  return parsePlan(files).plan as RawPlan;
}

/** Checks the files of a plan and gives their notes, or throws with every problem. */
export function notesFrom(files: Map<string, string>): Notes {
  return toNotes(checkedPlan(files));
}

/** Checks the files of a plan and gives the page's data, or throws with every problem. */
export function buildDataFrom(files: Map<string, string>): BuildData {
  const data = toBuildData(checkedPlan(files));
  // Every area holds at least one feature: the map has no empty part.
  const used = new Set(data.features.map((feature) => feature.area));
  for (const area of data.areas) {
    if (!used.has(area.id)) throw new PlanFileError(`plan/areas.json: ${area.id}: has no feature. Fix: give it a feature, or remove the area`);
  }
  try {
    checkLayout(data.areas.map((area) => area.id));
  } catch (error) {
    if (error instanceof LayoutError) throw new PlanFileError(error.message);
    throw error;
  }
  return data;
}

function planFilesIn(repository: string): Map<string, string> {
  try {
    return loadDir(join(repository, PLAN_DIR));
  } catch {
    throw new PlanFileError(`${PLAN_DIR}/: cannot be read`);
  }
}

/** Reads plan/ under `repository` and returns the page's data, or throws. */
export function readBuildData(repository: string): BuildData {
  return buildDataFrom(planFilesIn(repository));
}

/** Reads plan/ under `repository` and returns its notes, or throws. */
export function readNotes(repository: string): Notes {
  return notesFrom(planFilesIn(repository));
}
