// The canonical form of the plan's JSON: keys in the order of their kind, two spaces, a final newline.

import {
  DECISION_KEYS, FEATURE_KEYS, ORDER_KEYS, PART_KEYS, PRINCIPLE_KEYS, PROPERTY_KEYS, STAGE_KEYS, SUGGEST_KEYS, SURFACES_KEYS, WORD_KEYS, listKind,
} from './names.js';

/** @param {unknown} value @returns {string} the file's text. */
export function canonicalText(value) {
  return `${JSON.stringify(value, null, 2)}\n`;
}

/**
 * Copies an object with the known keys first, in order, then any other key in its own order
 * (the check reports unknown keys; the canonical form keeps them so nothing is lost).
 * Anything that is not a plain object is returned as it is.
 * @param {any} object @param {readonly string[]} keys
 */
function ordered(object, keys) {
  if (object === null || typeof object !== 'object' || Array.isArray(object)) return object;
  /** @type {Record<string, unknown>} */
  const out = {};
  for (const key of keys) if (Object.hasOwn(object, key)) out[key] = object[key];
  for (const key of Object.keys(object)) if (!Object.hasOwn(out, key)) out[key] = object[key];
  return out;
}

/** @param {any} value @param {string} key @param {(v: any) => any} fn */
function within(value, key, fn) {
  if (value && typeof value === 'object' && !Array.isArray(value) && Object.hasOwn(value, key)) value[key] = fn(value[key]);
  return value;
}

const each = (fn) => (list) => (Array.isArray(list) ? list.map(fn) : list);

/** @param {any} word */
export const canonicalWord = (word) => ordered(word, WORD_KEYS);
/** @param {any} principle */
export const canonicalPrinciple = (principle) => ordered(principle, PRINCIPLE_KEYS);
/** @param {any} property */
export const canonicalProperty = (property) => ordered(property, PROPERTY_KEYS);
/** @param {any} part */
export const canonicalPart = (part) => ordered(part, PART_KEYS);

const SENTENCE_KEYS = { word: WORD_KEYS, principle: PRINCIPLE_KEYS, property: PROPERTY_KEYS };

/**
 * An item of `sets`: `in`, then the keys of the sentence kind its list holds (none known when the
 * list is not one).
 * @param {any} item
 */
export function canonicalSetItem(item) {
  const kind = item && typeof item === 'object' ? listKind(item.in) : null;
  return ordered(item, ['in', ...(kind ? SENTENCE_KEYS[kind] : [])]);
}

/** @param {any} entry @param {readonly string[]} keys */
function canonicalEntry(entry, keys) {
  const out = ordered(entry, keys);
  within(out, 'sets', each(canonicalSetItem));
  within(out, 'suggest', (suggest) => ordered(suggest, SUGGEST_KEYS));
  within(out, 'surfaces', (surfaces) => ordered(surfaces, SURFACES_KEYS));
  return out;
}

/** @param {any} decision */
export const canonicalDecision = (decision) => canonicalEntry(decision, DECISION_KEYS);
/** @param {any} feature */
export const canonicalFeature = (feature) => canonicalEntry(feature, FEATURE_KEYS);

/** @param {any} order */
export function canonicalOrder(order) {
  return within(ordered(order, ORDER_KEYS), 'stages', each((stage) => ordered(stage, STAGE_KEYS)));
}
