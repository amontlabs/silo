// The live state of the plan on GitHub, as `status` and `show` read it.
//
// One interface: `read({ issue })` resolves to either
//   { available: false, reason, failed }   `failed` true when GitHub was tried and could not be read (exit 3)
//   { available: true, entries, suggestions, source }
// with
//   entries[id] = { number, url, verdict, building, branch, up, comments: [{ author, createdAt, body }],
//                   history: [{ createdAt, url, change }] }   (`change` as model/markers.js readChange gives it)
//   suggestions = [{ number, url, title, up, open }]
// `comments` and `history` are read for the entry named by `issue` only; `branch` for entries being built.
//
// The real reader tries, in order: the site's `GET <site>/api/workshop/state` (one call, no token;
// `issue?n=` for one issue's comments), then GitHub with the bot's token, else "not available".
// Neither failing is an error: `status` and `show` print their offline parts and exit 0.

import { appCredentials } from '../github/credentials.js';
import { GitHub, labelNames } from '../github/client.js';
import { entryIndex } from '../github/issues.js';
import { readBuildingComment } from '../model/comments.js';
import { readChange } from '../model/markers.js';
import { BUILDING_LABEL, DEFAULT_WEB, ENTRY_LABEL, ID_PATTERN, SUGGESTION_LABEL, VERDICTS } from '../model/names.js';

/** @typedef {{ available: false, reason: string, failed: boolean }} LiveUnavailable */
/**
 * @typedef {{ number: number, url: string, verdict: import('../model/status.js').Verdict | null, building: boolean, branch?: string | null, up: number,
 *   comments: { author: string, createdAt: string, body: string }[], history: { createdAt: string, url?: string, change: import('../model/markers.js').Change }[] }} LiveEntry
 */
/** @typedef {{ number: number, url: string, title: string, up: number, open: boolean }} LiveSuggestion */
/** @typedef {LiveUnavailable | { available: true, entries: Record<string, LiveEntry>, suggestions: LiveSuggestion[], source?: string }} LiveState */
/** @typedef {{ read(options?: { issue?: string }): Promise<LiveState> }} Live */

/** @returns {Live} */
export function offlineLive(reason = 'live state is not available') {
  return {
    async read() {
      return { available: false, reason, failed: false };
    },
  };
}

const SITE_TIMEOUT_MS = 8000;
const REPO = /^[A-Za-z0-9_.-]+\/[A-Za-z0-9_.-]+$/;
const verdictOf = (value) => (VERDICTS.includes(value) ? value : null);
const compareUrl = (web, repo, change) => `${web}/${repo}/compare/${change.before}...${change.commit}`;

/**
 * A thread as `show` wants it: people's and the bot's comments, and the bot's history lines apart.
 * @param {{ author: string, bot: boolean, createdAt: string, body: string, change?: any }[]} items @param {string} web @param {string} repo
 */
function thread(items, web, repo) {
  const comments = [];
  const history = [];
  let branch = null;
  for (const item of items) {
    const change = item.bot ? readChange(item.body) ?? (item.change && readChange(`<!-- silo-change: ${JSON.stringify(item.change)} -->`)) : null;
    if (change) history.push({ createdAt: item.createdAt, url: compareUrl(web, repo, change), change });
    else comments.push({ author: item.author, createdAt: item.createdAt, body: item.body });
    if (item.bot && readBuildingComment(item.body)) branch = readBuildingComment(item.body);
  }
  return { comments, history, branch };
}

/**
 * @param {{ fetch: typeof fetch, site: string, web?: string, repo: string, env: Record<string, string | undefined>, home: string, keychain: any, api: string }} deps
 * @returns {Live}
 */
export function createLive(deps) {
  const web = deps.web ?? DEFAULT_WEB;
  return {
    async read({ issue } = {}) {
      let siteReason;
      try {
        return await fromSite(deps, web, issue);
      } catch (error) {
        siteReason = /** @type {Error} */ (error).message;
      }
      let credentials;
      try {
        credentials = appCredentials(deps);
      } catch (error) {
        return { available: false, reason: `the site did not answer (${siteReason}) and GitHub cannot be read (${/** @type {Error} */ (error).message})`, failed: false };
      }
      try {
        return await fromGitHub(deps, web, credentials, issue);
      } catch (error) {
        return { available: false, reason: `the site did not answer (${siteReason}) and GitHub did not either (${/** @type {Error} */ (error).message})`, failed: false };
      }
    },
  };
}

