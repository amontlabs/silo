// `workshop sync`: makes GitHub match docs/product and writes issue numbers
// back. Entry issues get their title and body only: sync never sets or
// removes a label other than the kind label on creation and never changes an
// entry's state. Every number is written to disk right after its issue is
// made, so a run stopped by a rate limit resumes without duplicates.
import { decisionTitle, listItems } from './decisions.js';
import { decisionsPath, featuresPath, readText } from './files.js';
import { parseFeatures } from './jsonl.js';
import { decisionIndex, entryIndex, referenceDecision, updateFeature, withoutRelativeLinks } from './match.js';
import { ENTRY_MARKER, Failure, KIND_LABELS, LABELS, RateLimited, entryMarker } from './names.js';

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
    await syncDecisions({ gh, root, dryRun, act, counts, skipped, duplicates });
  } catch (error) {
    if (!(error instanceof RateLimited)) throw error;
    stopped = error.resetAt ? error.resetAt.toISOString() : 'unknown';
  }
  return { dryRun, ...counts, stopped, skipped, duplicates, actions };
}

// Creates each of `labels` that the repository lacks, with its colour and description. A label that
// exists under the same name, in any case, is left as it is.
export async function ensureLabels({ gh, dryRun = false, act = () => {}, counts = { labels: 0 }, labels = LABELS }) {
  const existing = new Set((await gh.paginate(gh.repoPath('/labels'))).map((label) => label.name.toLowerCase()));
  for (const label of labels) {
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
  const numbers = new Set();
  for (const { entry } of lines) {
    if (!entry) continue;
    if (typeof entry.id !== 'string' || !/^[A-Za-z0-9._-]+$/.test(entry.id)) {
      throw new Failure(`features.jsonl has an entry without a usable id (${JSON.stringify(entry.id)})`);
    }
    if (seen.has(entry.id)) throw new Failure(`features.jsonl has the id ${entry.id} twice`);
    seen.add(entry.id);
    if (entry.issue != null && numbers.has(entry.issue)) throw new Failure(`features.jsonl names issue #${entry.issue} on two lines`);
    numbers.add(entry.issue);
  }
  for (const { entry } of lines) {
    if (!entry) continue;
    const title = entry.name;
    const body = entryBody(entry);
    let issue = null;
    let restore = false;
    if (entry.issue != null) {
      const found = index.byNumber.get(entry.issue);
      if (found && found.id !== entry.id) throw new Failure(`${entry.id} names issue #${entry.issue}, which belongs to ${found.id}`);
      if (found) issue = found.issue;
      else {
        // Edited on GitHub (text after the marker, or the entry label removed): the bot's own issue is restored.
        issue = await ownEntryIssue(gh, entry);
        restore = true;
      }
    } else {
      issue = index.byMarker.get(entry.id) ?? null;
    }
    if (restore) {
      act(`restore #${issue.number} (${entry.id}) as it was edited on GitHub`);
      counts.updated += 1;
      if (!dryRun) {
        await gh.request('PATCH', gh.repoPath(`/issues/${issue.number}`), { body: { title, body } });
        if (!labelNames(issue).includes(KIND_LABELS.entry)) {
          await gh.request('POST', gh.repoPath(`/issues/${issue.number}/labels`), { body: { labels: [KIND_LABELS.entry] } });
        }
      }
    } else if (!issue) {
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
      if (!dryRun && updateFeature(root, entry.id, 'issue', issue.number) !== 'ok') {
        skipped.push(`${entry.id}: its line left features.jsonl during the run; #${issue.number} is its issue`);
      }
    }
  }
}

async function syncDecisions({ gh, root, dryRun, act, counts, skipped, duplicates }) {
  const items = listItems(readText(decisionsPath(root)).split('\n'));
  const issues = await gh.issuesWithLabel(KIND_LABELS.decision);
  const byNumber = new Map(issues.map((issue) => [issue.number, issue]));
  const { byBody: made, duplicates: twice } = decisionIndex(issues, gh, (body) => decisionTitle(body, 60));
  duplicates.push(...twice);
  // A referenced issue sync made but could not close yet (stopped in between) is closed now;
  // one that was ever closed and then reopened is left as the person who reopened it wants.
  for (const item of items.filter((i) => i.issue != null)) {
    const issue = byNumber.get(item.issue);
    if (!issue || issue.state !== 'open' || !gh.isOwnBot(issue.user) || issue.body !== decisionBody(item.text)) continue;
    const events = await gh.paginate(gh.repoPath(`/issues/${issue.number}/events`));
    if (events.some((e) => e.event === 'closed')) continue;
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

const labelNames = (issue) => (issue.labels ?? []).map((label) => (typeof label === 'string' ? label : label.name));

/** The issue a line names when it is not in the entry index: only the bot's own entry issue, never another. */
async function ownEntryIssue(gh, entry) {
  const found = await gh.request('GET', gh.repoPath(`/issues/${entry.issue}`), { allow: [404, 410] });
  const issue = found.data;
  const otherKind = issue && labelNames(issue).some((name) => name === KIND_LABELS.decision || name === KIND_LABELS.suggestion);
  if (!issue || issue.pull_request || !gh.isOwnBot(issue.user) || otherKind) {
    throw new Failure(`${entry.id} names issue #${entry.issue}, which the bot did not create as a Workshop entry`);
  }
  const other = [...(issue.body ?? '').matchAll(new RegExp(ENTRY_MARKER.source, 'g'))].map((m) => m[1]).find((id) => id !== entry.id);
  if (other) throw new Failure(`${entry.id} names issue #${entry.issue}, which carries the marker of ${other}`);
  return issue;
}

function closeCompleted(gh, number) {
  return gh.request('PATCH', gh.repoPath(`/issues/${number}`), { body: { state: 'closed', state_reason: 'completed' } });
}

