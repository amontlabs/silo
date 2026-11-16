// `workshop sync`: makes GitHub match docs/product and writes issue numbers
// back. Entry issues get their title and body only: sync never sets or
// removes a label other than the kind label on creation and never changes an
// entry's state. Every number is written to disk right after its issue is
// made, so a run stopped by a rate limit resumes without duplicates.
import { appendReference, decisionTitle, listItems } from './decisions.js';
import { decisionsPath, featuresPath, readText, writeText } from './files.js';
import { joinFeatures, parseFeatures, setField } from './jsonl.js';
import { ENTRY_MARKER, Failure, KIND_LABELS, LABELS, RateLimited, entryMarker } from './names.js';

export const entryBody = (entry) => `${entry.what ?? ''}\n\n${entryMarker(entry.id)}`;

function normalized(text) {
  return (text ?? '').replace(/\r\n/g, '\n').trim();
}

export async function sync({ gh, root, dryRun, log }) {
  const counts = { created: 0, updated: 0, unchanged: 0, labels: 0 };
  const actions = [];
  const act = (text) => {
    actions.push(text);
    if (dryRun) log(`would ${text}`);
  };
  let stopped = null;
  try {
    await ensureLabels({ gh, dryRun, act, counts });
    await syncEntries({ gh, root, dryRun, act, counts });
    await syncDecisions({ gh, root, dryRun, act, counts });
  } catch (error) {
    if (!(error instanceof RateLimited)) throw error;
    stopped = error.resetAt ? error.resetAt.toISOString() : 'unknown';
  }
  return { dryRun, ...counts, stopped, actions };
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

async function syncEntries({ gh, root, dryRun, act, counts }) {
  const path = featuresPath(root);
  const lines = parseFeatures(readText(path));
  const issues = await gh.issuesWithLabel(KIND_LABELS.entry);
  const byNumber = new Map(issues.map((issue) => [issue.number, issue]));
  const byMarker = new Map();
  for (const issue of issues.slice().sort((a, b) => a.number - b.number)) {
    const marker = (issue.body ?? '').match(ENTRY_MARKER);
    if (marker && !byMarker.has(marker[1])) byMarker.set(marker[1], issue);
  }
  const seen = new Set();
  for (const line of lines) {
    const entry = line.entry;
    if (!entry) continue;
    if (typeof entry.id !== 'string' || !/^[A-Za-z0-9._-]+$/.test(entry.id)) {
      throw new Failure(`features.jsonl has an entry without a usable id (${JSON.stringify(entry.id)})`);
    }
    if (seen.has(entry.id)) throw new Failure(`features.jsonl has the id ${entry.id} twice`);
    seen.add(entry.id);
    const title = entry.name;
    const body = entryBody(entry);
    let issue = null;
    if (entry.issue != null) {
      issue = byNumber.get(entry.issue);
      if (!issue) throw new Failure(`${entry.id} names issue #${entry.issue}, which is not a Workshop entry`);
      const marker = (issue.body ?? '').match(ENTRY_MARKER);
      if (marker && marker[1] !== entry.id) {
        throw new Failure(`${entry.id} names issue #${entry.issue}, which belongs to ${marker[1]}`);
      }
    } else {
      issue = byMarker.get(entry.id) ?? null;
    }
    if (!issue) {
      act(`create the issue for ${entry.id}`);
      counts.created += 1;
      if (dryRun) continue;
      const { data } = await gh.request('POST', gh.repoPath('/issues'), {
        body: { title, body, labels: [KIND_LABELS.entry] },
      });
      issue = data;
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
      if (!dryRun) {
        line.raw = setField(line.raw, 'issue', issue.number);
        line.entry = JSON.parse(line.raw);
        writeText(path, joinFeatures(lines));
      }
    }
  }
}

async function syncDecisions({ gh, root, dryRun, act, counts }) {
  const path = decisionsPath(root);
  let lines = readText(path).split('\n');
  const pending = listItems(lines).filter((item) => item.issue == null);
  if (!pending.length) return;
  const open = await gh.issuesWithLabel(KIND_LABELS.decision);
  const byBody = new Map();
  for (const issue of open.slice().sort((a, b) => a.number - b.number)) {
    const key = normalized(issue.body);
    if (!byBody.has(key)) byBody.set(key, issue);
  }
  // Oldest first, so issue numbers follow the order the decisions were made.
  for (const item of pending.reverse()) {
    let issue = byBody.get(normalized(item.text));
    if (!issue) {
      act(`create a closed decision issue for "${decisionTitle(item.text, 60)}"`);
      counts.created += 1;
      if (dryRun) continue;
      ({ data: issue } = await gh.request('POST', gh.repoPath('/issues'), {
        body: { title: decisionTitle(item.text), body: item.text, labels: [KIND_LABELS.decision] },
      }));
    } else {
      counts.updated += 1;
    }
    if (issue.state !== 'closed') {
      act(`close #${issue.number} as completed`);
      if (!dryRun) {
        await gh.request('PATCH', gh.repoPath(`/issues/${issue.number}`), {
          body: { state: 'closed', state_reason: 'completed' },
        });
      }
    }
    act(`write (#${issue.number}) into DECISIONS.md`);
    if (!dryRun) {
      const current = readText(path).split('\n');
      const again = listItems(current).find((i) => i.start === item.start && i.text === item.text && i.issue == null);
      if (!again) throw new Failure('DECISIONS.md changed while sync was running; run it again');
      lines = appendReference(current, again, issue.number);
      writeText(path, lines.join('\n'));
    }
  }
}
