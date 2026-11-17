// `plan pull`: makes each feature's verdict equal to the verdict that counts on its issue. The only
// path from a label to a file. A label counts when the newest `labeled` event for it is by a person
// with write, maintain or admin permission, or by the Workshop bot itself (`plan decide`). A verdict
// is removed from a file only on a counted removal: an issue with no verdict event at all (one that
// was replaced) never clears it. Only the three verdict labels count, in any capitalisation.
import { execFileSync } from 'node:child_process';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { labelNames } from './client.js';
import { entryIssues, eventsOf } from './issues.js';
import { labelVerdicts } from '../model/comments.js';
import { canonicalFeature, canonicalText } from '../model/canonical.js';
import { labelVerdict, verdictLabel } from '../model/names.js';

const DECIDERS = new Set(['admin', 'maintain', 'write']);
const isVerdictEvent = (event) => (event.event === 'labeled' || event.event === 'unlabeled') && labelVerdict(event.label?.name ?? '') !== null;

/** @param {import('./client.js').GitHub} gh */
export function decider(gh) {
  /** @type {Map<string, boolean>} */
  const known = new Map();
  /** Whether the actor of an event counts: the bot itself, or a person who can decide. */
  return async (actor, event) => {
    if (!actor) return false;
    if (gh.isOwnBot(actor, event)) return true;
    if (gh.isBot(actor)) return false;
    if (!known.has(actor.login)) {
      const found = await gh.request('GET', gh.repoPath(`/collaborators/${encodeURIComponent(actor.login)}/permission`), { allow: [404] });
      known.set(actor.login, DECIDERS.has(found.data?.role_name) || DECIDERS.has(found.data?.permission));
    }
    return /** @type {boolean} */ (known.get(actor.login));
  };
}

/**
 * @typedef {{ id: string, issue: number, from: string | null, to: string | null, by: string | null, at: string | null }} Change
 * @typedef {{ id: string, issue: number, file: string | null, github: string[], why: string }} Report
 */

/**
 * @param {{ gh: import('./client.js').GitHub, root: string, plan: any, raw: any }} options
 * @returns {Promise<{ changes: Change[], reports: Report[], duplicates: any[] }>}
 */
export async function readVerdicts({ gh, root: _root, plan, raw }) {
  const index = await entryIssues(gh);
  const counts = decider(gh);
  /** @type {Change[]} */
  const changes = [];
  /** @type {Report[]} */
  const reports = [];
  for (const id of plan.stages.flatMap((stage) => stage.features)) {
    const data = raw.features[id];
    const issue = index.byId.get(id);
    if (!data || !issue) continue;
    const file = data.verdict ?? null;
    const labels = labelVerdicts(labelNames(issue));
    const report = (why) => reports.push({ id, issue: issue.number, file, github: labels, why });
    if (labels.length > 1) {
      report(`several verdict labels (${labels.map(verdictLabel).join(', ')}); the file is left alone`);
      continue;
    }
    const target = labels[0] ?? null;
    if (target === file) continue;
    if (data.built) {
      report(`built on ${data.built}: its verdict never changes, and its issue shows ${target ? verdictLabel(target) : 'no verdict label'}; the file is left alone`);
      continue;
    }
    const events = (await eventsOf(gh, issue.number)).filter(isVerdictEvent);
    if (target) {
      const event = events.filter((e) => e.event === 'labeled' && labelVerdict(e.label.name) === target).at(-1);
      if (event && (await counts(event.actor, event))) changes.push({ id, issue: issue.number, from: file, to: target, by: event.actor.login, at: event.created_at ?? null });
      else report(`${verdictLabel(target)} was set by ${event?.actor?.login ?? 'someone unknown'}, who cannot decide; the file is left alone`);
      continue;
    }
    // No verdict label while the file has one: only a counted removal clears it.
    const event = events.at(-1);
    if (!event) report(`the file says ${file}, GitHub has no verdict: was the issue replaced? The file is left alone`);
    else if (event.event !== 'unlabeled') report(`GitHub shows no verdict label but its newest verdict event adds ${event.label.name}; the file is left alone`);
    else if (await counts(event.actor, event)) changes.push({ id, issue: issue.number, from: file, to: null, by: event.actor.login, at: event.created_at ?? null });
    else report(`its verdict label was removed by ${event.actor?.login ?? 'someone unknown'}, who cannot decide; the file is left alone`);
  }
  return { changes, reports, duplicates: index.duplicates };
}

/** The commit message of `pull --commit`. @param {Change[]} changes */
export function commitMessage(changes) {
  const word = (change) => `${change.to ?? 'clear'} ${change.id}`;
  let subject = `Owner: ${changes.map(word).join(', ')}`;
  if (subject.length > 72) subject = `${subject.slice(0, 71)}…`;
  const body = changes.map((change) => `${word(change)} (#${change.issue}): ${change.by ? `by ${change.by}` : 'never set on GitHub'}${change.at ? ` at ${change.at}` : ''}`);
  return { subject, body: body.join('\n') };
}

/**
 * Applies the changes to the feature files, each only over the verdict read at the start.
 * @param {string} root @param {Change[]} changes
 * @returns {{ written: Change[], skipped: string[] }}
 */
export function writeVerdicts(root, changes) {
  /** @type {Change[]} */
  const written = [];
  /** @type {string[]} */
  const skipped = [];
  for (const change of changes) {
    const path = join(root, 'plan', 'features', `${change.id}.json`);
    let data;
    try {
      data = JSON.parse(readFileSync(path, 'utf8'));
    } catch {
      skipped.push(`${change.id}: its file could not be read again; left alone`);
      continue;
    }
    if ((data.verdict ?? null) !== change.from || data.built) {
      skipped.push(`${change.id}: its file changed during the run; left alone`);
      continue;
    }
    const next = { ...data };
    if (change.to) next.verdict = change.to;
    else delete next.verdict;
    writeFileSync(path, canonicalText(canonicalFeature(next)));
    written.push(change);
  }
  return { written, skipped };
}

/** Commits the written files. @param {string} root @param {Change[]} written @returns {string} the commit */
export function commitVerdicts(root, written) {
  const { subject, body } = commitMessage(written);
  const paths = written.map((change) => `plan/features/${change.id}.json`);
  execFileSync('git', ['-C', root, 'commit', '--quiet', '-m', subject, '-m', body, '--', ...paths], { stdio: ['ignore', 'pipe', 'pipe'] });
  return execFileSync('git', ['-C', root, 'rev-parse', 'HEAD'], { stdio: ['ignore', 'pipe', 'pipe'] }).toString().trim();
}
