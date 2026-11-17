// `plan sync`: makes GitHub follow the plan at origin/main. Never reads the working tree, never
// writes a verdict label. Each issue carries the commit it was last synced at (`silo-synced`), so
// a run stopped half way is resumed by the next one and a second run changes nothing.
import { execFileSync } from 'node:child_process';
import { checkFiles } from '../check/index.js';
import { comment as postComment, commentsOf, ensureLabels, entryIssues, eventsOf, isBuilding, isSuggestion, removeLabel } from './issues.js';
import { labelNames } from './client.js';
import { Failure, RateLimited } from './errors.js';
import { placedComment, labelVerdicts } from '../model/comments.js';
import { entryLink, wantedIssue } from '../model/issue.js';
import { changeComment, readChange, readSyncedMarker, withoutMarkers } from '../model/markers.js';
import { BUILDING_LABEL, CHANGE_FIELDS, ENTRY_LABEL, FEATURE_KEYS } from '../model/names.js';
import { loadRef, parsePlan } from '../read/index.js';

/** @param {string} root @param {string[]} args */
function git(root, args) {
  return execFileSync('git', ['-C', root, ...args], { stdio: ['ignore', 'pipe', 'pipe'], maxBuffer: 256 * 1024 * 1024 }).toString();
}

/** @param {string} root @param {string[]} args */
function gitOk(root, args) {
  try {
    git(root, args);
    return true;
  } catch {
    return false;
  }
}

/**
 * The fields of one feature that differ between two commits: its keys, and `notes` for its `.md`.
 * @param {Map<string, string>} before @param {Map<string, string>} after @param {string} id
 */
export function changedFields(before, after, id) {
  const parse = (files) => {
    try {
      const value = JSON.parse(files.get(`features/${id}.json`) ?? '{}');
      return value && typeof value === 'object' && !Array.isArray(value) ? value : {};
    } catch {
      return {};
    }
  };
  const old = parse(before);
  const now = parse(after);
  const fields = FEATURE_KEYS.filter((key) => JSON.stringify(old[key]) !== JSON.stringify(now[key]));
  if ((before.get(`features/${id}.md`) ?? null) !== (after.get(`features/${id}.md`) ?? null)) fields.push('notes');
  /** @type {import('../model/markers.js').Change} */
  const change = { commit: '', before: '', fields: CHANGE_FIELDS.filter((field) => fields.includes(field)) };
  if (fields.includes('verdict')) change.verdict = now.verdict ?? null;
  if (fields.includes('built') && now.built) change.built = now.built;
  return change;
}

/**
 * @param {{ gh: import('./client.js').GitHub, root: string, dryRun: boolean, log: (text: string) => void, site: string, web: string, repo: string }} options
 */
export async function sync({ gh, root, dryRun, log, site, web, repo }) {
  if (!gitOk(root, ['fetch', '--quiet', 'origin', 'main'])) throw new Failure('git fetch origin main failed; sync reads the plan at origin/main');
  const synced = git(root, ['rev-parse', '--verify', 'origin/main^{commit}']).trim();
  const files = loadRef(root, synced);
  const { errors } = checkFiles(files);
  if (errors.length) throw new Failure(`the plan at origin/main (${synced.slice(0, 7)}) fails plan check (${errors.length} error${errors.length === 1 ? '' : 's'}); nothing synced`);
  const { plan } = parsePlan(files);

  const counts = { labels: 0, created: 0, updated: 0, closed: 0, reopened: 0, history: 0, unchanged: 0, suggestions: 0 };
  /** @type {string[]} */
  const actions = [];
  /** @type {string[]} */
  const notes = [];
  const act = (text) => {
    actions.push(text);
    if (dryRun) log(`would ${text}`);
  };
  /** @type {Map<string, Map<string, string>>} */
  const atCommit = new Map([[synced, files]]);
  const filesAt = (commit) => {
    if (!atCommit.has(commit)) atCommit.set(commit, loadRef(root, commit));
    return /** @type {Map<string, string>} */ (atCommit.get(commit));
  };
  const reachable = (commit) => commit === synced || gitOk(root, ['merge-base', '--is-ancestor', commit, synced]);

  let stopped = null;
  let duplicates = [];
  try {
    counts.labels = await ensureLabels({ gh, dryRun, act });
    const index = await entryIssues(gh);
    duplicates = index.duplicates;
    /** @type {Map<string, number>} */
    const numbers = new Map();
    for (const id of plan.stages.flatMap((stage) => stage.features)) {
      const feature = plan.features[id];
      const issue = index.byId.get(id);
      if (!issue) {
        const wanted = wantedIssue(feature, { synced, site });
        act(`create the issue of ${id}`);
        counts.created += 1;
        if (dryRun) continue;
        const { data } = await gh.request('POST', gh.repoPath('/issues'), { body: { title: wanted.title, body: wanted.body, labels: [ENTRY_LABEL] } });
        numbers.set(id, data.number);
        if (wanted.state === 'closed') {
          await gh.request('PATCH', gh.repoPath(`/issues/${data.number}`), { body: { state: 'closed', state_reason: wanted.stateReason } });
          counts.closed += 1;
        }
        continue;
      }
      numbers.set(id, issue.number);
      await syncEntry({ gh, id, feature, issue, synced, site, web, repo, dryRun, act, counts, notes, filesAt, reachable });
    }
    await closeSuggestions({ gh, plan, numbers, site, dryRun, act, counts, notes });
  } catch (error) {
    if (!(error instanceof RateLimited)) throw error;
    stopped = error;
  }
  return { dryRun, synced, ...counts, duplicates, notes, actions, stopped: stopped ? (stopped.resetAt?.toISOString() ?? 'unknown') : null, stoppedMessage: stopped?.message ?? null };
}