/** @param {any} deps @param {string} path */
async function siteJson(deps, path) {
  const url = `${deps.site.replace(/\/$/, '')}/api/workshop/${path}`;
  let response;
  try {
    response = await deps.fetch(url, { headers: { accept: 'application/json', 'user-agent': 'silo-plan' }, signal: AbortSignal.timeout(SITE_TIMEOUT_MS) });
  } catch {
    throw new Error(`no answer from ${new URL(url).origin}`);
  }
  if (!response.ok) throw new Error(`${new URL(url).origin} answered ${response.status}`);
  try {
    return await response.json();
  } catch {
    throw new Error(`${new URL(url).origin} answered something that is not JSON`);
  }
}

/** @param {any} deps @param {string} web @param {string | undefined} wanted @returns {Promise<LiveState>} */
async function fromSite(deps, web, wanted) {
  const state = await siteJson(deps, 'state');
  if (!state || typeof state.issues !== 'object' || state.issues === null) throw new Error('the site answered without issues');
  const repo = typeof state.repo === 'string' && REPO.test(state.repo) ? state.repo : deps.repo;
  /** @type {Record<string, LiveEntry>} */
  const entries = Object.create(null);
  /** @type {LiveSuggestion[]} */
  const suggestions = [];
  const numbers = Object.keys(state.issues).map(Number).filter((n) => Number.isInteger(n) && n > 0).sort((a, b) => a - b);
  for (const number of numbers) {
    const item = state.issues[number];
    if (!item || typeof item !== 'object') continue;
    const url = `${web}/${repo}/issues/${number}`;
    const up = Number.isInteger(item.up) ? item.up : 0;
    if (item.kind === 'entry' && item.bot === true && typeof item.entry === 'string' && ID_PATTERN.test(item.entry) && !entries[item.entry]) {
      entries[item.entry] = { number, url, verdict: verdictOf(item.verdict), building: item.building === true, branch: null, up, comments: [], history: [] };
    } else if (item.kind === 'suggestion') {
      suggestions.push({ number, url, title: String(item.title ?? ''), up, open: item.state === 'open' });
    }
  }
  for (const [id, entry] of Object.entries(entries)) {
    if (!entry.building && id !== wanted) continue;
    const answer = await siteJson(deps, `issue?n=${entry.number}`);
    const items = (Array.isArray(answer?.comments) ? answer.comments : []).map((c) => ({ author: String(c.author ?? ''), bot: c.bot === true, createdAt: String(c.createdAt ?? ''), body: String(c.text ?? ''), change: c.change }));
    const read = thread(items, web, repo);
    entry.branch = read.branch;
    if (id === wanted) Object.assign(entry, { comments: read.comments, history: read.history });
  }
  return { available: true, entries, suggestions, source: 'site' };
}

/** @param {any} deps @param {string} web @param {{ appId: string, privateKey: string }} credentials @param {string | undefined} wanted @returns {Promise<LiveState>} */
async function fromGitHub(deps, web, credentials, wanted) {
  const gh = new GitHub({ api: deps.api, repo: deps.repo, fetch: deps.fetch, writeDelayMs: 0 });
  await gh.authenticate(credentials);
  const { byId } = entryIndex(gh, await gh.issuesWithLabel(ENTRY_LABEL));
  /** @type {Record<string, LiveEntry>} */
  const entries = Object.create(null);
  for (const [id, issue] of byId) {
    const labels = labelNames(issue);
    const verdicts = VERDICTS.filter((v) => labels.includes(`verdict:${v}`));
    const verdict = verdicts.length === 1 ? verdicts[0] : null;
    const building = labels.includes(BUILDING_LABEL) && issue.state === 'open';
    const entry = { number: issue.number, url: issue.html_url ?? `${web}/${deps.repo}/issues/${issue.number}`, verdict, building, branch: null, up: issue.reactions?.['+1'] ?? 0, comments: [], history: [] };
    if (building || id === wanted) {
      const items = (await gh.paginate(gh.repoPath(`/issues/${issue.number}/comments`))).map((c) => ({ author: c.user?.login ?? 'ghost', bot: gh.isOwnBot(c.user, c), createdAt: c.created_at, body: c.body ?? '' }));
      const read = thread(items, web, deps.repo);
      entry.branch = read.branch;
      if (id === wanted) Object.assign(entry, { comments: read.comments, history: read.history });
    }
    entries[id] = /** @type {LiveEntry} */ (entry);
  }
  const suggestions = (await gh.issuesWithLabel(SUGGESTION_LABEL))
    .filter((issue) => !labelNames(issue).includes(ENTRY_LABEL))
    .sort((a, b) => a.number - b.number)
    .map((issue) => ({ number: issue.number, url: issue.html_url ?? `${web}/${deps.repo}/issues/${issue.number}`, title: issue.title ?? '', up: issue.reactions?.['+1'] ?? 0, open: issue.state === 'open' }));
  return { available: true, entries, suggestions, source: 'github' };
}
