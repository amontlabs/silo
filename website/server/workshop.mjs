// The Workshop API: every endpoint under /api/workshop/, independent of the
// HTTP server that runs it. A request is { method, url, headers, body } and a
// response is { status, headers, cookies, body }.
//
// Identities: anonymous reads are answered from the in-memory snapshot
// (snapshot.mjs), which alone reads GitHub for them, with the installation
// token. Everything done for a signed-in person uses that person's token,
// except labelling a suggestion and opening an entry's thread (with the reads
// of entry issues before and after it), which the App does.
import { AsyncLocalStorage } from 'node:async_hooks';
import { createHash, randomBytes, timingSafeEqual } from 'node:crypto';
import { createAppAuth } from './app-auth.mjs';
import { createGitHub, DEFAULT_TIMEOUT, GitHubError, nextLink } from './github.mjs';
import { clean, commentIssue, countOf, kindOf, LABELS, labelNames } from './issues.mjs';
import { entryMarker, LABELS as LABEL_DEFINITIONS, readEntryMarker } from '../../tools/workshop/model/index.js';
import { clearCookie, cookieNames, HINT_COOKIE, parseCookies, seal, serializeCookie, SESSION_MAX_AGE, STATE_MAX_AGE, unseal } from './session.mjs';
import { createSnapshot, SNAPSHOT } from './snapshot.mjs';

export { LABELS };

const DIRECTION_CONTROLS = /[\u202A-\u202E\u2066-\u2069]/g;
const TITLE_FORBIDDEN = /[\u0000-\u001F\u007F-\u009F\u2028\u2029]/;
export const LIMITS = { text: 5000, title: 120 };
// Ceilings per instance. The installation token has one per use, kept apart so that no use can
// starve another. `exchange` covers new sign-ins at GitHub's token endpoint and `renew` the token
// refreshes of existing sessions, so that made-up sign-ins cannot stop sessions from refreshing.
// The rest are per person (writes, suggestions) and per session (GitHub reads). The threads opened for
// entries are bounded by the plan's entries, one thread each, and by the `open` ceiling for how fast.
export const BUDGET = {
  refreshCallsPerMinute: 40,
  signinLookupsPerMinute: 4,
  labelCallsPerMinute: 10,
  // An opening is 3 calls: the read of entry issues, the opening and the read that confirms it.
  openCallsPerMinute: 27,
  tokenExchangesPerMinute: 30,
  tokenRenewalsPerMinute: 60,
  writesPerMinute: 20,
  suggestionsPerMinute: 3,
  suggestionsPerHour: 10,
  sessionReadsPerMinute: 60,
};
const CEILINGS = {
  refresh: ['refreshCallsPerMinute', true],
  signin: ['signinLookupsPerMinute', true],
  label: ['labelCallsPerMinute', true],
  open: ['openCallsPerMinute', true],
  exchange: ['tokenExchangesPerMinute', false],
  renew: ['tokenRenewalsPerMinute', false],
};
export const DEADLINE = 12_000;
const RECENT_POST = 5 * 60_000;
const REFRESH_MARGIN = 5 * 60_000;
const MAX_BLOCK = 60 * 60_000;
const UNLABELLED_LIMIT = 100;
const LABEL_ATTEMPTS = 5;
const REJECTED_FOR = 24 * 3600_000;
const REMEMBERED = 10_000;
const SIGNIN_FAILED = '/workshop?signin=failed';

const PUBLIC_READ = 'public, s-maxage=10, stale-while-revalidate=60';
// `state` answers at once from whatever it has, so the CDN may serve an old copy while it asks again;
// a `fresh=1` answer that is fresh is cached briefly, one that is still old is not cached at all.
const STATE_READ = 'public, s-maxage=10, stale-while-revalidate=86400';
const FRESH_READ = 'public, s-maxage=10';
const PRIVATE = 'private, no-store';

const ENDPOINTS = new Map(Object.entries({
  state: ['GET'], me: ['GET'], issue: ['GET'], login: ['GET'], callback: ['GET'], vote: ['GET', 'POST'],
  logout: ['POST'], comment: ['POST'], 'comment-vote': ['POST'], suggest: ['POST'],
}));

// The person's 👍 on the latest 100 comments of an issue, in one GraphQL call with their token. A
// comment id passed 2^31 long ago: `fullDatabaseId` is the REST id as a string.
const REACTED = 'query($owner: String!, $name: String!, $number: Int!) { repository(owner: $owner, name: $name) { issue(number: $number) { comments(last: 100) { nodes { fullDatabaseId reactionGroups { content viewerHasReacted } } } } } }';

class Refusal extends Error {
  constructor(status, code, message, extra = {}, headers = {}) {
    super(message);
    this.status = status;
    this.code = code;
    this.extra = extra;
    this.headers = headers;
  }
}

const refuse = (status, code, message, extra, headers) => { throw new Refusal(status, code, message, extra, headers); };
const tooMany = (seconds, message = 'Too many requests. Try again shortly.') => refuse(429, 'rate_limited', message, {}, { 'Retry-After': String(Math.max(1, Math.ceil(seconds))) });
const hash = (value) => createHash('sha256').update(String(value)).digest('hex');

// Fixed-window counters by key, bounded in size.
function counter(windowMs) {
  const windows = new Map();
  return {
    // Takes `cost` from `key`'s allowance of `limit`, all or nothing, or answers the seconds until it resets.
    take(key, limit, at, cost = 1) {
      const wait = this.peek(key, limit, at, cost);
      if (!wait) windows.get(key).count += cost;
      return wait;
    },
    // Whether `cost` would fit, without taking it: 0, or the seconds until the allowance resets.
    peek(key, limit, at, cost = 1) {
      let entry = windows.get(key);
      if (!entry || at - entry.start >= windowMs) {
        entry = { start: at, count: 0 };
        windows.delete(key);
        windows.set(key, entry);
        if (windows.size > REMEMBERED) windows.delete(windows.keys().next().value);
      }
      return entry.count + cost > limit ? Math.max(1, (entry.start + windowMs - at) / 1000) : 0;
    },
  };
}