/** One existing entry issue: history, title, body, state, building label. */
async function syncEntry({ gh, id, feature, issue, synced, site, web, repo, dryRun, act, counts, notes, filesAt, reachable }) {
  const before = readSyncedMarker(issue.body);
  let marker = before ?? synced;
  /** @type {import('../model/markers.js').Change | null} */
  let change = null;
  if (before !== synced) {
    if (!before || !reachable(before)) marker = synced;
    else {
      const found = changedFields(filesAt(before), filesAt(synced), id);
      if (found.fields.length) {
        change = { ...found, commit: synced, before };
        marker = synced;
      }
    }
  }
  if (change) {
    const posted = (await commentsOf(gh, issue.number)).some((c) => gh.isOwnBot(c.user, c) && readChange(c.body)?.commit === synced);
    if (!posted) {
      act(`post the history of ${id} on #${issue.number} (${change.fields.join(', ')})`);
      counts.history += 1;
      if (!dryRun) await postComment(gh, issue.number, changeComment(change, { repo, web }));
    }
  }

  let wanted = wantedIssue(feature, { synced: marker, site });
  const textChanged = issue.title !== wanted.title || withoutMarkers(issue.body) !== withoutMarkers(wanted.body);
  if (textChanged && marker !== synced) {
    marker = synced;
    wanted = wantedIssue(feature, { synced: marker, site });
  }
  /** @type {Record<string, unknown>} */
  const patch = {};
  if (textChanged || marker !== before) Object.assign(patch, { title: wanted.title, body: wanted.body });

  // The verdict on the issue that pull has not brought to the file yet: the state waits for it.
  const labels = labelVerdicts(labelNames(issue));
  const pending = !feature.built && !(labels.length === (feature.verdict ? 1 : 0) && (labels[0] ?? undefined) === feature.verdict);
  const stateDiffers = issue.state !== wanted.state || (wanted.state === 'closed' && issue.state_reason !== wanted.stateReason);
  if (stateDiffers && pending) notes.push(`#${issue.number} ${id}: its verdict on GitHub is not in the file yet; its state waits for plan pull`);
  else if (stateDiffers) {
    patch.state = wanted.state;
    if (wanted.state === 'closed') patch.state_reason = wanted.stateReason;
    else patch.state_reason = 'reopened';
  }

  if (Object.keys(patch).length) {
    const what = [];
    if (patch.title !== undefined && textChanged) what.push('text');
    if (patch.body !== undefined && marker !== before) what.push('marker');
    if (patch.state === 'closed') what.push(`close as ${wanted.stateReason === 'completed' ? 'completed' : 'not planned'}`);
    if (patch.state === 'open') what.push('reopen');
    act(`update #${issue.number} ${id} (${what.join(', ')})`);
    if (patch.state === 'closed') counts.closed += 1;
    else if (patch.state === 'open') counts.reopened += 1;
    else counts.updated += 1;
    if (!dryRun) await gh.request('PATCH', gh.repoPath(`/issues/${issue.number}`), { body: patch });
  } else if (!change) counts.unchanged += 1;

  const closedNow = patch.state === 'closed' || (issue.state === 'closed' && patch.state === undefined && wanted.state === 'closed');
  if (closedNow && isBuilding(issue)) {
    act(`remove ${BUILDING_LABEL} from #${issue.number} ${id}`);
    if (!dryRun) await removeLabel(gh, issue.number, BUILDING_LABEL);
  }
}

/** A suggestion a feature names in `suggested_in` is closed with a link to the entry, once. */
async function closeSuggestions({ gh, plan, numbers, site, dryRun, act, counts, notes }) {
  for (const id of plan.stages.flatMap((stage) => stage.features)) {
    const feature = plan.features[id];
    if (feature.suggested_in === undefined) continue;
    const number = feature.suggested_in;
    const entry = numbers.get(id);
    if (entry === undefined) continue;
    const issue = await gh.issue(number);
    if (!issue || !isSuggestion(issue)) {
      notes.push(`${id}: suggested_in #${number} is not a suggestion; left alone`);
      continue;
    }
    if (issue.state !== 'open') continue;
    // Closed once by the bot and reopened by someone: left as they want it.
    if ((await eventsOf(gh, number)).some((event) => event.event === 'closed' && gh.isOwnBot(event.actor, event))) continue;
    const text = placedComment(entry, entryLink(feature, site));
    const posted = (await commentsOf(gh, number)).some((c) => gh.isOwnBot(c.user, c) && c.body === text);
    act(`close the suggestion #${number}, placed as ${id} (#${entry})`);
    counts.suggestions += 1;
    if (dryRun) continue;
    if (!posted) await postComment(gh, number, text);
    await gh.request('PATCH', gh.repoPath(`/issues/${number}`), { body: { state: 'closed', state_reason: 'completed' } });
  }
}
