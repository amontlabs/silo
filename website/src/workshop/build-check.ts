// The checks the site's build makes of a plan, every one that can stop it: the plan tool's check, the addresses the
// Workshop keeps for its own pages, and the map's layout against the plan. The build runs them (data.ts), and so does
// the server on the plan an owner's verdict would leave on main (server/verdicts.mjs), so a verdict never breaks the
// next build. Plain enough for Node to load as it is: the server imports this file, with no build step.
import { checkFiles, formatProblem } from '../../../tools/plan/check/index.js';
import { entryStatus, replacedBy } from '../../../tools/plan/model/index.js';
import { parsePlan } from '../../../tools/plan/read/index.js';
import { checkLayout, LayoutError, SILO_MAP } from './map.ts';
import type { LayoutPlan, MapLayout } from './map.ts';
import { RESERVED_ADDRESSES } from './routes.ts';

export class PlanFileError extends Error {}

/** What the map reads of a plan object of tools/plan/read: its parts, its entries and its words. */
export interface LayoutSource {
  parts: { id: string; title: string }[];
  entries: Record<string, { kind: string }>;
  words: { id: string | null }[];
}

/** What the map reads of a plan: its parts, the status of each decision and feature, and its words. */
export function layoutPlanOf(plan: LayoutSource): LayoutPlan {
  // The plan tool's model reads the entries as the files give them, which the check has already passed.
  const entries = plan.entries as unknown as Record<string, Parameters<typeof entryStatus>[0]>;
  const decisions: LayoutPlan['decisions'] = {};
  const features: LayoutPlan['features'] = {};
  for (const [id, entry] of Object.entries(entries)) {
    if (entry.kind === 'decision') decisions[id] = { status: entryStatus(entry, entries) as LayoutPlan['decisions'][string]['status'], replacedBy: replacedBy(entry) };
    else features[id] = { status: entryStatus(entry, entries) as LayoutPlan['features'][string]['status'] };
  }
  return { parts: plan.parts, decisions, features, words: plan.words.flatMap((word) => (word.id ? [word.id] : [])) };
}

/** Checks the files of a plan with the plan tool's check, or throws with every problem. `exists` says whether a repository path exists. */
export function checkedPlan(files: Map<string, string>, exists?: (path: string) => boolean): unknown {
  const { errors } = checkFiles(files, exists ? { exists } : {});
  if (errors.length) throw new PlanFileError(errors.map(formatProblem).join('\n'));
  return parsePlan(files).plan;
}

/**
 * Every check of the build on the files of a plan: the plan tool's, the addresses the Workshop keeps, and the map's
 * layout `map`. Gives the plan object, or throws a PlanFileError with the build's message.
 */
export function checkBuild(files: Map<string, string>, map: MapLayout = SILO_MAP, exists?: (path: string) => boolean): unknown {
  const plan = checkedPlan(files, exists) as LayoutSource;
  for (const part of plan.parts) {
    if ((RESERVED_ADDRESSES as readonly string[]).includes(part.id)) {
      throw new PlanFileError(`plan/parts.json: the part "${part.id}" takes an address the Workshop keeps for its own page (#${part.id}). Fix: give the part another id; the Workshop keeps ${RESERVED_ADDRESSES.join(', ')}.`);
    }
  }
  try {
    checkLayout(layoutPlanOf(plan), map);
  } catch (error) {
    if (error instanceof LayoutError) throw new PlanFileError(error.message);
    throw error;
  }
  return plan;
}