// A set of keys remembered for a while, bounded in size.
function memory(forMs) {
  const entries = new Map();
  return {
    has(key, at) {
      const until = entries.get(key);
      if (until === undefined) return false;
      if (until > at) return true;
      entries.delete(key);
      return false;
    },
    add(key, at) {
      entries.delete(key);
      entries.set(key, at + forMs);
      if (entries.size > REMEMBERED) entries.delete(entries.keys().next().value);
    },
  };
}

// `entries`: the plan's entries by id, from entries.mjs.
export function createWorkshop({
  config, entries = new Map(), fetch = globalThis.fetch, now = Date.now, log = defaultLog,
  timeoutMs = DEFAULT_TIMEOUT, deadlineMs = DEADLINE, keepAlive = () => {},
}) {
  // Every handler's GitHub calls, and every wait it does, share one deadline; background work runs outside it.
  const requests = new AsyncLocalStorage();
  const detach = (fn) => requests.exit(fn);
  const github = createGitHub({ apiBase: config.apiBase, fetch, now, timeoutMs, deadline: () => requests.getStore()?.deadline });
  const app = config.configured ? createAppAuth({ config, github, now }) : null;
  const names = cookieNames(config.secureCookies);
  const repo = config.repo;
  const secure = config.secureCookies;
  if (config.problem) log({ config: 'invalid', variable: config.problem });

  let blockedUntil = 0;
  const ceilings = Object.fromEntries(Object.keys(CEILINGS).map((name) => [name, { start: 0, used: 0 }]));
  const refreshing = new Map();
  const writes = counter(60_000);
  const suggestionsMinute = counter(60_000);
  const suggestionsHour = counter(3600_000);
  const sessionReads = counter(60_000);
  const openings = new Map();
  const usedSignins = memory(STATE_MAX_AGE * 1000);
  const rejected = memory(REJECTED_FOR);
  const unlabelled = new Map();

  async function handle(request) {
    const ctx = { request, endpoint: null, cookies: [], headers: {} };
    const store = { deadline: Date.now() + deadlineMs, changed: false };
    let response;
    try {
      let url;
      try {
        url = new URL(request.url, 'http://workshop.invalid');
      } catch {
        refuse(404, 'not_found', 'There is nothing here.');
      }
      ctx.url = url;
      const endpoint = url.pathname.match(/^\/api\/workshop\/([a-z]+(?:-[a-z]+)?)$/)?.[1];
      if (!endpoint || !ENDPOINTS.has(endpoint)) refuse(404, 'not_found', 'There is nothing here.');
      ctx.endpoint = endpoint;
      const methods = ENDPOINTS.get(endpoint);
      if (!methods.includes(request.method)) {
        ctx.headers.Allow = methods.join(', ');
        refuse(405, 'method_not_allowed', 'This address does not accept that method.');
      }
      if (!config.configured) refuse(503, 'not_configured', 'Live comments are not set up on this server.');
      if (request.method === 'POST') checkSameSite(request);
      const route = routes.get(`${request.method} ${endpoint}`);
      response = await requests.run(store, () => route(ctx));
    } catch (error) {
      response = errorResponse(ctx, error);
      // A write that failed on an entry's thread still names the thread, which may have just been opened.
      if (ctx.thread) response.body = JSON.stringify({ ...JSON.parse(response.body), n: ctx.thread });
      if ((ctx.endpoint === 'login' || ctx.endpoint === 'callback') && response.status !== 405) response = redirect(SIGNIN_FAILED);
    }
    // Any request that changed something on GitHub starts a refresh; a successful one waits for it briefly.
    if (store.changed && snapshots) {
      const included = snapshots.refreshSoon();
      if (response.status === 200) await requests.run(store, () => bounded(included).catch(() => {}));
    }
    response.headers = { 'Cache-Control': PRIVATE, ...ctx.headers, ...response.headers, 'Content-Security-Policy': "frame-ancestors 'none'", 'X-Frame-Options': 'DENY' };
    response.cookies = [...ctx.cookies, ...(response.cookies || [])];
    if (response.cookies.length) response.headers['Cache-Control'] = PRIVATE;
    log({ endpoint: ctx.endpoint || 'unknown', status: response.status, ...(ctx.githubStatus !== undefined ? { github: ctx.githubStatus } : {}) });
    return response;
  }

  function errorResponse(ctx, error) {
    if (error instanceof Refusal) return json(error.status, { error: error.code, message: error.message, ...error.extra }, error.headers);
    if (error instanceof GitHubError) {
      ctx.githubStatus = error.status;
      if (error.notInstalled) return json(503, { error: 'not_configured', message: 'Live comments are not set up on this server.' });
      if (error.rateLimited) return rateLimited(error.retryAfter);
      if (error.status === 401 && error.as === 'user') {
        if (ctx.session) rejected.add(hash(ctx.session.t), now());
        clearSession(ctx);
        return json(401, { error: 'signed_out', message: 'Sign in again.' });
      }
      if (error.status === 401) return json(502, { error: 'github_failed', message: 'GitHub did not answer. Try again later.' });
      if (error.status === 404 || error.status === 410) return json(404, { error: 'not_found', message: 'This is not a Workshop issue.' });
      if (error.status === 403) {
        return json(403, { error: 'forbidden', message: 'GitHub did not allow this.' });
      }
      if (error.status === 422) return json(400, { error: 'rejected', message: 'GitHub did not accept this.' });
      return json(502, { error: 'github_failed', message: 'GitHub did not answer. Try again later.' });
    }
    log({ endpoint: ctx.endpoint || 'unknown', failure: error?.name || 'Error' });
    return json(502, { error: 'github_failed', message: 'Something went wrong. Try again later.' });
  }

  function rateLimited(seconds) {
    return json(429, { error: 'rate_limited', message: 'GitHub is busy. Try again in a minute.' }, { 'Retry-After': String(Math.min(Math.max(1, Math.ceil(seconds || 60)), 3600)) });
  }

  function checkSameSite(request) {
    if (request.headers.origin !== config.origin) refuse(403, 'bad_origin', 'This request did not come from the Workshop.');
    if (request.headers['x-workshop'] !== '1') refuse(403, 'bad_request_header', 'This request did not come from the Workshop.');
  }

  // Waits for `promise`, but never past the current request's deadline (outside a request, without limit).
  function bounded(promise) {
    const deadline = requests.getStore()?.deadline;
    if (deadline === undefined) return promise;
    let timer;
    return Promise.race([
      promise,
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new GitHubError(0, { timeout: true })), Math.max(0, deadline - Date.now()));
        timer.unref?.();
      }),
    ]).finally(() => clearTimeout(timer));
  }

  // Ceilings

  function blockFor(seconds) {
    blockedUntil = Math.max(blockedUntil, now() + Math.min((seconds || 60) * 1000, MAX_BLOCK));
  }

  function noteInstallationError(error) {
    if (!(error instanceof GitHubError)) return;
    if (error.rateLimited && !error.local) blockFor(error.retryAfter);
    if (error.status === 401) app.dropToken();
  }

  function ceilingOf(name) {
    const ceiling = ceilings[name];
    const at = now();
    if (at - ceiling.start >= 60_000) {
      ceiling.start = at;
      ceiling.used = 0;
    }
    return ceiling;
  }

  // Whether ceiling `name` has `cost` calls left this minute (and, for the installation, GitHub is not making us wait).
  function hasRoom(name, cost = 1) {
    const [key, installation] = CEILINGS[name];
    if (installation && now() < blockedUntil) return false;
    return ceilingOf(name).used + cost <= BUDGET[key];
  }

  function spend(name, cost = 1) {
    if (!hasRoom(name, cost)) return false;
    ceilingOf(name).used += cost;
    return true;
  }

  function spendOrRefuse(name, cost = 1) {
    if (spend(name, cost)) return;
    const at = now();
    const blocked = CEILINGS[name][1] && at < blockedUntil;
    const wait = Math.max(blocked ? blockedUntil - at : 0, ceilings[name].start + 60_000 - at, 1000);
    // New threads are busy, not GitHub: the answer says so, with the wait.
    if (name === 'open' && !blocked) tooMany(wait / 1000, 'New threads are busy. Try again shortly.');
    // `local`: our own ceiling, not GitHub, said no.
    throw new GitHubError(429, { rateLimited: true, retryAfter: Math.ceil(wait / 1000), as: 'installation', local: !blocked });
  }

  // Every installation-token call goes through here: nothing reaches GitHub while it has asked us to wait.
  async function asInstallation(call) {
    if (now() < blockedUntil) throw new GitHubError(429, { rateLimited: true, retryAfter: Math.ceil((blockedUntil - now()) / 1000), as: 'installation' });
    try {
      return await call(await bounded(app.installationToken()));
    } catch (error) {
      noteInstallationError(error);
      throw error;
    }
  }

  const installationRequest = (method, path, options = {}) => asInstallation((token) => github.request(method, path, { ...options, token, as: 'installation' }));

  // A call with the person's token. Reads (a GET, or a GraphQL query: `read`) count in the session's
  // own ceiling; a change marks the request as having changed GitHub, so a refresh follows.
  async function userRequest(session, method, path, { read = method === 'GET', ...options } = {}) {
    if (read) {
      const wait = sessionReads.take(hash(session.t), BUDGET.sessionReadsPerMinute, now());
      if (wait) tooMany(wait);
    }
    const answer = await github.request(method, path, { ...options, token: session.t, as: 'user' });
    if (!read) {
      const store = requests.getStore();
      if (store) store.changed = true;
    }
    return answer;
  }

  async function userPages(session, path) {
    const items = [];
    let next = path + (path.includes('?') ? '&' : '?') + 'per_page=100';
    for (let page = 0; next && page < 50; page += 1) {
      const { data, headers } = await userRequest(session, 'GET', next);
      items.push(...(Array.isArray(data) ? data : []));
      next = nextLink(headers.get('link'));
      if (next && !next.startsWith(config.apiBase + '/')) throw new GitHubError(502, { as: 'user' });
    }
    return items;
  }

  const snapshots = config.configured ? createSnapshot({
    repo,
    entries,
    app: () => ({ id: config.appId, login: app?.login() ?? null }),
    now,
    log,
    detach,
    keepAlive,
    afterRefresh: () => { if (unlabelled.size) retryLabels().catch(() => {}); },
    request: (method, path, options) => {
      // A refresh that was aborted (lost or past its limit) stops spending.
      if (options?.signal?.aborted) throw new GitHubError(0, { network: true, timeout: true, as: 'installation' });
      spendOrRefuse('refresh');
      return installationRequest(method, path, options);
    },
  }) : null;

  // GitHub data

  // The issue as the signed-in person sees it, refused unless it is a Workshop issue.
  async function workshopIssue(session, n) {
    const { data: issue } = await userRequest(session, 'GET', `/repos/${repo}/issues/${n}`);
    const kind = kindOf(issue);
    if (!kind) refuse(404, 'not_found', 'This is not a Workshop issue.');
    return { issue, kind };
  }

  // Whether `session`'s person posted exactly `body` on issue `n` in the last five minutes, so that a
  // request retried after a cut does not post it twice.
  async function alreadyPosted(session, n, body) {
    const since = new Date(now() - RECENT_POST).toISOString().replace(/\.\d{3}Z$/, 'Z');
    const { data } = await userRequest(session, 'GET', `/repos/${repo}/issues/${n}/comments?since=${since}&per_page=100`);
    return (Array.isArray(data) ? data : []).find((comment) => comment.user?.id === session.id && (comment.body || '').trim() === body) ?? null;
  }

  // The snapshot for an anonymous read, waiting up to `wait` ms for a refresh when it is old. A cold
  // instance waits for its first refresh. Every wait ends at the request's deadline at the latest (the
  // refresh goes on in the background); with no snapshot, 502, or 503 when the App is not installed
  // on the repository.
  async function readSnapshot(ctx, wait = 0) {
    try {
      return await bounded(snapshots.read({ wait }));
    } catch (error) {
      if (error instanceof GitHubError) ctx.githubStatus = error.status;
      if (error instanceof GitHubError && error.notInstalled) refuse(503, 'not_configured', 'Live comments are not set up on this server.');
      refuse(502, 'github_failed', 'GitHub did not answer. Try again later.');
    }
  }

  async function labelSuggestion(number) {
    spendOrRefuse('label');
    await installationRequest('POST', `/repos/${repo}/issues/${number}/labels`, { body: { labels: [LABELS.suggestion] } });
  }

  // Suggestions created but not labelled. Retried, outside any request, after a successful suggestion
  // and after each snapshot refresh: an issue is labelled only if it is still open and has no label
  // at all; anything else leaves the list, a failing number does not hold up the others, and a number
  // that failed five times is dropped.
  let retrying = null;
  function retryLabels() {
    retrying ??= detach(async () => {
      for (const [number, failures] of [...unlabelled]) {
        if (!hasRoom('label', 2)) break;
        try {
          spendOrRefuse('label');
          const { data: issue } = await installationRequest('GET', `/repos/${repo}/issues/${number}`);
          if (issue.state === 'open' && !issue.pull_request && labelNames(issue).length === 0) await labelSuggestion(number);
          unlabelled.delete(number);
        } catch (error) {
          if ((error instanceof GitHubError && [404, 410, 422].includes(error.status)) || failures + 1 >= LABEL_ATTEMPTS) unlabelled.delete(number);
          else unlabelled.set(number, failures + 1);
        }
      }
    }).finally(() => { retrying = null; });
    keepAlive(retrying.catch(() => {}));
    return retrying;
  }

  // Entries' threads

  // The issue a write names: `n`, an issue, or `entry`, an id of the plan; never both.
  function target(input) {
    if (input.n !== undefined && input.entry !== undefined) refuse(400, 'bad_input', 'Name an issue or an entry, not both.');
    if (input.entry === undefined) return { n: issueNumber(input.n), entry: null };
    if (typeof input.entry !== 'string') refuse(400, 'bad_input', 'That entry is not valid.');
    const entry = entries.get(input.entry);
    if (!entry) refuse(404, 'not_found', 'This is not an entry of the plan.');
    return { n: null, entry };
  }

  // The issue named by `n`, refused unless it is a Workshop issue.
  async function namedIssue(session, n) {
    const { issue } = await workshopIssue(session, n);
    return { n, issue, fresh: false };
  }

  // Whether `issue`, as GitHub gave it, is `entry`'s thread: the suggestion its file names, or an entry
  // issue whose marker names it.
  function isThreadOf(issue, entry) {
    const kind = kindOf(issue);
    if (kind === 'suggestion') return issue.number === entry.suggestedIn;
    return kind === 'entry' && readEntryMarker(issue.body) === entry.id;
  }

  // The entry issues on GitHub that name `id`, lowest number first. Read with the installation token
  // within the `open` ceiling: the newest pages, until they reach a minute before the snapshot's
  // refresh began; the snapshot gives the older issues that name `id`. Without a snapshot, every page.
  // `ignore`: numbers a read has shown are not the thread.
  async function entryIssuesOnGitHub(id, ignore) {
    const held = snapshots.held(id);
    const numbers = new Set((held?.numbers ?? []).filter((number) => !ignore.has(number)));
    const found = new Map();
    const since = held ? held.startedAt - SNAPSHOT.overlap : -Infinity;
    let next = `/repos/${repo}/issues?labels=${encodeURIComponent(LABELS.entry)}&state=all&sort=created&direction=desc&per_page=100`;
    for (let page = 0; next && page < SNAPSHOT.issuePages; page += 1) {
      spendOrRefuse('open');
      const { data, headers } = await installationRequest('GET', next);
      const issues = Array.isArray(data) ? data : [];
      for (const issue of issues) {
        if (kindOf(issue) !== 'entry') continue;
        if (readEntryMarker(issue.body) === id && !ignore.has(issue.number)) {
          numbers.add(issue.number);
          found.set(issue.number, issue);
        }
      }
      const oldest = issues.at(-1);
      if (!oldest || Date.parse(oldest.created_at) < since) break;
      next = nextLink(headers.get('link'));
      if (next && !next.startsWith(config.apiBase + '/')) throw new GitHubError(502, { as: 'installation' });
    }
    return { numbers: [...numbers].sort((a, b) => a - b), found };
  }

  // The entry's title as an issue title: one line, cut to the title limit at a word, with an ellipsis.
  function threadTitle(title) {
    const line = clean(title, { oneLine: true }).replace(DIRECTION_CONTROLS, '').replace(/\s+/g, ' ').trim();
    const chars = [...line];
    if (chars.length <= LIMITS.title) return line;
    const room = chars.slice(0, LIMITS.title - 1).join('');
    const space = room.lastIndexOf(' ');
    return `${(space > 0 ? room.slice(0, space) : room).replace(/[\s,;:.]+$/, '')}…`;
  }

  // What the issue is, with a link to its row on the Workshop, then the marker that names the entry.
  function threadBody(entry) {
    const what = entry.kind === 'decision' ? 'Decision' : 'Feature';
    return `${what} of the Silo plan: ${config.origin}/workshop#${entry.part ?? 'product'}/${entry.id}\n\n${entryMarker(entry.id)}`;
  }

  // An App call that repairs what an opening left (a label, a duplicate issue): charged to the `open`
  // ceiling but never refused by it, so that no open issue without its label, and no duplicate, is
  // left behind for want of room. GitHub asking us to wait still stops it.
  function repair(method, path, options) {
    ceilingOf('open').used += 1;
    return installationRequest(method, path, options);
  }

  // Closes an issue this instance opened, as not planned and without the label `workshop:entry`.
  async function retire(number) {
    try {
      await repair('PATCH', `/repos/${repo}/issues/${number}`, { body: { state: 'closed', state_reason: 'not_planned', labels: [] } });
    } catch (error) {
      log({ endpoint: 'thread', failure: 'retire_failed', ...(error instanceof GitHubError ? { github: error.status } : {}) });
    }
  }

  // The issue GitHub created, with the label `workshop:entry`. When GitHub dropped the label, the App
  // creates it in the repository (it may exist already) and adds it; when the issue still lacks it,
  // the App closes the issue and the write is refused.
  async function labelled(data, entry) {
    if (isThreadOf(data, entry)) return data;
    try {
      const definition = LABEL_DEFINITIONS.find((label) => label.name === LABELS.entry);
      await repair('POST', `/repos/${repo}/labels`, { body: definition }).catch((error) => {
        if (!(error instanceof GitHubError && error.status === 422)) throw error;
      });
      const { data: labels } = await repair('POST', `/repos/${repo}/issues/${data.number}/labels`, { body: { labels: [LABELS.entry] } });
      const issue = { ...data, labels: Array.isArray(labels) ? labels : [] };
      if (isThreadOf(issue, entry)) return issue;
    } catch {
      // Refused below, after the issue is closed.
    }
    log({ endpoint: 'thread', failure: 'thread_not_labelled' });
    await retire(data.number);
    refuse(502, 'github', 'The thread could not be opened on the Workshop. Try again later.');
  }

  // Finds the entry's thread on GitHub and, with `open`, opens it as the App when there is none.
  // Answers `{ candidates }`, the numbers to confirm, lowest first, and `opened`, the issue this call
  // opened when it is the thread. Requests that may open share one call per entry on this instance.
  // An opening is bounded only by the `open` ceiling. After an opening, the entry issues are read
  // again; when that read fails, the write is not made. When it shows a lower issue for the entry
  // (another instance opened one at the same moment), that one is the thread and the issue this call
  // opened is closed.
  function lookUp(entry, ignore, open) {
    const store = requests.getStore();
    const run = async () => {
      const before = await entryIssuesOnGitHub(entry.id, ignore);
      if (before.numbers.length || !open) return { candidates: before.numbers, opened: null };
      // Room for the opening and the read that confirms it.
      if (!hasRoom('open', 2)) spendOrRefuse('open', 2);
      spendOrRefuse('open');
      const { data } = await installationRequest('POST', `/repos/${repo}/issues`, { body: { title: threadTitle(entry.title), body: threadBody(entry), labels: [LABELS.entry] } });
      if (store) store.changed = true;
      const issue = await labelled(data, entry);
      // The read confirms only when it shows the issue just opened; otherwise the write is not made.
      const after = await entryIssuesOnGitHub(entry.id, ignore);
      if (!after.found.has(issue.number)) refuse(502, 'github_failed', 'GitHub did not show the new thread yet. Try again.');
      const lowest = after.numbers[0];
      if (lowest === issue.number) {
        snapshots.patch.issue(issue, 'entry');
        return { candidates: [lowest], opened: issue };
      }
      await retire(issue.number);
      const lower = after.found.get(lowest);
      if (lower) snapshots.patch.issue(lower, 'entry');
      return { candidates: [lowest], opened: null };
    };
    if (!open) return run();
    const opening = run().finally(() => openings.delete(entry.id));
    opening.catch(() => {});
    openings.set(entry.id, opening);
    return opening;
  }

  // The entry's thread: the suggestion it was written from when that is a Workshop suggestion, else the
  // lowest-numbered entry issue that names it. Every candidate, from the snapshot or from GitHub, is
  // confirmed by the read of it the write makes anyway: an issue whose label or marker was changed on
  // GitHub is no thread, and the thread is then looked up on GitHub once more. With `open`, an entry
  // without one gets one; without, null. Answers `{ n, issue, fresh }`, `fresh` when this request
  // opened the issue and holds it as GitHub created it. Once a thread is known, `ctx.thread` names it,
  // so that every answer to the write carries it, errors included.
  async function threadOf(ctx, session, entry, { open }) {
    const found = (n, issue, fresh = false) => {
      ctx.thread = n;
      return { n, issue, fresh };
    };
    const ignore = new Set();
    const confirm = async (n) => {
      ctx.thread = n;
      const read = await userRequest(session, 'GET', `/repos/${repo}/issues/${n}`).catch((error) => {
        if (error instanceof GitHubError && (error.status === 404 || error.status === 410)) return null;
        throw error;
      });
      if (read && isThreadOf(read.data, entry)) return read.data;
      ctx.thread = undefined;
      ignore.add(n);
      return null;
    };
    if (entry.suggestedIn) {
      const issue = await confirm(entry.suggestedIn);
      if (issue) return found(entry.suggestedIn, issue);
    }
    const held = snapshots.held(entry.id)?.numbers ?? [];
    if (held.length) {
      const issue = await confirm(held[0]);
      if (issue) return found(held[0], issue);
    }
    for (let round = 0; round < 2; round += 1) {
      const shared = open ? openings.get(entry.id) : null;
      const { candidates, opened } = await bounded(shared ?? lookUp(entry, ignore, open));
      if (opened && !shared) return found(opened.number, opened, true);
      for (const n of candidates) {
        const issue = await confirm(n);
        if (issue) return found(n, issue);
      }
      if (!candidates.length && !open) return null;
    }
    refuse(502, 'github', 'The thread of this entry could not be found. Try again.');
  }

  // A write to an entry's thread that GitHub refuses: the thread is read again, and when it is closed
  // or locked on GitHub the answer says so.
  async function onThread(session, named, n, write) {
    try {
      return await write();
    } catch (error) {
      if (named.entry && error instanceof GitHubError && error.status === 403 && !error.rateLimited) {
        const read = await userRequest(session, 'GET', `/repos/${repo}/issues/${n}`).catch(() => null);
        if (read && (read.data?.state === 'closed' || read.data?.locked)) refuse(403, 'closed', 'This thread is closed on GitHub.');
      }
      throw error;
    }
  }

  // The ids of the latest 100 comments of issue `n` the person gave a 👍.
  async function reactedComments(session, n) {
    const [owner, name] = repo.split('/');
    const { data } = await userRequest(session, 'POST', '/graphql', { body: { query: REACTED, variables: { owner, name, number: n } }, read: true });
    const nodes = data?.data?.repository?.issue?.comments?.nodes;
    if (!Array.isArray(nodes)) {
      if (Array.isArray(data?.errors) && data.errors.some((error) => error?.type === 'RATE_LIMITED')) throw new GitHubError(429, { rateLimited: true, retryAfter: 60, as: 'user' });
      throw new GitHubError(502, { as: 'user' });
    }
    return nodes
      .filter((node) => Array.isArray(node?.reactionGroups) && node.reactionGroups.some((group) => group?.content === 'THUMBS_UP' && group.viewerHasReacted === true))
      .map((node) => Number(node.fullDatabaseId))
      .filter((id) => Number.isSafeInteger(id) && id > 0);
  }

  // Sessions

  function readSession(ctx) {
    const raw = parseCookies(ctx.request.headers.cookie)[names.session];
    if (raw === undefined) return null;
    const session = unseal(config.sessionSecret, 'session', raw);
    if (!session || session.v !== 1 || typeof session.login !== 'string' || !(session.exp > now()) || rejected.has(hash(session.t), now())) {
      clearSession(ctx);
      return null;
    }
    return session;
  }

  function writeSession(ctx, session, { hint = false } = {}) {
    const maxAge = Math.max(0, Math.floor((session.exp - now()) / 1000));
    ctx.cookies.push(serializeCookie(names.session, seal(config.sessionSecret, 'session', session), { maxAge, secure }));
    if (hint) ctx.cookies.push(serializeCookie(HINT_COOKIE, '1', { maxAge, secure, httpOnly: false }));
  }

  function clearSession(ctx) {
    ctx.session = null;
    if (ctx.cleared) return;
    ctx.cleared = true;
    ctx.cookies.push(clearCookie(names.session, { secure }), clearCookie(HINT_COOKIE, { secure, httpOnly: false }));
  }

  // GitHub's token endpoint, within `ceiling`: { data } on success, { invalid } when GitHub refused.
  async function oauthToken(params, ceiling) {
    spendOrRefuse(ceiling);
    const { data } = await github.request('POST', `${config.webBase}/login/oauth/access_token`, {
      accept: 'application/json',
      as: 'client',
      body: { client_id: config.clientId, client_secret: config.clientSecret, ...params },
    });
    if (data?.access_token) return { data };
    return { invalid: true, error: typeof data?.error === 'string' ? data.error : 'unknown' };
  }

  function tokenFields(data) {
    if (!data.refresh_token || !data.expires_in) return null;
    const at = now();
    return {
      t: data.access_token,
      te: at + Number(data.expires_in) * 1000,
      r: data.refresh_token,
      re: at + Number(data.refresh_token_expires_in || 0) * 1000,
    };
  }

  // { fields } after a refresh, { invalid } when GitHub says the refresh token is no good, { failed } otherwise.
  function refresh(session) {
    const key = hash(session.r);
    let pending = refreshing.get(key);
    if (!pending) {
      pending = detach(() => oauthToken({ grant_type: 'refresh_token', refresh_token: session.r }, 'renew'))
        .then((result) => {
          if (result.data) {
            const fields = tokenFields(result.data);
            return fields ? { fields } : { invalid: true };
          }
          return result.error === 'bad_refresh_token' ? { invalid: true } : { failed: true };
        }, (error) => ({ failed: true, error }));
      refreshing.set(key, pending);
      // A successful refresh is shared for 30 seconds with requests still carrying the old cookie; a failure is not kept.
      pending.then((result) => {
        if (result.fields) setTimeout(() => refreshing.delete(key), 30_000).unref?.();
        else refreshing.delete(key);
      });
    }
    return bounded(pending);
  }

  // The signed-in person, or null. With `refresh`, the user token is valid for at least five more minutes.
  async function currentUser(ctx, { refresh: renew = true } = {}) {
    if (ctx.session !== undefined) return ctx.session;
    let session = readSession(ctx);
    if (renew && session && session.te - now() < REFRESH_MARGIN) {
      const result = session.re > now() ? await refresh(session) : { invalid: true };
      if (result.fields) {
        session = { ...session, ...result.fields };
        writeSession(ctx, session);
      } else if (result.invalid) {
        rejected.add(hash(session.t), now());
        clearSession(ctx);
        session = null;
      } else {
        throw result.error instanceof GitHubError ? result.error : new GitHubError(502, { as: 'client' });
      }
    }
    ctx.session = session;
    return session;
  }

  async function requireUser(ctx, options) {
    const session = await currentUser(ctx, options);
    if (!session) refuse(401, 'signed_out', 'Sign in with GitHub first.');
    return session;
  }

  // Writes per person a minute.
  function countWrite(session) {
    const wait = writes.take(session.id, BUDGET.writesPerMinute, now());
    if (wait) tooMany(wait, 'Too many changes in a minute. Try again shortly.');
  }

  async function writer(ctx) {
    const session = await requireUser(ctx);
    countWrite(session);
    return session;
  }

  // Input

  function readJson(ctx) {
    let value;
    try {
      value = JSON.parse(ctx.request.body || '');
    } catch {
      refuse(400, 'bad_input', 'The request was not understood.');
    }
    if (!value || typeof value !== 'object' || Array.isArray(value)) refuse(400, 'bad_input', 'The request was not understood.');
    return value;
  }

  function issueNumber(value) {
    if (!Number.isSafeInteger(value) || value < 1 || value > 999_999_999) refuse(400, 'bad_input', 'That issue number is not valid.');
    return value;
  }

  function queryNumber(ctx) {
    const raw = ctx.url.searchParams.get('n') || '';
    if (!/^[1-9]\d{0,8}$/.test(raw)) refuse(400, 'bad_input', 'That issue number is not valid.');
    return Number(raw);
  }

  function text(value, { limit = LIMITS.text, required = true, what = 'Text', oneLine = false } = {}) {
    if (value === undefined || value === null) value = '';
    if (typeof value !== 'string') refuse(400, 'bad_input', `${what} must be text.`);
    if (oneLine && TITLE_FORBIDDEN.test(value)) refuse(400, 'bad_input', `${what} must be one line.`);
    const trimmed = value.replace(DIRECTION_CONTROLS, '').trim();
    if (required && !trimmed) refuse(400, 'empty', `${what} is empty.`);
    if ([...trimmed].length > limit) refuse(400, 'too_long', `${what} is longer than ${limit.toLocaleString('en-US')} characters.`);
    return trimmed;
  }

  // Routes

  const routes = new Map(Object.entries({
    // Answers at once from the snapshot it has, with `old` when that is over SNAPSHOT.maxAge old.
    // `fresh=1`, the page's second ask, waits for a refresh up to SNAPSHOT.freshWait.
    async 'GET state'(ctx) {
      const values = ctx.url.searchParams.getAll('fresh');
      const fresh = values.length === 1 && values[0] === '1';
      const { snapshot, stale, old } = await readSnapshot(ctx, fresh ? SNAPSHOT.freshWait : 0);
      const body = snapshots.stateAnswer(snapshot);
      const cache = fresh ? (old ? PRIVATE : FRESH_READ) : STATE_READ;
      return json(200, stale || old ? { ...body, ...(stale ? { stale: true } : {}), ...(old ? { old: true } : {}) } : body, { 'Cache-Control': cache });
    },

    // Reads only the cookie: no call to GitHub.
    async 'GET me'(ctx) {
      const session = await currentUser(ctx, { refresh: false });
      if (!session) {
        clearSession(ctx);
        refuse(401, 'signed_out', 'Sign in with GitHub first.');
      }
      return json(200, { login: session.login, avatar: session.avatar });
    },

    async 'GET issue'(ctx) {
      const n = queryNumber(ctx);
      const { snapshot, stale } = await readSnapshot(ctx, SNAPSHOT.ageWait);
      const answer = snapshots.issueAnswer(snapshot, n);
      if (!answer) refuse(404, 'not_found', 'This is not a Workshop issue.');
      return json(200, stale ? { ...answer, stale: true } : answer, { 'Cache-Control': PUBLIC_READ });
    },

    async 'GET vote'(ctx) {
      const n = queryNumber(ctx);
      const session = await requireUser(ctx);
      await workshopIssue(session, n);
      const reactions = await userPages(session, `/repos/${repo}/issues/${n}/reactions?content=%2B1`);
      const comments = await reactedComments(session, n);
      return json(200, { on: reactions.some((reaction) => reaction.user?.id === session.id), comments });
    },

    async 'GET login'(ctx) {
      const back = returnPath(ctx.url.searchParams.get('return'));
      const state = randomBytes(32).toString('base64url');
      const verifier = randomBytes(32).toString('base64url');
      const challenge = createHash('sha256').update(verifier).digest('base64url');
      ctx.cookies.push(serializeCookie(names.state, seal(config.sessionSecret, 'signin', { s: state, v: verifier, r: back, exp: now() + STATE_MAX_AGE * 1000 }), { maxAge: STATE_MAX_AGE, secure }));
      const target = new URL(`${config.webBase}/login/oauth/authorize`);
      target.search = new URLSearchParams({
        client_id: config.clientId,
        redirect_uri: callbackUrl(),
        state,
        code_challenge: challenge,
        code_challenge_method: 'S256',
      }).toString();
      return redirect(target.toString());
    },

    // A sign-in cookie is good for one callback on this instance.
    async 'GET callback'(ctx) {
      const raw = parseCookies(ctx.request.headers.cookie)[names.state];
      ctx.cookies.push(clearCookie(names.state, { secure }));
      const saved = raw === undefined ? null : unseal(config.sessionSecret, 'signin', raw);
      const state = ctx.url.searchParams.get('state') || '';
      if (!saved || !(saved.exp > now()) || !sameText(saved.s, state)) refuse(400, 'bad_state', 'Sign-in expired. Try again.');
      if (usedSignins.has(saved.s, now())) refuse(400, 'bad_state', 'Sign-in expired. Try again.');
      usedSignins.add(saved.s, now());
      const back = returnPath(saved.r);
      if (ctx.url.searchParams.get('error')) return redirect(back);
      const code = ctx.url.searchParams.get('code') || '';
      if (!code || code.length > 512) refuse(400, 'bad_input', 'Sign-in did not finish. Try again.');
      const repositoryId = await app.repoId(async () => {
        spendOrRefuse('signin');
        return (await installationRequest('GET', `/repos/${repo}`)).data.id;
      });
      const result = await oauthToken({ code, redirect_uri: callbackUrl(), code_verifier: saved.v, repository_id: String(repositoryId) }, 'exchange');
      if (!result.data) refuse(400, 'bad_code', 'Sign-in did not finish. Try again.');
      const fields = tokenFields(result.data);
      if (!fields) {
        log({ endpoint: 'callback', failure: 'token_does_not_expire' });
        refuse(502, 'github_failed', 'Sign-in is not set up correctly.');
      }
      const { data: user } = await github.request('GET', '/user', { token: fields.t, as: 'user' });
      const session = { v: 1, id: user.id, login: user.login, avatar: user.avatar_url, ...fields, exp: now() + SESSION_MAX_AGE * 1000 };
      writeSession(ctx, session, { hint: true });
      return redirect(back);
    },

    // Revokes the person's whole grant for the App (every session) with the token the cookie holds,
    // never waiting on a refresh, then clears the cookies whatever happened. `revoked` says whether
    // GitHub confirmed the revocation.
    async 'POST logout'(ctx) {
      const session = readSession(ctx);
      clearSession(ctx);
      if (!session) return json(200, { ok: true, revoked: false });
      countWrite(session);
      const basic = Buffer.from(`${config.clientId}:${config.clientSecret}`).toString('base64');
      const revoked = await github.request('DELETE', `/applications/${encodeURIComponent(config.clientId)}/grant`, { token: basic, auth: 'Basic', as: 'client', body: { access_token: session.t } })
        .then(() => true, () => false);
      if (revoked) rejected.add(hash(session.t), now());
      return json(200, { ok: true, revoked });
    },

    // `comments` is the count GitHub gave in the read before the write, plus the comment this request
    // posted; absent when that read gave none. With `entry`, the answer carries `n`, the thread.
    async 'POST comment'(ctx) {
      const session = await writer(ctx);
      const input = readJson(ctx);
      const named = target(input);
      const body = text(input.text, { what: 'The comment' });
      const { n, issue, fresh } = named.entry ? await threadOf(ctx, session, named.entry, { open: true }) : await namedIssue(session, named.n);
      // An issue this request just opened has no comment to find.
      const existing = fresh ? null : await alreadyPosted(session, n, body);
      const data = existing ?? (await onThread(session, named, n, () => userRequest(session, 'POST', `/repos/${repo}/issues/${n}/comments`, { body: { body } }))).data;
      if (data && !existing) snapshots.patch.comment(n, data);
      const read = countOf(issue.comments);
      return json(200, { ok: true, id: data?.id ?? null, ...(read === null ? {} : { comments: read + (existing ? 0 : 1) }), ...(named.entry ? { n } : {}) });
    },

    // `up` is the 👍 count GitHub gave in the read before the write, moved by what this request did;
    // absent when that read gave none. With `entry`, the answer carries `n`, the thread; removing a 👍
    // from an entry without a thread opens nothing.
    async 'POST vote'(ctx) {
      const session = await writer(ctx);
      const input = readJson(ctx);
      const named = target(input);
      if (typeof input.on !== 'boolean') refuse(400, 'bad_input', 'The request was not understood.');
      const thread = named.entry ? await threadOf(ctx, session, named.entry, { open: input.on }) : await namedIssue(session, named.n);
      if (!thread) return json(200, { ok: true, up: 0 });
      const { n, issue } = thread;
      let up = countOf(issue.reactions?.['+1']);
      if (input.on) {
        const { status } = await onThread(session, named, n, () => userRequest(session, 'POST', `/repos/${repo}/issues/${n}/reactions`, { body: { content: '+1' } }));
        if (status === 201) {
          snapshots.patch.up(n, 1);
          if (up !== null) up += 1;
        }
      } else {
        const mine = (await userPages(session, `/repos/${repo}/issues/${n}/reactions?content=%2B1`)).filter((reaction) => reaction.user?.id === session.id);
        for (const reaction of mine) {
          await onThread(session, named, n, () => userRequest(session, 'DELETE', `/repos/${repo}/issues/${n}/reactions/${reaction.id}`));
          snapshots.patch.up(n, -1);
          if (up !== null) up = Math.max(0, up - 1);
        }
      }
      return json(200, { ok: true, ...(up === null ? {} : { up }), ...(named.entry ? { n } : {}) });
    },

    // The person's 👍 on comment `comment` of Workshop issue `n`. `up` is counted as `POST vote` counts
    // it, from the comment read before the write.
    async 'POST comment-vote'(ctx) {
      const session = await writer(ctx);
      const input = readJson(ctx);
      const n = issueNumber(input.n);
      const id = input.comment;
      if (!Number.isSafeInteger(id) || id < 1) refuse(400, 'bad_input', 'That comment is not valid.');
      if (typeof input.on !== 'boolean') refuse(400, 'bad_input', 'The request was not understood.');
      await workshopIssue(session, n);
      const read = await userRequest(session, 'GET', `/repos/${repo}/issues/comments/${id}`).catch((error) => {
        if (error instanceof GitHubError && (error.status === 404 || error.status === 410)) return null;
        throw error;
      });
      if (!read || commentIssue(read.data) !== n) refuse(404, 'not_found', 'This is not a comment of that issue.');
      let up = countOf(read.data.reactions?.['+1']);
      if (input.on) {
        const { status } = await userRequest(session, 'POST', `/repos/${repo}/issues/comments/${id}/reactions`, { body: { content: '+1' } });
        if (status === 201) {
          snapshots.patch.commentUp(n, id, 1);
          if (up !== null) up += 1;
        }
      } else {
        const mine = (await userPages(session, `/repos/${repo}/issues/comments/${id}/reactions?content=%2B1`)).filter((reaction) => reaction.user?.id === session.id);
        for (const reaction of mine) {
          await userRequest(session, 'DELETE', `/repos/${repo}/issues/comments/${id}/reactions/${reaction.id}`);
          snapshots.patch.commentUp(n, id, -1);
          if (up !== null) up = Math.max(0, up - 1);
        }
      }
      return json(200, up === null ? { ok: true } : { ok: true, up });
    },

    // At most 3 suggestions a minute and 10 an hour per person; nothing is created when the label
    // ceiling has no room left, and the label is charged when its call is made.
    async 'POST suggest'(ctx) {
      const session = await writer(ctx);
      const input = readJson(ctx);
      const title = text(input.title, { limit: LIMITS.title, what: 'The title', oneLine: true });
      const body = text(input.text, { required: false, what: 'The description' });
      const minute = suggestionsMinute.peek(session.id, BUDGET.suggestionsPerMinute, now());
      if (minute) tooMany(minute, 'Too many suggestions. Try again shortly.');
      const hour = suggestionsHour.peek(session.id, BUDGET.suggestionsPerHour, now());
      if (hour) tooMany(hour, 'Too many suggestions this hour.');
      if (!hasRoom('label')) spendOrRefuse('label');
      const { data } = await userRequest(session, 'POST', `/repos/${repo}/issues`, { body: { title, body } });
      // An attempt counts only once the issue exists.
      suggestionsMinute.take(session.id, BUDGET.suggestionsPerMinute, now());
      suggestionsHour.take(session.id, BUDGET.suggestionsPerHour, now());
      const number = data.number;
      try {
        await labelSuggestion(number).catch(() => labelSuggestion(number));
      } catch (error) {
        if (error instanceof GitHubError) ctx.githubStatus = error.status;
        unlabelled.set(number, 0);
        if (unlabelled.size > UNLABELLED_LIMIT) unlabelled.delete(unlabelled.keys().next().value);
        refuse(502, 'github', 'The suggestion was posted on GitHub but could not be added to the Workshop.', { number });
      }
      snapshots.patch.issue(data, 'suggestion');
      if (unlabelled.size) retryLabels().catch(() => {});
      return json(200, { number });
    },

  }));

  function callbackUrl() {
    return `${config.origin}/api/workshop/callback`;
  }

  // Settles once no background work (snapshot refresh, label retry) is running.
  async function idle() {
    await snapshots?.idle();
    await retrying?.catch(() => {});
  }

  return { handle, idle };
}

