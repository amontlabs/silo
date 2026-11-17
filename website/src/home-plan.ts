// The home page's two plan lines ("First up: …" and the Workshop door), written into index.html at
// build time by the `homePlan` plugin in vite.config.ts.
import { PlanFileError, readBuildData } from './workshop/data.ts';
import { entryStatus, groupAreas } from './workshop/plan.ts';

const HOME_PLAN_MARKERS = { firstUp: '<!-- plan:first-up -->', door: '<!-- plan:door -->' } as const;

const escapeHtml = (value: string) => value.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

/**
 * The home page's two plan lines, as HTML, from the plan under `repository`. The plan is read and
 * checked exactly as the Workshop reads it, so a missing or malformed file throws.
 */
export function homePlanLines(repository: string): { firstUp: string; door: string } {
  const data = readBuildData(repository);
  const line = data.stages[0]?.line.trim();
  if (!line) throw new PlanFileError('plan/order.json: stages: the first stage has no line. Fix: write its line');
  // The map's counts, from the files alone: "planned" is what the map calls planned, and a feature counts as
  // the map counts it, in its area and not dropped.
  const byId = new Map(data.features.map((feature) => [feature.id, feature]));
  const groups = groupAreas(data, (id) => entryStatus(byId.get(id) as (typeof data.features)[number], undefined));
  const planned = groups.reduce((sum, group) => sum + group.counts.planned, 0);
  const total = groups.reduce((sum, group) => sum + group.rows.length - group.counts.dropped, 0);
  // One span per sentence, so a narrow screen breaks the line between them.
  const sentences = [`${planned} of ${total} features are planned.`, 'Have a say in the rest.'];
  return {
    firstUp: escapeHtml(`${line[0].toLowerCase()}${line.slice(1)}`),
    door: sentences.map((sentence) => `<span>${escapeHtml(sentence)}</span>`).join(' '),
  };
}

/** Writes the two plan lines into the home page's markers. */
export function fillHomePlan(html: string, repository: string): string {
  const lines = homePlanLines(repository);
  for (const [key, marker] of Object.entries(HOME_PLAN_MARKERS)) {
    if (!html.includes(marker)) throw new PlanFileError(`index.html: no ${marker} marker`);
    // A function replacement, so `$` in the plan is never read as a replacement pattern.
    html = html.replace(marker, () => lines[key as keyof typeof lines]);
  }
  return html;
}
