// The commands that post as the bot on one issue: decide, start, stop, decline.
import { labelNames } from './client.js';
import { Failure, NoIssue } from './errors.js';
import { addLabel, builtOnGitHub, claimBranch, comment, commentsOf, entryIssue, eventsOf, isBuilding, isSuggestion, removeLabel } from './issues.js';
import { CLEARED_COMMENT, RECORDED_COMMENT, buildingComment, labelVerdicts, readBuildingComment } from '../model/comments.js';
import { BUILDING_LABEL, ENTRY_LABEL, verdictLabel } from '../model/names.js';

/** @typedef {import('./client.js').GitHub} GitHub */

/** @param {GitHub} gh @param {string} id */
async function needIssue(gh, id) {
  const issue = await entryIssue(gh, id);
  if (!issue) throw new NoIssue(`${id} has no issue on GitHub yet; plan sync creates it after the feature is on main`);
  return issue;
}

/**
 * Sets the verdict label (or removes every verdict label for `clear`) and says so in a comment.
 * The new label goes on before the others come off, so the issue never shows no verdict between.
 * @param {{ gh: GitHub, id: string, verdict: 'take' | 'later' | 'drop' | 'clear' }} options
 */
export async function decide({ gh, id, verdict }) {
  const issue = await needIssue(gh, id);
  if (builtOnGitHub(issue)) throw new Failure(`${id} is built (#${issue.number} is closed as completed); its verdict never changes`);
  const present = labelVerdicts(labelNames(issue));
  const wanted = verdict === 'clear' ? [] : [verdict];
  if (present.length === wanted.length && present.every((v, i) => v === wanted[i])) return { id, issue: issue.number, verdict, changed: false };
  if (verdict !== 'clear' && !present.includes(verdict)) await addLabel(gh, issue.number, verdictLabel(verdict));
  for (const other of present.filter((v) => v !== verdict)) await removeLabel(gh, issue.number, verdictLabel(other));
  await comment(gh, issue.number, verdict === 'clear' ? CLEARED_COMMENT : RECORDED_COMMENT);
  return { id, issue: issue.number, verdict, changed: true };
}

/**
 * Claims a feature: sets the building label, comments `Being built on <branch>.`, then reads the
 * issue again. When an older claim for another branch was posted since the label was set, this
 * session lost the race: its comment is removed and it refuses.
 * @param {{ gh: GitHub, id: string, branch: string }} options
 */
export async function start({ gh, id, branch }) {
  const issue = await needIssue(gh, id);
  if (issue.state !== 'open') throw new Failure(`#${issue.number} (${id}) is closed`);
  if (isBuilding(issue)) {
    const other = claimBranch(gh, await commentsOf(gh, issue.number));
    throw new Failure(`${id} is being built already${other ? ` on ${other}` : ''} (#${issue.number}); plan stop ${id} releases it`);
  }
  const text = buildingComment(branch);
  await addLabel(gh, issue.number, BUILDING_LABEL);
  const mine = await comment(gh, issue.number, text);

  const labeled = (await eventsOf(gh, issue.number)).filter((e) => e.event === 'labeled' && e.label?.name === BUILDING_LABEL).at(-1);
  const since = labeled?.created_at ? Date.parse(labeled.created_at) : -Infinity;
  const claims = (await commentsOf(gh, issue.number)).filter((c) => gh.isOwnBot(c.user, c) && readBuildingComment(c.body) && Date.parse(c.created_at) >= since);
  const first = claims.sort((a, b) => Date.parse(a.created_at) - Date.parse(b.created_at) || a.id - b.id)[0];
  if (first && first.id !== mine.id && readBuildingComment(first.body) !== branch) {
    await gh.request('DELETE', gh.repoPath(`/issues/comments/${mine.id}`), { allow: [404] });
    throw new Failure(`another session claimed ${id} first, on ${readBuildingComment(first.body)} (#${issue.number}); this claim is withdrawn`);
  }
  return { id, issue: issue.number, branch, claimed: true };
}

/** Removes the building label. @param {{ gh: GitHub, id: string }} options */
export async function stop({ gh, id }) {
  const issue = await needIssue(gh, id);
  if (!isBuilding(issue)) return { id, issue: issue.number, changed: false };
  await removeLabel(gh, issue.number, BUILDING_LABEL);
  return { id, issue: issue.number, changed: true };
}

/** Closes an open suggestion as not planned, with the reason as the bot's comment. @param {{ gh: GitHub, number: number, why: string }} options */
export async function decline({ gh, number, why }) {
  const issue = await gh.issue(number);
  if (!issue || !isSuggestion(issue) || labelNames(issue).includes(ENTRY_LABEL)) throw new Failure(`#${number} is not a suggestion`);
  if (issue.state !== 'open') throw new Failure(`#${number} is closed already`);
  const posted = (await commentsOf(gh, number)).some((c) => gh.isOwnBot(c.user, c) && c.body === why);
  if (!posted) await comment(gh, number, why);
  await gh.request('PATCH', gh.repoPath(`/issues/${number}`), { body: { state: 'closed', state_reason: 'not_planned' } });
  return { number, closed: true };
}
