// `workshop doctor`: where each credential comes from (never a value), whether the App is public and
// installed, its permissions (Issues and Contents write), whether the labels the site sets exist, and entries named by several issues.
import { lookup } from './credentials.js';
import { CREDENTIAL_NAMES } from './names.js';
import { ENTRY_LABEL, SET_LABELS } from '../model/names.js';
import { issuesByEntry, readEntryMarker } from '../model/markers.js';

/** @param {{ deps: any, gh: import('./client.js').GitHub, credentials: () => { appId: string, privateKey: string } }} options */
export async function doctor({ deps, gh, credentials }) {
  const { sources } = lookup(CREDENTIAL_NAMES, deps);
  /** @type {{ credentials: Record<string, string>, app: string | null, public: boolean | null, installed: boolean, permissions: Record<string, string> | null, labels: Record<string, boolean> | null, duplicates: Record<string, number[]> | null, problems: string[] }} */
  const report = {
    credentials: Object.fromEntries(CREDENTIAL_NAMES.map((name) => [name, sources[name] ?? 'missing'])),
    app: null,
    public: null,
    installed: false,
    permissions: null,
    labels: null,
    duplicates: null,
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
    report.problems.push(/** @type {Error} */ (error).message);
    if (report.app) await checkPublic(gh, report);
    return report;
  }
  report.app = gh.app.slug;
  await checkPublic(gh, report);
  report.installed = true;
  report.permissions = gh.installation.permissions ?? {};
  if (report.permissions?.issues !== 'write') report.problems.push('the App cannot write issues');
  if (report.permissions?.contents !== 'write') {
    report.problems.push('the App cannot write contents: the owner\'s Take and Drop on the Workshop need the permission Contents: Read and write. Give it in the App\'s settings (Permissions & events), then accept it on the installation');
  }
  const existing = new Set((await gh.paginate(gh.repoPath('/labels'))).map((label) => String(label.name).toLowerCase()));
  report.labels = Object.fromEntries(SET_LABELS.map((label) => [label.name, existing.has(label.name.toLowerCase())]));
  const missing = SET_LABELS.filter((label) => !report.labels?.[label.name]).map((label) => label.name);
  if (missing.length) report.problems.push(`labels missing (create them on GitHub, in the repository's labels): ${missing.join(', ')}`);
  // The lowest number is the entry's thread; the site ignores the others.
  const entries = (await gh.issuesWithLabel(ENTRY_LABEL)).map((issue) => ({ number: issue.number, entry: readEntryMarker(issue.body) }));
  report.duplicates = Object.fromEntries([...issuesByEntry(entries)].filter(([, numbers]) => numbers.length > 1));
  for (const [id, numbers] of Object.entries(report.duplicates)) {
    report.problems.push(`the entry ${id} has several issues: #${numbers.join(', #')}. #${numbers[0]} is its thread and the site ignores the others: move what matters to #${numbers[0]} and remove the label ${ENTRY_LABEL} from the others`);
  }
  return report;
}

/**
 * Whether the App is public, read the way an outsider sees it: an unauthenticated GET /apps/{slug}
 * answers a public App and not a private one. Only members of the owning organization can
 * authorize a private App, so nobody else could sign in to the Workshop.
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
    report.problems.push('the App is private: only members of its organization can sign in to the Workshop. Make it public in the App settings (Advanced, Make public)');
  }
}
