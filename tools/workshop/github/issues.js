// What `decline` reads and writes on one issue: its comments and labels.
import { ENTRY_LABEL, SUGGESTION_LABEL } from '../model/names.js';
import { labelNames } from './client.js';

/** @typedef {import('./client.js').GitHub} GitHub */

/** @param {GitHub} gh @param {number} number @param {string} body */
export async function comment(gh, number, body) {
  const { data } = await gh.request('POST', gh.repoPath(`/issues/${number}/comments`), { body: { body } });
  return data;
}

/** @param {GitHub} gh @param {number} number */
export const commentsOf = (gh, number) => gh.paginate(gh.repoPath(`/issues/${number}/comments`));

export const isSuggestion = (issue) => labelNames(issue).includes(SUGGESTION_LABEL);
export const isEntryLabelled = (issue) => labelNames(issue).includes(ENTRY_LABEL);
