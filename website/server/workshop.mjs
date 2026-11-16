// The Workshop API: every endpoint under /api/workshop/, independent of the
// HTTP server that runs it. A request is { method, url, headers, body } and a
// response is { status, headers, cookies, body }.
//
// Identities: anonymous reads are answered from the in-memory snapshot
// (snapshot.mjs), which alone reads GitHub for them, with the installation
// token. Everything done for a signed-in person uses that person's token,
// except the permission check and labelling a suggestion.
import { AsyncLocalStorage } from 'node:async_hooks';
import { createHash, randomBytes, timingSafeEqual } from 'node:crypto';
import { createAppAuth } from './app-auth.mjs';
import { createGitHub, DEFAULT_TIMEOUT, GitHubError } from './github.mjs';
import { kindOf, LABELS, labelNames, reasonOf, VERDICTS } from './issues.mjs';
import { clearCookie, cookieNames, HINT_COOKIE, parseCookies, seal, serializeCookie, SESSION_MAX_AGE, STATE_MAX_AGE, unseal } from './session.mjs';
import { createSnapshot } from './snapshot.mjs';

export { LABELS, VERDICTS };

const DIRECTION_CONTROLS = /[\u202A-\u202E\u2066-\u2069]/g;
const TITLE_FORBIDDEN = /[\u0000-\u001F\u007F-\u009F\u2028\u2029]/;
export const LIMITS = { text: 5000, title: 120 };
// Ceilings per instance and minute: one for each use of the installation token, kept
// apart so that no use can starve another, and the writes each person may make.
export const BUDGET = { refreshCallsPerMinute: 60, permissionChecksPerMinute: 12, signinLookupsPerMinute: 4, labelCallsPerMinute: 6, writesPerMinute: 30 };
const CEILINGS = { refresh: 'refreshCallsPerMinute', permission: 'permissionChecksPerMinute', signin: 'signinLookupsPerMinute', label: 'labelCallsPerMinute' };
export const DEADLINE = 12_000;
const RECENT_DECISION = 5 * 60_000;
const PERMISSION_TTL = 5 * 60_000;
const REFRESH_MARGIN = 5 * 60_000;
const MAX_BLOCK = 60 * 60_000;
const UNLABELLED_LIMIT = 100;
const SIGNIN_FAILED = '/workshop?signin=failed';

const PUBLIC_READ = 'public, s-maxage=30, stale-while-revalidate=300';
const PRIVATE = 'private, no-store';

const ENDPOINTS = new Map(Object.entries({
  state: ['GET'], me: ['GET'], issue: ['GET'], login: ['GET'], callback: ['GET'], vote: ['GET', 'POST'],
  logout: ['POST'], comment: ['POST'], suggest: ['POST'], verdict: ['POST'], decide: ['POST'],
}));

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

