// What an entry's issue should look like on GitHub, from its feature file.

import { DEFAULT_SITE } from './names.js';
import { entryMarker, syncedMarker } from './markers.js';

/** @param {{ id: string, area?: string }} feature @param {string} [site] the entry's address on the Workshop. */
export function entryLink(feature, site = DEFAULT_SITE) {
  return `${site}/workshop#${feature.area ?? 'left'}/${feature.id}`;
}

/**
 * Title, body and state of an entry's issue. Closed as completed when built, as not planned
 * when the verdict is drop, open otherwise.
 * @param {{ id: string, name: string, what: string, area?: string, suggested_in?: number, verdict?: string, built?: string }} feature
 * @param {{ synced: string, site?: string }} options `synced`: the commit the plan was read at.
 * @returns {{ title: string, body: string, state: 'open' | 'closed', stateReason: null | 'completed' | 'not_planned' }}
 */
export function wantedIssue(feature, { synced, site = DEFAULT_SITE }) {
  const lines = [feature.what, '', `On the Workshop: ${entryLink(feature, site)}`];
  if (feature.suggested_in !== undefined) lines.push('', `Suggested in #${feature.suggested_in}`);
  lines.push('', syncedMarker(synced), entryMarker(feature.id));
  const state = feature.built ? 'closed' : feature.verdict === 'drop' ? 'closed' : 'open';
  const stateReason = feature.built ? 'completed' : feature.verdict === 'drop' ? 'not_planned' : null;
  return { title: feature.name, body: lines.join('\n'), state, stateReason };
}
