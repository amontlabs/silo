// `doctor`, `ask`, `comment` and `built`.
import { lookup } from './credentials.js';
import { featuresPath, readText } from './files.js';
import { parseFeatures } from './jsonl.js';
import { CREDENTIAL_NAMES, ENTRY_MARKER, Failure, KIND_LABELS, LABELS, UsageError } from './names.js';

const KINDS = new Set(Object.values(KIND_LABELS));
const labelNames = (issue) => (issue.labels ?? []).map((label) => (typeof label === 'string' ? label : label.name));

export async function doctor({ deps, gh, credentials }) {
  const { sources } = lookup(CREDENTIAL_NAMES, deps);
  const report = {
    credentials: Object.fromEntries(CREDENTIAL_NAMES.map((name) => [name, sources[name] ?? 'missing'])),
    app: null,
    public: null,
    installed: false,
    permissions: null,
    labels: null,
    problems: [],
  };
  if (!sources.WORKSHOP_APP_ID || !sources.WORKSHOP_PRIVATE_KEY) {
    report.problems.push('the App id or private key is missing: run `workshop setup`');
    return report;
  }
  try {
    await gh.authenticate(credentials());
  } catch (error) {
    report.app = gh.app?.slug ?? null;
    report.problems.push(error.message);
    if (report.app) await checkPublic(gh, report);
    return report;
  }
  report.app = gh.app.slug;
  await checkPublic(gh, report);
  report.installed = true;
  report.permissions = gh.installation.permissions ?? {};
  if (report.permissions.issues !== 'write') report.problems.push('the App cannot write issues');
  const existing = new Set((await gh.paginate(gh.repoPath('/labels'))).map((label) => label.name.toLowerCase()));
  report.labels = Object.fromEntries(LABELS.map((label) => [label.name, existing.has(label.name.toLowerCase())]));
  const missing = LABELS.filter((label) => !report.labels[label.name]).map((label) => label.name);
  if (missing.length) report.problems.push(`labels missing (sync creates them): ${missing.join(', ')}`);
  return report;
}

/**
 * Whether the App is public, read the way an outsider sees it: an
 * unauthenticated GET /apps/{slug} answers a public App and not a private one.
 * Only members of the owning organization can authorize a private App, so
 * nobody else could sign in to the Workshop.
 */
async function checkPublic(gh, report) {
  let status;
  try {
    status = (await gh.request('GET', `/apps/${encodeURIComponent(report.app)}`, { auth: null, allow: [404] })).status;
  } catch {
    status = null;
  }
  report.public = status === 200 ? true : status === 404 ? false : null;
  if (report.public === false) {
    report.problems.push(
      'the App is private: only members of its organization can sign in to the Workshop, so sign-in will not work for anyone else. Make it public in the App settings (Advanced, Make public)',
    );
  }
}

async function workshopIssue(gh, number) {
  const found = await gh.request('GET', gh.repoPath(`/issues/${number}`), { allow: [404, 410] });
  const issue = found.data;
  if (!issue || issue.pull_request || !labelNames(issue).some((name) => KINDS.has(name))) {
    throw new Failure(`#${number} is not a Workshop issue`);
  }
  return issue;
}

export async function ask({ gh, root, title, body, entry }) {
  if (!title.trim()) throw new UsageError('--title is empty');
  if (title.length > 120) throw new UsageError('--title is longer than 120 characters');
  if (!body.trim()) throw new UsageError('the body is empty');
  let text = body.replace(/\s+$/, '');
  if (entry) {
    const found = parseFeatures(readText(featuresPath(root))).find((line) => line.entry?.id === entry)?.entry;
    if (!found) throw new UsageError(`no entry ${entry} in features.jsonl`);
    text += `\n\nAbout: ${found.issue ? `#${found.issue}` : found.name}`;
  }
  const { data } = await gh.request('POST', gh.repoPath('/issues'), {
    body: { title, body: text, labels: [KIND_LABELS.decision] },
  });
  return { number: data.number, url: data.html_url };
}

export async function comment({ gh, number, text }) {
  if (!text.trim()) throw new UsageError('the comment is empty');
  await workshopIssue(gh, number);
  const { data } = await gh.request('POST', gh.repoPath(`/issues/${number}/comments`), { body: { body: text } });
  return { number, comment: data.id, url: data.html_url };
}

export async function built({ gh, number }) {
  const issue = await workshopIssue(gh, number);
  if (!labelNames(issue).includes(KIND_LABELS.entry)) throw new Failure(`#${number} is not a Workshop entry`);
  const marker = (issue.body ?? '').match(ENTRY_MARKER);
  if (issue.state === 'closed' && issue.state_reason === 'completed') return { number, entry: marker?.[1] ?? null, changed: false };
  if (issue.state === 'closed') throw new Failure(`#${number} is closed as not planned (dropped); reopen it on GitHub first`);
  await gh.request('PATCH', gh.repoPath(`/issues/${number}`), { body: { state: 'closed', state_reason: 'completed' } });
  return { number, entry: marker?.[1] ?? null, changed: true };
}
