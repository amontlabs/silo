// The three markers the bot writes on GitHub, and their readers.
//   <!-- silo-entry: <id> -->         last line of an entry's issue body
//   <!-- silo-synced: <40 hex> -->    the commit the issue was last synced at
//   <!-- silo-change: {json} -->      last line of a history comment

import { CHANGE_FIELDS, DEFAULT_RAW, DEFAULT_REPO, DEFAULT_WEB, ID_PATTERN, VERDICTS, isDate } from './names.js';

const SHA = /^[0-9a-f]{40}$/;
const ENTRY = /<!-- silo-entry: ([a-z0-9][a-z0-9-]{0,62}) -->/;
const SYNCED = /<!-- silo-synced: ([0-9a-f]{40}) -->/;
const CHANGE = /<!-- silo-change: (\{.*\}) -->\s*$/;

const normalized = (text) => (text ?? '').replace(/\r\n/g, '\n').trim();

/** @param {string} id */
export function entryMarker(id) {
  if (!ID_PATTERN.test(id)) throw new Error(`not a feature id: ${id}`);
  return `<!-- silo-entry: ${id} -->`;
}

/** @param {string | null | undefined} body @returns {string | null} the id of the entry marker ending the body. */
export function readEntryMarker(body) {
  const lines = normalized(body).split('\n');
  const match = ENTRY.exec(lines[lines.length - 1]);
  return match && lines[lines.length - 1].trim() === match[0] ? match[1] : null;
}

/** @param {string} commit */
export function syncedMarker(commit) {
  if (!SHA.test(commit)) throw new Error(`not a commit: ${commit}`);
  return `<!-- silo-synced: ${commit} -->`;
}

/** @param {string | null | undefined} body @returns {string | null} */
export function readSyncedMarker(body) {
  return SYNCED.exec(normalized(body))?.[1] ?? null;
}

/** @param {string | null | undefined} body @returns {string} the body without the entry and synced markers. */
export function withoutMarkers(body) {
  return normalized(body)
    .split('\n')
    .filter((line) => !ENTRY.test(line) && !SYNCED.test(line))
    .join('\n')
    .trim();
}

/**
 * @typedef {{ commit: string, before: string, fields: string[], verdict?: import('./status.js').Verdict | null, built?: string }} Change
 * `verdict` is present when the verdict changed (null when cleared); `built` when it was set.
 */

/** @param {Change} change */
function validChange(change) {
  if (!change || typeof change !== 'object' || Array.isArray(change)) return false;
  const keys = Object.keys(change);
  if (!keys.every((key) => ['commit', 'before', 'fields', 'verdict', 'built'].includes(key))) return false;
  if (!SHA.test(change.commit) || !SHA.test(change.before)) return false;
  if (!Array.isArray(change.fields) || change.fields.length === 0) return false;
  if (!change.fields.every((field) => CHANGE_FIELDS.includes(field)) || new Set(change.fields).size !== change.fields.length) return false;
  if (Object.hasOwn(change, 'verdict') && change.verdict !== null && !VERDICTS.includes(change.verdict)) return false;
  if (Object.hasOwn(change, 'built') && !isDate(change.built)) return false;
  return true;
}

/**
 * The bot's history comment. It carries no text from the plan.
 * @param {Change} change @param {{ repo?: string, web?: string }} [where]
 */
export function changeComment(change, { repo = DEFAULT_REPO, web = DEFAULT_WEB } = {}) {
  if (!validChange(change)) throw new Error('not a valid change');
  /** @type {Record<string, unknown>} */
  const marker = { commit: change.commit, before: change.before, fields: [...change.fields] };
  if (Object.hasOwn(change, 'verdict')) marker.verdict = change.verdict;
  if (Object.hasOwn(change, 'built')) marker.built = change.built;
  return [
    `Updated in ${change.commit.slice(0, 7)}: ${change.fields.join(', ')}`,
    compareLink(change, { repo, web }),
    `<!-- silo-change: ${JSON.stringify(marker)} -->`,
  ].join('\n');
}

/** @param {string | null | undefined} body @returns {Change | null} the change a history comment records, or null. */
export function readChange(body) {
  const match = CHANGE.exec(normalized(body));
  if (!match) return null;
  try {
    const change = JSON.parse(match[1]);
    return validChange(change) ? change : null;
  } catch {
    return null;
  }
}

/** @param {{ commit: string, before: string }} change @param {{ repo?: string, web?: string }} [where] the change on GitHub. */
export function compareLink(change, { repo = DEFAULT_REPO, web = DEFAULT_WEB } = {}) {
  if (!SHA.test(change.commit) || !SHA.test(change.before)) throw new Error('not a commit');
  return `${web}/${repo}/compare/${change.before}...${change.commit}`;
}

/** @param {string} commit @param {string} id @param {{ repo?: string, raw?: string }} [where] a feature's file at a commit, as plain text. */
export function featureFileAt(commit, id, { repo = DEFAULT_REPO, raw = DEFAULT_RAW } = {}) {
  if (!SHA.test(commit)) throw new Error(`not a commit: ${commit}`);
  if (!ID_PATTERN.test(id)) throw new Error(`not a feature id: ${id}`);
  return `${raw}/${repo}/${commit}/plan/features/${id}.json`;
}
