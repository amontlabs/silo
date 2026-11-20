// The list the server reads as `plan-entries.json` (see server/entries.mjs), made from the plan at build
// time: read with tools/plan/read and checked with tools/plan/check, as the page's build does.
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { checkFiles, formatProblem } from '../../tools/plan/check/index.js';
import { loadDir, parsePlan } from '../../tools/plan/read/index.js';
import { readPlanEntries } from '../server/entries.mjs';

const repository = fileURLToPath(new URL('../..', import.meta.url));

/** plan/, or the plan WORKSHOP_PLAN names, as the page's build reads it. */
export function planDir(env = process.env) {
  return env.WORKSHOP_PLAN || join(repository, 'plan');
}

/** The entries of the plan in `dir`, in the order of order.json; throws with every problem of the plan. */
export function planEntries(dir = planDir()) {
  const files = loadDir(dir);
  const { errors } = checkFiles(files);
  if (errors.length) throw new Error(errors.map(formatProblem).join('\n'));
  const { plan } = parsePlan(files);
  const list = plan.stages.flatMap((stage) => stage.entries).map((id) => plan.entries[id]).filter(Boolean).map((entry) => ({
    id: entry.id,
    kind: entry.kind,
    title: (entry.kind === 'decision' ? entry.asks : entry.name) ?? '',
    part: entry.part ?? null,
    suggestedIn: Number.isInteger(entry.suggested_in) ? entry.suggested_in : null,
  }));
  // The server's reader refuses what it could not use, here rather than on the deployed function.
  readPlanEntries(list);
  return list;
}
