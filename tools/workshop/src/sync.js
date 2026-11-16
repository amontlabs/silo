// `workshop sync`: makes GitHub match docs/product and writes issue numbers
// back. Entry issues get their title and body only: sync never sets or
// removes a label other than the kind label on creation and never changes an
// entry's state. Every number is written to disk right after its issue is
// made, so a run stopped by a rate limit resumes without duplicates.
import { decisionTitle, listItems } from './decisions.js';
import { decisionsPath, featuresPath, readText } from './files.js';
import { parseFeatures } from './jsonl.js';
import { entryIndex, referenceDecision, updateFeature, withoutRelativeLinks } from './match.js';
import { Failure, KIND_LABELS, LABELS, RateLimited, entryMarker } from './names.js';

export const entryBody = (entry) => `${entry.what ?? ''}\n\n${entryMarker(entry.id)}`;
export const decisionBody = (text) => withoutRelativeLinks(text);

function normalized(text) {
  return (text ?? '').replace(/\r\n/g, '\n').trim();
}

export async function sync({ gh, root, dryRun, log }) {
  const counts = { created: 0, updated: 0, unchanged: 0, labels: 0 };
  const actions = [];
  const skipped = [];
  const duplicates = [];
  const act = (text) => {
    actions.push(text);
    if (dryRun) log(`would ${text}`);
  };
  let stopped = null;
  try {
    await ensureLabels({ gh, dryRun, act, counts });
    await syncEntries({ gh, root, dryRun, act, counts, skipped, duplicates });
    await syncDecisions({ gh, root, dryRun, act, counts, skipped });
  } catch (error) {
    if (!(error instanceof RateLimited)) throw error;
    stopped = error.resetAt ? error.resetAt.toISOString() : 'unknown';
  }
  return { dryRun, ...counts, stopped, skipped, duplicates, actions };
}

async function ensureLabels({ gh, dryRun, act, counts }) {
  const existing = new Set((await gh.paginate(gh.repoPath('/labels'))).map((label) => label.name.toLowerCase()));
  for (const label of LABELS) {
    if (existing.has(label.name.toLowerCase())) continue;
    act(`create label ${label.name}`);
    counts.labels += 1;
    if (!dryRun) await gh.request('POST', gh.repoPath('/labels'), { body: label });
  }
}

async function syncEntries({ gh, root, dryRun, act, counts, skipped, duplicates }) {
  const lines = parseFeatures(readText(featuresPath(root)));
  const index = entryIndex(await gh.issuesWithLabel(KIND_LABELS.entry), gh);
  duplicates.push(...index.duplicates);
  const seen = new Set();
  for (const { entry } of lines) {
    if (!entry) continue;
    if (typeof entry.id !== 'string' || !/^[A-Za-z0-9._-]+$/.test(entry.id)) {
      throw new Failure(`features.jsonl has an entry without a usable id (${JSON.stringify(entry.id)})`);
    }
    if (seen.has(entry.id)) throw new Failure(`features.jsonl has the id ${entry.id} twice`);
    seen.add(entry.id);
  }
  for (const { entry } of lines) {
    if (!entry) continue;
    const title = entry.name;
    const body = entryBody(entry);
    let issue = null;
    if (entry.issue != null) {
      const found = index.byNumber.get(entry.issue);
      if (!found) throw new Failure(`${entry.id} names issue #${entry.issue}, which is not a Workshop entry made by the bot`);
      if (found.id !== entry.id) throw new Failure(`${entry.id} names issue #${entry.issue}, which belongs to ${found.id}`);
      issue = found.issue;
    } else {
      issue = index.byMarker.get(entry.id) ?? null;
    }
    if (!issue) {
      act(`create the issue for ${entry.id}`);
      counts.created += 1;
      if (dryRun) continue;
      ({ data: issue } = await gh.request('POST', gh.repoPath('/issues'), {
        body: { title, body, labels: [KIND_LABELS.entry] },
      }));
    } else if (issue.title !== title || normalized(issue.body) !== normalized(body)) {
      act(`update #${issue.number} (${entry.id})`);
      counts.updated += 1;
      if (!dryRun) await gh.request('PATCH', gh.repoPath(`/issues/${issue.number}`), { body: { title, body } });
    } else if (entry.issue === issue.number) {
      counts.unchanged += 1;
    } else {
      counts.updated += 1;
    }
    if (entry.issue !== issue.number) {
      act(`write issue #${issue.number} into ${entry.id}`);
      if (!dryRun && !updateFeature(root, entry.id, 'issue', issue.number)) {
        skipped.push(`${entry.id}: its line left features.jsonl during the run; #${issue.number} is its issue`);
      }
    }
  }
}

async function syncDecisions({ gh, root, dryRun, act, counts, skipped }) {
  const items = listItems(readText(decisionsPath(root)).split('\n'));
  const issues = await gh.issuesWithLabel(KIND_LABELS.decision);
  const byNumber = new Map(issues.map((issue) => [issue.number, issue]));
  const made = new Map();
  for (const issue of issues.slice().sort((a, b) => a.number - b.number)) {
    if (issue.state === 'closed' && gh.isOwnBot(issue.user) && !made.has(issue.body)) made.set(issue.body, issue);
  }
  // A referenced issue sync made but could not close yet (stopped in between) is closed now.
  for (const item of items.filter((i) => i.issue != null)) {
    const issue = byNumber.get(item.issue);
    if (!issue || issue.state !== 'open' || !gh.isOwnBot(issue.user) || issue.body !== decisionBody(item.text)) continue;
    act(`close #${issue.number} as completed`);
    counts.updated += 1;
    if (!dryRun) await closeCompleted(gh, issue.number);
  }
  // Oldest first, so issue numbers follow the order the decisions were made.
  for (const item of items.filter((i) => i.issue == null).reverse()) {
    const body = decisionBody(item.text);
    const found = made.get(body);
    if (found) {
      counts.updated += 1;
      act(`write (#${found.number}) into DECISIONS.md`);
      if (!dryRun && !referenceDecision(root, item.text, found.number)) skipped.push(`decision "${decisionTitle(item.text, 60)}": its line changed during the run`);
      continue;
    }
    act(`create a closed decision issue for "${decisionTitle(body, 60)}"`);
    counts.created += 1;
    if (dryRun) continue;
    const { data: issue } = await gh.request('POST', gh.repoPath('/issues'), {
      body: { title: decisionTitle(body), body, labels: [KIND_LABELS.decision] },
    });
    // The reference goes in before the close, so a run stopped in between finds this issue again.
    if (!referenceDecision(root, item.text, issue.number)) {
      skipped.push(`decision "${decisionTitle(item.text, 60)}": its line changed during the run; #${issue.number} was made for it`);
    }
    await closeCompleted(gh, issue.number);
  }
}

function closeCompleted(gh, number) {
  return gh.request('PATCH', gh.repoPath(`/issues/${number}`), { body: { state: 'closed', state_reason: 'completed' } });
}

