// Finding the plan's issues on GitHub: an entry's issue is one the bot created whose body ends with
// the entry marker. They are found among every issue the bot created, not by the workshop:entry
// label, which anyone with triage rights can remove; sync puts the label back.
import { BUILDING_LABEL, ENTRY_LABEL, LABELS, SUGGESTION_LABEL } from '../model/names.js';
import { readBuildingComment } from '../model/comments.js';
import { readEntryMarker } from '../model/markers.js';
import { labelNames, labelOn } from './client.js';
import { Failure } from './errors.js';

/** @typedef {import('./client.js').GitHub} GitHub */

const MARKER_LINE = /^<!-- silo-entry: ([a-z0-9][a-z0-9-]{0,62}) -->$/;

/**
 * The entry id of one of the bot's issues: its last line's marker, else the one marker line in its
 * body (text was added after it on GitHub; sync puts the body back).
 * @param {string | null | undefined} body
 */
function markerOf(body) {
  const strict = readEntryMarker(body);
  if (strict) return strict;
  const ids = (body ?? '').replace(/\r\n/g, '\n').split('\n').map((line) => MARKER_LINE.exec(line.trim())?.[1]).filter(Boolean);
  return ids.length === 1 ? ids[0] : null;
}

/**
 * The bot's entry issues by id, from issues the bot created (others are left out). When two carry
 * one marker, the oldest (lowest number) is used; the open others are duplicates.
 * @param {GitHub} gh @param {any[]} issues
 * @returns {{ byId: Map<string, any>, duplicates: { id: string, kept: number, duplicate: number }[], issues: Map<number, any> }}
 */
export function entryIndex(gh, issues) {
  /** @type {Map<string, any>} */
  const byId = new Map();
  /** @type {Map<number, any>} */
  const byNumber = new Map();
  /** @type {{ id: string, kept: number, duplicate: number }[]} */
  const duplicates = [];
  for (const issue of [...issues].sort((a, b) => a.number - b.number)) {
    if (issue.pull_request || !gh.isOwnBot(issue.user, issue)) continue;
    const id = markerOf(issue.body);
    if (!id) continue;
    byNumber.set(issue.number, issue);
    const kept = byId.get(id);
    if (!kept) byId.set(id, issue);
    else if (issue.state === 'open') duplicates.push({ id, kept: kept.number, duplicate: issue.number });
  }
  return { byId, duplicates, issues: byNumber };
}

/** @param {GitHub} gh */
export async function entryIssues(gh) {
  return entryIndex(gh, await gh.botIssues());
}

/** @param {GitHub} gh @param {string} id @returns {Promise<any | null>} */
export async function entryIssue(gh, id) {
  return (await entryIssues(gh)).byId.get(id) ?? null;
}

/**
 * Creates each label the repository lacks, with its colour and description. A label that exists
 * under the same name, in any case, is left as it is.
 * @param {{ gh: GitHub, dryRun?: boolean, act?: (text: string) => void, labels?: readonly { name: string, color: string, description: string }[] }} options
 * @returns {Promise<number>} how many it created (or would)
 */
export async function ensureLabels({ gh, dryRun = false, act = () => {}, labels = LABELS }) {
  const existing = new Set((await gh.paginate(gh.repoPath('/labels'))).map((label) => String(label.name).toLowerCase()));
  let made = 0;
  for (const label of labels) {
    if (existing.has(label.name.toLowerCase())) continue;
    act(`create the label ${label.name}`);
    made += 1;
    if (!dryRun) await gh.request('POST', gh.repoPath('/labels'), { body: label, allow: [422] });
  }
  return made;
}

/** Adds one label of LABELS, creating it first when the repository lacks it. @param {GitHub} gh @param {number} number @param {string} name */
export async function addLabel(gh, number, name) {
  await ensureLabels({ gh, labels: LABELS.filter((label) => label.name === name) });
  await gh.request('POST', gh.repoPath(`/issues/${number}/labels`), { body: { labels: [name] } });
}

/**
 * Removes a label from an issue, under the name it has there in whatever capitalisation. A label
 * already gone (404) is fine, unless `strict`: then it is an error.
 * @param {GitHub} gh @param {any} issue @param {string} name @param {{ strict?: boolean }} [options]
 */
export async function removeLabel(gh, issue, name, { strict = false } = {}) {
  const actual = labelOn(issue, name) ?? name;
  const { status } = await gh.request('DELETE', gh.repoPath(`/issues/${issue.number}/labels/${encodeURIComponent(actual)}`), { allow: [404] });
  if (strict && status === 404) throw new Failure(`${actual} could not be removed from #${issue.number}: GitHub does not show it there; nothing is recorded`);
}

/** @param {GitHub} gh @param {number} number @param {string} body */
export async function comment(gh, number, body) {
  const { data } = await gh.request('POST', gh.repoPath(`/issues/${number}/comments`), { body: { body } });
  return data;
}

/** @param {GitHub} gh @param {number} number */
export const commentsOf = (gh, number) => gh.paginate(gh.repoPath(`/issues/${number}/comments`));

/** @param {GitHub} gh @param {number} number */
export const eventsOf = (gh, number) => gh.paginate(gh.repoPath(`/issues/${number}/events`));

/**
 * The branch of the newest claim (`Being built on <branch>.`) the bot posted on an issue, or null.
 * @param {GitHub} gh @param {any[]} comments
 */
export function claimBranch(gh, comments) {
  const claims = comments.filter((c) => gh.isOwnBot(c.user, c) && readBuildingComment(c.body));
  return claims.length ? readBuildingComment(claims[claims.length - 1].body) : null;
}

export const isBuilding = (issue) => labelNames(issue).includes(BUILDING_LABEL);
export const isSuggestion = (issue) => labelNames(issue).includes(SUGGESTION_LABEL);
export const isEntryLabelled = (issue) => labelNames(issue).includes(ENTRY_LABEL);
export const builtOnGitHub = (issue) => issue.state === 'closed' && issue.state_reason === 'completed';
