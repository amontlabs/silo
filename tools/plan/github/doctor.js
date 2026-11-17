// `plan doctor`: where each credential comes from (never a value), whether the App is public and
// installed, its permissions, and whether the six labels exist.
import { lookup } from './credentials.js';
import { CREDENTIAL_NAMES } from './names.js';
import { LABELS } from '../model/names.js';

/** @param {{ deps: any, gh: import('./client.js').GitHub, credentials: () => { appId: string, privateKey: string } }} options */
export async function doctor({ deps, gh, credentials }) {
  const { sources } = lookup(CREDENTIAL_NAMES, deps);
  /** @type {{ credentials: Record<string, string>, app: string | null, public: boolean | null, installed: boolean, permissions: Record<string, string> | null, labels: Record<string, boolean> | null, problems: string[] }} */
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
    report.problems.push('the App id or private key is missing: run `plan setup`');
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
  const existing = new Set((await gh.paginate(gh.repoPath('/labels'))).map((label) => String(label.name).toLowerCase()));
  report.labels = Object.fromEntries(LABELS.map((label) => [label.name, existing.has(label.name.toLowerCase())]));
  const missing = LABELS.filter((label) => !report.labels?.[label.name]).map((label) => label.name);
  if (missing.length) report.problems.push(`labels missing (plan sync creates them): ${missing.join(', ')}`);
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
