// The status of an entry, from the files alone. First match wins.

/** @typedef {'take' | 'drop'} Verdict */
/** @typedef {'dropped' | 'replaced' | 'taken' | 'answered' | 'open'} DecisionStatus */
/** @typedef {'built' | 'dropped' | 'ready' | 'taken' | 'undecided'} FeatureStatus */
/** @typedef {{ id?: string, kind: 'decision' | 'feature', verdict?: Verdict, answer?: string, sets?: unknown, built?: string, needs?: string[], replaces?: unknown }} Entry */

export const DECISION_STATUSES = Object.freeze(['dropped', 'replaced', 'taken', 'answered', 'open']);
export const FEATURE_STATUSES = Object.freeze(['built', 'dropped', 'ready', 'taken', 'undecided']);

/**
 * Whether an entry puts forward statements: a `sets` that is a list of one item or more. A decision
 * whose whole proposal is its statements has no `answer`, and its `sets` stands for one.
 * @param {{ sets?: unknown }} entry
 */
export function hasSets(entry) {
  return Array.isArray(entry.sets) && entry.sets.length > 0;
}

/**
 * The taken decision that names a taken decision in its `replaces`, or null: the decision is
 * replaced and no longer in force. The entry's id is its `id`, or else its key in `entries`. The
 * check allows one such decision at most; with more, the first in `entries` is given.
 * @param {Entry} entry @param {Record<string, Entry>} entries the plan's entries, by id
 * @returns {string | null}
 */
export function replacedBy(entry, entries) {
  if (entry.kind !== 'decision' || entry.verdict !== 'take') return null;
  const id = typeof entry.id === 'string' ? entry.id : Object.keys(entries).find((key) => entries[key] === entry);
  if (id === undefined) return null;
  for (const [other, value] of Object.entries(entries)) {
    if (other === id || !value || value.kind !== 'decision' || value.verdict !== 'take') continue;
    if (Array.isArray(value.replaces) && value.replaces.includes(id)) return other;
  }
  return null;
}

/**
 * Done: a decision that is taken (a replaced one too: it was taken), a feature that is built. An
 * id that names no entry is not done.
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
 * Decision: dropped, replaced (taken, and a taken decision names it in `replaces`), taken, answered
 * (an `answer` or a `sets`), open. Feature: built, dropped, ready (taken and every need
 * done), taken, undecided.
 * @param {Entry} entry @param {Record<string, Entry>} entries the plan's entries, by id
 * @returns {DecisionStatus | FeatureStatus}
 */
export function entryStatus(entry, entries) {
  if (entry.kind === 'decision') {
    if (entry.verdict === 'drop') return 'dropped';
    if (entry.verdict === 'take') return replacedBy(entry, entries) ? 'replaced' : 'taken';
    if (entry.answer !== undefined || hasSets(entry)) return 'answered';
    return 'open';
  }
  if (entry.built) return 'built';
  if (entry.verdict === 'drop') return 'dropped';
  if (entry.verdict === 'take') return waitingFor(entry, entries).length ? 'taken' : 'ready';
  return 'undecided';
}
