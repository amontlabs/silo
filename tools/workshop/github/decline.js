// `workshop decline`: closes a visitor's suggestion as not planned, with the reason as the bot's comment.
import { Failure } from './errors.js';
import { comment, commentsOf, isEntryLabelled, isSuggestion } from './issues.js';

/** @typedef {import('./client.js').GitHub} GitHub */

/**
 * Refuses an issue that is not an open suggestion. The reason is posted once: a run stopped after the
 * comment and run again closes the issue without a second comment.
 * @param {{ gh: GitHub, number: number, why: string }} options
 */
export async function decline({ gh, number, why }) {
  const issue = await gh.issue(number);
  if (!issue || !isSuggestion(issue) || isEntryLabelled(issue)) throw new Failure(`#${number} is not a suggestion`);
  if (issue.state !== 'open') throw new Failure(`#${number} is closed already`);
  const posted = (await commentsOf(gh, number)).some((c) => gh.isOwnBot(c.user, c) && c.body === why);
  if (!posted) await comment(gh, number, why);
  await gh.request('PATCH', gh.repoPath(`/issues/${number}`), { body: { state: 'closed', state_reason: 'not_planned' } });
  return { number, closed: true };
}
