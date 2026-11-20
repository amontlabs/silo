// Readers of the bot's fixed comments and labels on the issues already on GitHub. The history
// comment is in markers.js.

import { VERDICTS, labelVerdict } from './names.js';

const BUILDING = /^Being built on ([A-Za-z0-9._/-]{1,200})\.$/;

/** @param {string | null | undefined} body @returns {string | null} the branch a claim (`Being built on <branch>.`) names, or null. */
export function readBuildingComment(body) {
  return BUILDING.exec((body ?? '').trim())?.[1] ?? null;
}

/**
 * The verdicts a set of labels names, in the order of VERDICTS; labels that name no verdict are left out.
 * @param {string[]} labels @returns {import('./names.js').Verdict[]}
 */
export function labelVerdicts(labels) {
  const named = new Set(labels.map(labelVerdict).filter(Boolean));
  return /** @type {any} */ (VERDICTS.filter((verdict) => named.has(verdict)));
}
