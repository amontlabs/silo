// The home page's two plan lines ("First up: …" and the Workshop door), written into index.html at
// build time by the `homePlan` plugin in vite.config.ts.
import { PlanFileError, readBuildData, STAGES_FILE } from './workshop/data.ts';

const HOME_PLAN_MARKERS = { firstUp: '<!-- plan:first-up -->', door: '<!-- plan:door -->' } as const;

const escapeHtml = (value: string) => value.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

/**
 * The home page's two plan lines, as HTML, from the plan under `repository`. The plan is read and
 * checked exactly as the Workshop reads it, so a missing or malformed file throws.
 */
export function homePlanLines(repository: string, recordingsDir: string): { firstUp: string; door: string } {
  const { stages, features } = readBuildData(repository, recordingsDir);
  const line = stages[0]?.line.trim();
  if (!line) throw new PlanFileError(`${STAGES_FILE}: the first stage has no line`);
  const counted = features.filter((feature) => feature.stage !== 'gone');
  const planned = counted.filter((feature) => feature.verdict === 'take').length;
  // One span per sentence, so a narrow screen breaks the line between them.
  const sentences = [`${planned} of ${counted.length} features are planned.`, 'Have a say in the rest.'];
  return {
    firstUp: escapeHtml(`${line[0].toLowerCase()}${line.slice(1)}`),
    door: sentences.map((sentence) => `<span>${escapeHtml(sentence)}</span>`).join(' '),
  };
}

/** Writes the two plan lines into the home page's markers. */
export function fillHomePlan(html: string, repository: string, recordingsDir: string): string {
  const lines = homePlanLines(repository, recordingsDir);
  for (const [key, marker] of Object.entries(HOME_PLAN_MARKERS)) {
    if (!html.includes(marker)) throw new PlanFileError(`index.html: no ${marker} marker`);
    // A function replacement, so `$` in the plan is never read as a replacement pattern.
    html = html.replace(marker, () => lines[key as keyof typeof lines]);
  }
  return html;
}
