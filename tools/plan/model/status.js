// The status of an entry, from the files alone. First match wins.

/** @typedef {'take' | 'drop'} Verdict */
/** @typedef {'dropped' | 'taken' | 'answered' | 'open'} DecisionStatus */
/** @typedef {'built' | 'dropped' | 'ready' | 'taken' | 'undecided'} FeatureStatus */
/** @typedef {{ kind: 'decision' | 'feature', verdict?: Verdict, answer?: string, built?: string, needs?: string[] }} Entry */

export const DECISION_STATUSES = Object.freeze(['dropped', 'taken', 'answered', 'open']);
export const FEATURE_STATUSES = Object.freeze(['built', 'dropped', 'ready', 'taken', 'undecided']);

/**
 * Done: a decision that is taken, a feature that is built. An id that names no entry is not done.
 * @param {Entry | undefined} entry
 */
export function isDone(entry) {
  if (!entry) return false;
  return entry.kind === 'decision' ? entry.verdict === 'take' : Boolean(entry.built);
}

/**
 * The needs of an entry that are not done, in the entry's order.
 * @param {Entry} entry @param {Record<string, Entry>} entries the plan's entries, by id
 * @returns {string[]}
 */
export function waitingFor(entry, entries) {
  const needs = Array.isArray(entry.needs) ? entry.needs : [];
  return needs.filter((id) => !isDone(Object.hasOwn(entries, id) ? entries[id] : undefined));
}

/**
 * Decision: dropped, taken, answered, open. Feature: built, dropped, ready (taken and every need
 * done), taken, undecided.
 * @param {Entry} entry @param {Record<string, Entry>} entries the plan's entries, by id
 * @returns {DecisionStatus | FeatureStatus}
 */
export function entryStatus(entry, entries) {
  if (entry.kind === 'decision') {
    if (entry.verdict === 'drop') return 'dropped';
    if (entry.verdict === 'take') return 'taken';
    if (entry.answer !== undefined) return 'answered';
    return 'open';
  }
  if (entry.built) return 'built';
  if (entry.verdict === 'drop') return 'dropped';
  if (entry.verdict === 'take') return waitingFor(entry, entries).length ? 'taken' : 'ready';
  return 'undecided';
}