// A path on this site under /workshop, or a refusal.
export function returnPath(value) {
  if (value === null || value === undefined || value === '') return '/workshop';
  if (typeof value !== 'string' || value.length > 512 || !/^\/workshop(?:[/?#]|$)/.test(value) || /[\\\s\u0000-\u001f\u007f]/.test(value) || value.includes('//')) {
    refuse(400, 'bad_return', 'That return address is not part of the Workshop.');
  }
  const resolved = new URL(value, 'https://workshop.invalid');
  if (resolved.origin !== 'https://workshop.invalid' || !/^\/workshop(?:\/|$)/.test(resolved.pathname)) refuse(400, 'bad_return', 'That return address is not part of the Workshop.');
  return resolved.pathname + resolved.search + resolved.hash;
}

function sameText(a, b) {
  const left = Buffer.from(String(a));
  const right = Buffer.from(String(b));
  return left.length === right.length && timingSafeEqual(left, right);
}

function json(status, body, headers = {}) {
  return { status, headers: { 'Content-Type': 'application/json; charset=utf-8', ...headers }, body: JSON.stringify(body) };
}

function redirect(location) {
  return { status: 302, headers: { Location: location, 'Cache-Control': PRIVATE }, body: '' };
}

function defaultLog(entry) {
  console.log(JSON.stringify({ workshop: entry }));
}
