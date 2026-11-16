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
import { joinFeatures, parseFeatures, setField } from './jsonl.js';
import { ENTRY_MARKER, KIND_LABELS, VERDICTS, verdictLabel } from './names.js';

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
  const path = featuresPath(root);
  const lines = parseFeatures(readText(path));
  const byNumber = new Map(entries.map((issue) => [issue.number, issue]));
  const byMarker = new Map();
  for (const issue of entries) {
    const marker = (issue.body ?? '').match(ENTRY_MARKER);
    if (marker && !byMarker.has(marker[1])) byMarker.set(marker[1], issue);
  }
  const verdicts = [];
  for (const line of lines) {
    const entry = line.entry;
    if (!entry) continue;
    const issue = entry.issue != null ? byNumber.get(entry.issue) : byMarker.get(entry.id);
    if (!issue) continue;
    const { verdict, conflict } = githubVerdict(issue);
    const built = issue.state === 'closed' && issue.state_reason === 'completed';
    if (conflict) {
      verdicts.push({ id: entry.id, issue: issue.number, file: entry.verdict ?? null, github: null, conflict, confirmed: false });
      continue;
    }
    if (verdict == null || verdict === (entry.verdict ?? null)) continue;
    const by = await verdictAuthor(gh, issue, verdict);
    const confirmed = Boolean(by) && (await canDecide(by));
    verdicts.push({ id: entry.id, issue: issue.number, file: entry.verdict ?? null, github: verdict, built, by: by?.login ?? null, confirmed, line });
  }

  // Decisions closed on GitHub that DECISIONS.md does not reference yet.
  const referenced = referencedIssues(readText(decisionsPath(root)));
  const decided = [];
  for (const issue of decisions) {
    if (issue.state !== 'closed' || issue.state_reason !== 'completed' || referenced.has(issue.number)) continue;
    const comments = await gh.paginate(gh.repoPath(`/issues/${issue.number}/comments`));
    const closedAt = Date.parse(issue.closed_at ?? '') || Infinity;
    const closing = comments
      .filter((c) => !gh.isBot(c.user) && Date.parse(c.created_at) <= closedAt + 60_000)
      .at(-1);
    decided.push({
      issue: issue.number,
      title: issue.title,
      closedAt: issue.closed_at,
      outcome: closing ? { author: closing.user.login, text: closing.body } : null,
    });
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
  if (write) {
    for (const v of verdicts) {
      if (!v.confirmed) continue;
      v.line.raw = setField(v.line.raw, 'verdict', v.github);
      v.line.entry = JSON.parse(v.line.raw);
      written += 1;
    }
    if (written) writeText(path, joinFeatures(lines));
    mkdirSync(stateDir(home), { recursive: true, mode: 0o700 });
    writeText(lastPullPath(home), `${now.toISOString()}\n`, 0o600);
  }
  return {
    since,
    verdicts: verdicts.map(({ line, ...rest }) => rest),
    decisions: decided,
    suggestions: suggested,
    comments,
    written: write ? written : null,
  };
}

/** Who last set the verdict: the latest "labeled" event for its label, or for a drop without a label, the latest "closed" event. */
async function verdictAuthor(gh, issue, verdict) {
  const events = await gh.paginate(gh.repoPath(`/issues/${issue.number}/events`));
  const label = verdictLabel(verdict);
  const hasLabel = labelNames(issue).includes(label);
  const match = events.filter((e) => (hasLabel ? e.event === 'labeled' && e.label?.name === label : e.event === 'closed')).at(-1);
  return match?.actor ?? null;
}
