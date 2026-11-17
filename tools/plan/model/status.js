// A feature's status, from its file and, when known, the live state of its issue.

/** @typedef {'take' | 'later' | 'drop'} Verdict */
/** @typedef {'built' | 'dropped' | 'building' | 'planned' | 'later' | 'undecided'} Status */
/**
 * The live state of an entry's issue that the status reads.
 * @typedef {{ verdict: Verdict | null, building: boolean }} LiveStatus
 */

export const STATUSES = Object.freeze(['built', 'dropped', 'building', 'planned', 'later', 'undecided']);

/**
 * The verdict that counts: the issue's when it has a verdict label, else the file's.
 * @param {{ verdict?: Verdict }} feature @param {LiveStatus | null | undefined} live
 * @returns {Verdict | null}
 */
export function countedVerdict(feature, live) {
  return live?.verdict ?? feature.verdict ?? null;
}

/**
 * First match wins: built, dropped, building, planned, later, undecided.
 * @param {{ verdict?: Verdict, built?: string }} feature @param {LiveStatus | null | undefined} [live]
 * @returns {Status}
 */
export function featureStatus(feature, live) {
  if (feature.built) return 'built';
  const verdict = countedVerdict(feature, live);
  if (verdict === 'drop') return 'dropped';
  if (verdict === 'take' && live?.building) return 'building';
  if (verdict === 'take') return 'planned';
  if (verdict === 'later') return 'later';
  return 'undecided';
}

/**
 * Ready to build: planned, and every need built.
 * @param {{ needs: string[] }} feature @param {Status} status @param {(id: string) => Status} statusOf
 */
export function isReady(feature, status, statusOf) {
  return status === 'planned' && feature.needs.every((id) => statusOf(id) === 'built');
}

/** @param {{ needs: string[] }} feature @param {(id: string) => Status} statusOf @returns {string[]} the needs not built yet. */
export function waitingFor(feature, statusOf) {
  return feature.needs.filter((id) => statusOf(id) !== 'built');
}
