// `workshop pull`: what GitHub has that the files do not yet. With --write it
// records verdicts in features.jsonl, but only a verdict whose last change on
// GitHub was made by a person with write, maintain or admin permission on the
// repository; anything else is reported as unconfirmed and left alone.
// DECISIONS.md is never written here.
import { mkdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { stateDir } from './credentials.js';
import { referencedIssues } from './decisions.js';
import { decisionsPath, featuresPath, readText, writeText } from './files.js';
import { parseFeatures } from './jsonl.js';
import { decisionTitle } from './decisions.js';
import { decisionIndex, entryIndex, updateFeature } from './match.js';
import { KIND_LABELS, VERDICTS, verdictLabel } from './names.js';

const DECIDERS = new Set(['admin', 'write', 'maintain']);

export const lastPullPath = (home) => join(stateDir(home), 'last-pull');

function readLastPull(home) {
  try {
    const text = readFileSync(lastPullPath(home), 'utf8').trim();
    return Number.isNaN(Date.parse(text)) ? null : text;
  } catch {
    return null;
  }
}

const labelNames = (issue) => (issue.labels ?? []).map((label) => (typeof label === 'string' ? label : label.name));

/** The verdict GitHub shows: one verdict label, else "drop" for an issue closed as not planned. */
export function githubVerdict(issue) {
  const verdicts = VERDICTS.filter((v) => labelNames(issue).includes(verdictLabel(v)));
  if (verdicts.length > 1) return { verdict: null, conflict: verdicts };
  if (verdicts.length === 1) return { verdict: verdicts[0] };
  if (issue.state === 'closed' && issue.state_reason === 'not_planned') return { verdict: 'drop' };
  return { verdict: null };
}

export async function pull({ gh, root, home, write, now = new Date() }) {
  const since = readLastPull(home);
  const [entries, decisions, suggestions] = await Promise.all([
    gh.issuesWithLabel(KIND_LABELS.entry),
    gh.issuesWithLabel(KIND_LABELS.decision),
    gh.issuesWithLabel(KIND_LABELS.suggestion),
  ]);
  const permissions = new Map();
  const canDecide = async (user) => {
    if (gh.isBot(user)) return false;
    if (!permissions.has(user.login)) {
      const found = await gh.request('GET', gh.repoPath(`/collaborators/${encodeURIComponent(user.login)}/permission`), {
        allow: [404],
      });
      const level = found.data ? (found.data.role_name ?? found.data.permission) : 'none';
      permissions.set(user.login, DECIDERS.has(level) || DECIDERS.has(found.data?.permission));
    }
    return permissions.get(user.login);
  };

  // Entries whose verdict on GitHub differs from the file.
  const lines = parseFeatures(readText(featuresPath(root)));
  const index = entryIndex(entries, gh);
  const verdicts = [];
  const unmatched = [];
  for (const { entry } of lines) {
    if (!entry) continue;
    const numbered = entry.issue != null ? index.byNumber.get(entry.issue) : null;
    if (entry.issue != null && numbered?.id !== entry.id) {
      unmatched.push({ id: entry.id, issue: entry.issue, reason: await whyUnmatched(gh, entry, numbered) });
      continue;
    }
    const issue = numbered ? numbered.issue : index.byMarker.get(entry.id);
    if (!issue) continue;
    const { verdict, conflict } = githubVerdict(issue);
    const file = entry.verdict ?? null;
    const built = issue.state === 'closed' && issue.state_reason === 'completed';
    if (conflict) {
      verdicts.push({ id: entry.id, issue: issue.number, file, github: null, conflict, confirmed: false });
      continue;
    }
    if (verdict == null) {
      // A verdict label removed on GitHub is reported, never written: the file keeps its carried-over verdict.
      if (file != null && (await lastVerdictEvent(gh, issue))?.event === 'unlabeled') {
        verdicts.push({ id: entry.id, issue: issue.number, file, github: null, cleared: true, confirmed: false });
      }
      continue;
    }
    if (verdict === file) continue;
    const by = await verdictAuthor(gh, issue, verdict);
    const confirmed = Boolean(by) && (await canDecide(by));
    verdicts.push({ id: entry.id, issue: issue.number, file, github: verdict, built, by: by?.login ?? null, confirmed });
  }

  // Decisions closed on GitHub that DECISIONS.md does not reference yet. One
  // counts as decided only when the person who closed it can decide; the
  // outcome is that person's last comment up to 60 seconds after the close.
  const referenced = referencedIssues(readText(decisionsPath(root)));
  const decided = [];
  const closedWithoutOutcome = [];
  const decisionDuplicates = decisionIndex(decisions, gh, (body) => decisionTitle(body, 60)).duplicates;
  const duplicateDecisions = new Set(decisionDuplicates.map((d) => d.duplicate));
  for (const issue of decisions) {
    if (issue.state !== 'closed' || issue.state_reason !== 'completed' || referenced.has(issue.number)) continue;
    if (duplicateDecisions.has(issue.number)) continue;
    const events = await gh.paginate(gh.repoPath(`/issues/${issue.number}/events`));
    const closer = events.filter((e) => e.event === 'closed').at(-1)?.actor ?? null;
    const row = { issue: issue.number, title: issue.title, closedAt: issue.closed_at, closedBy: closer?.login ?? null };
    let closing = null;
    if (closer && (await canDecide(closer))) {
      const comments = await gh.paginate(gh.repoPath(`/issues/${issue.number}/comments`));
      const closedAt = Date.parse(issue.closed_at ?? '') || Infinity;
      closing = comments.filter((c) => c.user?.login === closer.login && Date.parse(c.created_at) <= closedAt + 60_000).at(-1) ?? null;
    }
    if (closing) decided.push({ ...row, outcome: { author: closing.user.login, text: closing.body } });
    else closedWithoutOutcome.push(row);
  }

  const suggested = suggestions
    .filter((issue) => issue.state === 'open')
    .map((issue) => ({ issue: issue.number, title: issue.title, author: issue.user?.login ?? null, up: issue.reactions?.['+1'] ?? 0 }))
    .sort((a, b) => b.up - a.up || a.issue - b.issue);

  // Comments by people since the last `pull --write`, on Workshop issues only.
  const workshop = new Map([...entries, ...decisions, ...suggestions].map((issue) => [issue.number, issue]));
  const all = await gh.paginate(
    gh.repoPath(`/issues/comments?sort=created&direction=asc${since ? `&since=${encodeURIComponent(since)}` : ''}`),
  );
  const comments = all
    .filter((c) => !since || Date.parse(c.created_at) > Date.parse(since))
    .filter((c) => !gh.isBot(c.user))
    .map((c) => ({ c, issue: Number(String(c.issue_url).split('/').pop()) }))
    .filter(({ issue }) => workshop.has(issue))
    .map(({ c, issue }) => ({ issue, title: workshop.get(issue).title, author: c.user.login, createdAt: c.created_at, text: c.body }));

  let written = 0;
  const skipped = [];
  if (write) {
    for (const v of verdicts) {
      if (!v.confirmed || v.github == null) continue;
      // Written only over the verdict read at the start: a verdict changed meanwhile is left.
      const outcome = updateFeature(root, v.id, 'verdict', v.github, (now) => (now.verdict ?? null) === v.file);
      if (outcome === 'ok') written += 1;
      else if (outcome === 'missing') skipped.push(`${v.id}: its line left features.jsonl during the run`);
      else skipped.push(`${v.id}: its verdict changed in features.jsonl during the run; left as it is`);
    }
    mkdirSync(stateDir(home), { recursive: true, mode: 0o700 });
    writeText(lastPullPath(home), `${now.toISOString()}\n`, 0o600);
  }
  return {
    since,
    verdicts,
    duplicates: [...index.duplicates, ...decisionDuplicates],
    unmatched,
    decisions: decided.sort((a, b) => a.issue - b.issue),
    closedWithoutOutcome: closedWithoutOutcome.sort((a, b) => a.issue - b.issue),
    suggestions: suggested,
    comments,
    written: write ? written : null,
    skipped,
  };
}

/** Why a line's issue is not one of the bot's entry issues, in plain words. */
async function whyUnmatched(gh, entry, numbered) {
  if (numbered) return `it belongs to ${numbered.id}`;
  const found = await gh.request('GET', gh.repoPath(`/issues/${entry.issue}`), { allow: [404, 410] });
  const issue = found.data;
  if (!issue || issue.pull_request) return 'no such issue';
  if (!gh.isOwnBot(issue.user)) return 'the bot did not create it';
  return 'edited on GitHub (text after the marker, or the entry label removed); `workshop sync` restores it';
}

/** The latest labeled or unlabeled event of any verdict label. */
async function lastVerdictEvent(gh, issue) {
  const events = await gh.paginate(gh.repoPath(`/issues/${issue.number}/events`));
  return events.filter((e) => (e.event === 'labeled' || e.event === 'unlabeled') && /^verdict:/.test(e.label?.name ?? '')).at(-1) ?? null;
}

/** Who last set the verdict: the latest "labeled" event for its label, or for a drop without a label, the latest "closed" event. */
async function verdictAuthor(gh, issue, verdict) {
  const events = await gh.paginate(gh.repoPath(`/issues/${issue.number}/events`));
  const label = verdictLabel(verdict);
  const hasLabel = labelNames(issue).includes(label);
  const match = events.filter((e) => (hasLabel ? e.event === 'labeled' && e.label?.name === label : e.event === 'closed')).at(-1);
  return match?.actor ?? null;
}
