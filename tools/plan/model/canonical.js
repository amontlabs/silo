// The canonical form of the plan's JSON: keys in the order of their shape, two spaces, a final newline.

import { AREA_KEYS, DECISION_KEYS, FEATURE_KEYS, STAGE_KEYS, SUGGEST_KEYS, SURFACES_KEYS } from './names.js';

/** @param {unknown} value @returns {string} */
export function canonicalText(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

/**
 * Copies an object with the known keys first, in order, then any other key in its own order
 * (the check reports unknown keys; the canonical form keeps them so nothing is lost).
 * @param {Record<string, unknown>} object @param {readonly string[]} keys
 */
function ordered(object, keys) {
  if (object === null || typeof object !== 'object' || Array.isArray(object)) return object;
  /** @type {Record<string, unknown>} */
  const out = {};
  for (const key of keys) if (Object.hasOwn(object, key)) out[key] = object[key];
  for (const key of Object.keys(object)) if (!Object.hasOwn(out, key)) out[key] = object[key];
  return out;
}

const each = (list, fn) => (Array.isArray(list) ? list.map(fn) : list);

/** @param {any} feature */
export function canonicalFeature(feature) {
  const out = /** @type {any} */ (ordered(feature, FEATURE_KEYS));
  if (out && typeof out === 'object') {
    if (out.surfaces) out.surfaces = ordered(out.surfaces, SURFACES_KEYS);
    if (out.suggest) out.suggest = ordered(out.suggest, SUGGEST_KEYS);
  }
  return out;
}

/** @param {any} areas */
export const canonicalAreas = (areas) => each(areas, (area) => ordered(area, AREA_KEYS));

/** @param {any} order */
export function canonicalOrder(order) {
  const out = /** @type {any} */ (ordered(order, ['stages']));
  if (out && typeof out === 'object') out.stages = each(out.stages, (stage) => ordered(stage, STAGE_KEYS));
  return out;
}

/** @param {any} decisions */
export const canonicalDecisions = (decisions) => each(decisions, (decision) => ordered(decision, DECISION_KEYS));
