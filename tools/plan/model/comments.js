// The bot's fixed comments, other than the history comment (markers.js), and their readers.

import { VERDICTS, labelVerdict } from './names.js';

/** `plan decide <id> take|later|drop`. */
export const RECORDED_COMMENT = 'Recorded for the owner from a conversation.';

/** `plan decide <id> clear`. */
export const CLEARED_COMMENT = 'Verdict cleared for the owner from a conversation.';

const BRANCH = /^[A-Za-z0-9._/-]{1,200}$/;
const BUILDING = /^Being built on ([A-Za-z0-9._/-]{1,200})\.$/;

/** `plan start`: the claim. @param {string} branch */
export function buildingComment(branch) {
  if (!BRANCH.test(branch)) throw new Error(`not a branch name: ${branch}`);
  return `Being built on ${branch}.`;
}

/** @param {string | null | undefined} body @returns {string | null} the branch a claim names, or null. */
export function readBuildingComment(body) {
  return BUILDING.exec((body ?? '').trim())?.[1] ?? null;
}

/** `plan sync`: the comment that closes the issue of a feature no longer in the plan. */
export const REMOVED_COMMENT = 'No longer in the plan.';

/** `plan sync`: the comment that closes a second issue the bot made for one entry. @param {number} kept */
export function duplicateComment(kept) {
  return `Duplicate of #${kept}.`;
}

/**
 * `plan sync`: the comment that closes a visitor's suggestion once a feature names it.
 * @param {number} number the entry's issue @param {string} link the entry on the Workshop
 */
export function placedComment(number, link) {
  return `Placed in the plan as #${number}: ${link}`;
}

/**
 * The verdicts a set of labels names, in the order of VERDICTS; labels that name no verdict are left out.
 * @param {string[]} labels @returns {import('./status.js').Verdict[]}
 */
export function labelVerdicts(labels) {
  const named = new Set(labels.map(labelVerdict).filter(Boolean));
  return /** @type {any} */ (VERDICTS.filter((verdict) => named.has(verdict)));
}