export function createWorkshop({ config, fetch = globalThis.fetch, now = Date.now, log = defaultLog, timeoutMs = DEFAULT_TIMEOUT, deadlineMs = DEADLINE }) {
  // Every handler's GitHub calls share one deadline; background work runs outside it.
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
  const permissionCache = new Map();
  const permissionPending = new Map();
  const refreshing = new Map();
  const writes = new Map();
  const unlabelled = new Set();

  async function handle(request) {
    const ctx = { request, endpoint: null, cookies: [], headers: {} };
    let response;
    try {
      let url;
      try {
        url = new URL(request.url, 'http://workshop.invalid');
      } catch {
        refuse(404, 'not_found', 'There is nothing here.');
      }
      ctx.url = url;
      const endpoint = url.pathname.match(/^\/api\/workshop\/([a-z]+)$/)?.[1];
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
      response = await requests.run({ deadline: Date.now() + deadlineMs }, () => route(ctx));
    } catch (error) {
      response = errorResponse(ctx, error);
      if ((ctx.endpoint === 'login' || ctx.endpoint === 'callback') && response.status !== 405) response = redirect(SIGNIN_FAILED);
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
        clearSession(ctx);
        return json(401, { error: 'signed_out', message: 'Sign in again.' });
      }
      if (error.status === 401) return json(502, { error: 'github_failed', message: 'GitHub did not answer. Try again later.' });
      if (error.status === 404 || error.status === 410) return json(404, { error: 'not_found', message: 'This is not a Workshop issue.' });
      if (error.status === 403) return json(403, { error: 'forbidden', message: 'GitHub did not allow this.' });
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

  // The installation token: budget, blocking and calls

  function blockFor(seconds) {
    blockedUntil = Math.max(blockedUntil, now() + Math.min((seconds || 60) * 1000, MAX_BLOCK));
  }

  function blockedError() {
    return new GitHubError(429, { rateLimited: true, retryAfter: Math.ceil((blockedUntil - now()) / 1000), as: 'installation' });
  }

  function noteInstallationError(error) {
    if (!(error instanceof GitHubError)) return;
    if (error.rateLimited) blockFor(error.retryAfter);
    if (error.status === 401) app.dropToken();
  }

  // Takes `cost` calls from this minute's ceiling `name`, or answers false when it is spent or GitHub asked us to wait.
  function spend(name, cost = 1) {
    const at = now();
    if (at < blockedUntil) return false;
    const ceiling = ceilings[name];
    if (at - ceiling.start >= 60_000) {
      ceiling.start = at;
      ceiling.used = 0;
    }
    if (ceiling.used + cost > BUDGET[CEILINGS[name]]) return false;
    ceiling.used += cost;
    return true;
  }

  function spendOrRefuse(name, cost = 1) {
    if (spend(name, cost)) return;
    const at = now();
    const wait = Math.max(blockedUntil - at, ceilings[name].start + 60_000 - at, 1000);
    throw new GitHubError(429, { rateLimited: true, retryAfter: Math.ceil(wait / 1000), as: 'installation' });
  }

  // Every installation-token call goes through here: nothing reaches GitHub while it has asked us to wait.
  async function asInstallation(call) {
    if (now() < blockedUntil) throw blockedError();
    try {
      return await call(await app.installationToken());
    } catch (error) {
      noteInstallationError(error);
      throw error;
    }
  }

  const installationRequest = (method, path, options = {}) => asInstallation((token) => github.request(method, path, { ...options, token, as: 'installation' }));
  const snapshots = config.configured ? createSnapshot({
    repo,
    now,
    log,
    detach,
    afterRefresh: () => { if (unlabelled.size) retryLabels().catch(() => {}); },
    request: (method, path, options) => { spendOrRefuse('refresh'); return installationRequest(method, path, options); },
  }) : null;
  const userRequest = (session, method, path, options = {}) => github.request(method, path, { ...options, token: session.t, as: 'user' });
  const userPages = (session, path) => github.paginate(path, { token: session.t, as: 'user' });

  // Shares one in-flight call per key between concurrent requests.
  function shared(map, key, start) {
    let pending = map.get(key);
    if (!pending) {
      pending = start().finally(() => map.delete(key));
      map.set(key, pending);
    }
    return pending;
  }

  // GitHub data

  // The issue as the signed-in person sees it, refused unless it is a Workshop issue.
  async function workshopIssue(session, n) {
    const { data: issue } = await userRequest(session, 'GET', `/repos/${repo}/issues/${n}`);
    const kind = kindOf(issue);
    if (!kind) refuse(404, 'not_found', 'This is not a Workshop issue.');
    return { issue, kind };
  }

  // The snapshot for an anonymous read. A cold instance waits for its first refresh, at most until the
  // request's deadline (the refresh goes on in the background); with none, 502, or 503 when the App
  // is not installed on the repository.
  async function readSnapshot(ctx) {
    const left = requests.getStore().deadline - Date.now();
    let timer;
    try {
      return await Promise.race([
        snapshots.read(),
        new Promise((_, reject) => { timer = setTimeout(() => reject(new GitHubError(0, { timeout: true })), Math.max(0, left)); }),
      ]);
    } catch (error) {
      if (error instanceof GitHubError) ctx.githubStatus = error.status;
      if (error instanceof GitHubError && error.notInstalled) refuse(503, 'not_configured', 'Live comments are not set up on this server.');
      refuse(502, 'github_failed', 'GitHub did not answer. Try again later.');
    } finally {
      clearTimeout(timer);
    }
  }

  function cachedPermission(login) {
    const cached = permissionCache.get(login);
    return cached && now() - cached.at < PERMISSION_TTL ? cached : null;
  }

  function readPermission(login) {
    return shared(permissionPending, login, async () => {
      spendOrRefuse('permission');
      const { data } = await installationRequest('GET', `/repos/${repo}/collaborators/${encodeURIComponent(login)}/permission`).catch((error) => {
        if (error instanceof GitHubError && error.status === 404) return { data: null };
        throw error;
      });
      const value = ['admin', 'maintain', 'write'].includes(data?.role_name) || ['admin', 'write'].includes(data?.permission);
      permissionCache.delete(login);
      permissionCache.set(login, { value, at: now() });
      if (permissionCache.size > 1000) permissionCache.delete(permissionCache.keys().next().value);
      return value;
    });
  }

  async function canDecide(login) {
    return cachedPermission(login)?.value ?? readPermission(login);
  }

  async function labelSuggestion(number) {
    await installationRequest('POST', `/repos/${repo}/issues/${number}/labels`, { body: { labels: [LABELS.suggestion] } });
  }

  // Suggestions created but not labelled. Retried, outside any request, after a successful suggestion
  // and after each snapshot refresh: an issue is labelled only if it is still open and has no label
  // at all; anything else leaves the list, and a failing number does not hold up the others.
  let retrying = null;
  function retryLabels() {
    retrying ??= detach(async () => {
      for (const number of [...unlabelled]) {
        if (!spend('label', 2)) break;
        try {
          const { data: issue } = await installationRequest('GET', `/repos/${repo}/issues/${number}`);
          if (issue.state === 'open' && !issue.pull_request && labelNames(issue).length === 0) await labelSuggestion(number);
          unlabelled.delete(number);
        } catch (error) {
          if (error instanceof GitHubError && [404, 410, 422].includes(error.status)) unlabelled.delete(number);
        }
      }
    }).finally(() => { retrying = null; });
    return retrying;
  }

  // Sessions

  function readSession(ctx) {
    const raw = parseCookies(ctx.request.headers.cookie)[names.session];
    if (raw === undefined) return null;
    const session = unseal(config.sessionSecret, 'session', raw);
    if (!session || session.v !== 1 || typeof session.login !== 'string' || !(session.exp > now())) {
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

  // GitHub's token endpoint: { data } on success, { invalid } when GitHub refused the grant.
  async function oauthToken(params) {
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
    const key = createHash('sha256').update(session.r).digest('hex');
    let pending = refreshing.get(key);
    if (!pending) {
      pending = oauthToken({ grant_type: 'refresh_token', refresh_token: session.r })
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
    return pending;
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

  function countWrite(session) {
    const at = now();
    let entry = writes.get(session.id);
    if (!entry || at - entry.start >= 60_000) {
      entry = { start: at, count: 0 };
      writes.delete(session.id);
      writes.set(session.id, entry);
      if (writes.size > 10_000) writes.delete(writes.keys().next().value);
    }
    if (entry.count >= BUDGET.writesPerMinute) {
      refuse(429, 'rate_limited', 'Too many changes in a minute. Try again shortly.', {}, { 'Retry-After': String(Math.ceil((entry.start + 60_000 - at) / 1000)) });
    }
    entry.count += 1;
  }

  async function writer(ctx) {
    const session = await requireUser(ctx);
    countWrite(session);
    return session;
  }

  // Confirms the account behind the token and reads its permission again.
  // A cached "cannot decide" refuses without calling GitHub.
  async function requireDecider(session) {
    if (cachedPermission(session.login)?.value === false) refuse(403, 'cannot_decide', 'Only the owner can decide.');
    const { data: user } = await userRequest(session, 'GET', '/user');
    if (user?.id !== session.id) throw new GitHubError(401, { as: 'user' });
    if (!(await readPermission(user.login))) refuse(403, 'cannot_decide', 'Only the owner can decide.');
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
    async 'GET state'(ctx) {
      const { snapshot, stale } = await readSnapshot(ctx);
      const body = snapshots.stateAnswer(snapshot);
      return json(200, stale ? { ...body, stale: true } : body, { 'Cache-Control': PUBLIC_READ });
    },

    // Reads only the cookie; the permission check is the one GitHub call, cached five minutes. When it
    // cannot be read, the answer says the person cannot decide, and the next `me` tries again.
    async 'GET me'(ctx) {
      const session = await currentUser(ctx, { refresh: false });
      if (!session) {
        clearSession(ctx);
        refuse(401, 'signed_out', 'Sign in with GitHub first.');
      }
      let decides = false;
      try {
        decides = await canDecide(session.login);
      } catch (error) {
        if (error instanceof GitHubError) ctx.githubStatus = error.status;
      }
      return json(200, { login: session.login, avatar: session.avatar, canDecide: decides });
    },

    async 'GET issue'(ctx) {
      const n = queryNumber(ctx);
      const { snapshot, stale } = await readSnapshot(ctx);
      const answer = snapshots.issueAnswer(snapshot, n);
      if (!answer) refuse(404, 'not_found', 'This is not a Workshop issue.');
      return json(200, stale ? { ...answer, stale: true } : answer, { 'Cache-Control': PUBLIC_READ });
    },

    async 'GET vote'(ctx) {
      const n = queryNumber(ctx);
      const session = await requireUser(ctx);
      await workshopIssue(session, n);
      const reactions = await userPages(session, `/repos/${repo}/issues/${n}/reactions?content=%2B1`);
      return json(200, { on: reactions.some((reaction) => reaction.user?.id === session.id) });
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

    async 'GET callback'(ctx) {
      const raw = parseCookies(ctx.request.headers.cookie)[names.state];
      ctx.cookies.push(clearCookie(names.state, { secure }));
      const saved = raw === undefined ? null : unseal(config.sessionSecret, 'signin', raw);
      const state = ctx.url.searchParams.get('state') || '';
      if (!saved || !(saved.exp > now()) || !sameText(saved.s, state)) refuse(400, 'bad_state', 'Sign-in expired. Try again.');
      const back = returnPath(saved.r);
      if (ctx.url.searchParams.get('error')) return redirect(back);
      const code = ctx.url.searchParams.get('code') || '';
      if (!code || code.length > 512) refuse(400, 'bad_input', 'Sign-in did not finish. Try again.');
      const repositoryId = await app.repoId(async () => {
        spendOrRefuse('signin');
        return (await installationRequest('GET', `/repos/${repo}`)).data.id;
      });
      const result = await oauthToken({ code, redirect_uri: callbackUrl(), code_verifier: saved.v, repository_id: String(repositoryId) });
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
      return json(200, { ok: true, revoked });
    },

    async 'POST comment'(ctx) {
      const session = await writer(ctx);
      const input = readJson(ctx);
      const n = issueNumber(input.n);
      const body = text(input.text, { what: 'The comment' });
      await workshopIssue(session, n);
      const { data } = await userRequest(session, 'POST', `/repos/${repo}/issues/${n}/comments`, { body: { body } });
      if (data) snapshots.patch.comment(n, data);
      return json(200, { ok: true, id: data?.id ?? null });
    },

    async 'POST vote'(ctx) {
      const session = await writer(ctx);
      const input = readJson(ctx);
      const n = issueNumber(input.n);
      if (typeof input.on !== 'boolean') refuse(400, 'bad_input', 'The request was not understood.');
      await workshopIssue(session, n);
      if (input.on) {
        const { status } = await userRequest(session, 'POST', `/repos/${repo}/issues/${n}/reactions`, { body: { content: '+1' } });
        if (status === 201) snapshots.patch.up(n, 1);
      } else {
        const mine = (await userPages(session, `/repos/${repo}/issues/${n}/reactions?content=%2B1`)).filter((reaction) => reaction.user?.id === session.id);
        for (const reaction of mine) {
          await userRequest(session, 'DELETE', `/repos/${repo}/issues/${n}/reactions/${reaction.id}`);
          snapshots.patch.up(n, -1);
        }
      }
      return json(200, { ok: true });
    },

    async 'POST suggest'(ctx) {
      const session = await writer(ctx);
      const input = readJson(ctx);
      const title = text(input.title, { limit: LIMITS.title, what: 'The title', oneLine: true });
      const body = text(input.text, { required: false, what: 'The description' });
      spendOrRefuse('label', 2);
      const { data } = await userRequest(session, 'POST', `/repos/${repo}/issues`, { body: { title, body } });
      const number = data.number;
      try {
        await labelSuggestion(number).catch(() => labelSuggestion(number));
      } catch (error) {
        if (error instanceof GitHubError) ctx.githubStatus = error.status;
        unlabelled.add(number);
        if (unlabelled.size > UNLABELLED_LIMIT) unlabelled.delete(unlabelled.values().next().value);
        refuse(502, 'github', 'The suggestion was posted on GitHub but could not be added to the Workshop.', { number });
      }
      snapshots.patch.issue(data, 'suggestion');
      if (unlabelled.size) retryLabels().catch(() => {});
      return json(200, { number });
    },

    // Labels and state change first; the note is posted after they succeeded, so a retry never posts it twice.
    async 'POST verdict'(ctx) {
      const session = await writer(ctx);
      const input = readJson(ctx);
      const n = issueNumber(input.n);
      if (!Object.hasOwn(input, 'verdict')) refuse(400, 'bad_input', 'The verdict is missing.');
      const verdict = input.verdict;
      if (verdict !== null && !VERDICTS.includes(verdict)) refuse(400, 'bad_input', 'That verdict is not known.');
      const note = text(input.note, { required: verdict === 'refine', what: 'The note' });
      await requireDecider(session);
      const { issue, kind } = await workshopIssue(session, n);
      if (kind !== 'entry') refuse(404, 'not_found', 'This is not a Workshop entry.');
      const target = verdict ? `verdict:${verdict}` : null;
      const current = labelNames(issue).filter((name) => name.startsWith('verdict:'));
      for (const name of current) {
        if (name === target) continue;
        await userRequest(session, 'DELETE', `/repos/${repo}/issues/${n}/labels/${encodeURIComponent(name)}`).catch((error) => {
          if (!(error instanceof GitHubError && error.status === 404)) throw error;
        });
      }
      if (target && !current.includes(target)) await userRequest(session, 'POST', `/repos/${repo}/issues/${n}/labels`, { body: { labels: [target] } });
      const dropped = issue.state === 'closed' && issue.state_reason === 'not_planned';
      let state = { state: issue.state === 'closed' ? 'closed' : 'open', reason: reasonOf(issue) };
      if (verdict === 'drop' && !dropped) {
        await userRequest(session, 'PATCH', `/repos/${repo}/issues/${n}`, { body: { state: 'closed', state_reason: 'not_planned' } });
        state = { state: 'closed', reason: 'not_planned', closedAt: new Date(now()).toISOString() };
      } else if (verdict !== 'drop' && dropped) {
        await userRequest(session, 'PATCH', `/repos/${repo}/issues/${n}`, { body: { state: 'open', state_reason: 'reopened' } });
        state = { state: 'open', reason: null };
      }
      snapshots.patch.fields(n, { verdict, ...state, ...(state.state === 'open' ? { closedAt: null } : {}) });
      // The note comes last, after the change it explains succeeded, so a retry never posts it twice.
      if (note) {
        const { data } = await userRequest(session, 'POST', `/repos/${repo}/issues/${n}/comments`, { body: { body: note } });
        if (data) snapshots.patch.comment(n, data);
      }
      return json(200, { ok: true });
    },

    async 'POST decide'(ctx) {
      const session = await writer(ctx);
      const input = readJson(ctx);
      const n = issueNumber(input.n);
      const body = text(input.text, { what: 'The decision' });
      await requireDecider(session);
      const { issue, kind } = await workshopIssue(session, n);
      if (kind !== 'decision' || issue.state !== 'open') refuse(404, 'not_found', 'This is not an open Workshop decision.');
      // Comment, then close. A retry after a cut between the two finds the same comment from the same
      // person in the last five minutes and does not post it again.
      const since = new Date(now() - RECENT_DECISION).toISOString().replace(/\.\d{3}Z$/, 'Z');
      const { data: recent } = await userRequest(session, 'GET', `/repos/${repo}/issues/${n}/comments?since=${since}&per_page=100`);
      const already = (Array.isArray(recent) ? recent : []).some((comment) => comment.user?.id === session.id && (comment.body || '').trim() === body);
      const data = already ? null : (await userRequest(session, 'POST', `/repos/${repo}/issues/${n}/comments`, { body: { body } })).data;
      await userRequest(session, 'PATCH', `/repos/${repo}/issues/${n}`, { body: { state: 'closed', state_reason: 'completed' } });
      if (data) snapshots.patch.comment(n, data);
      snapshots.patch.fields(n, { state: 'closed', reason: 'completed', closedAt: new Date(now()).toISOString() });
      return json(200, { ok: true });
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
