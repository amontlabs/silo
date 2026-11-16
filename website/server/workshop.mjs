// The Workshop API: every endpoint under /api/workshop/, independent of the
// HTTP server that runs it. A request is { method, url, headers, body } and a
// response is { status, headers, cookies, body }.
import { createHash, randomBytes, timingSafeEqual } from 'node:crypto';
import { createAppAuth } from './app-auth.mjs';
import { createGitHub, GitHubError } from './github.mjs';
import { clearCookie, cookieNames, parseCookies, seal, serializeCookie, SESSION_MAX_AGE, STATE_MAX_AGE, unseal } from './session.mjs';

export const LABELS = { entry: 'workshop:entry', decision: 'workshop:decision', suggestion: 'workshop:suggestion' };
export const VERDICTS = ['take', 'refine', 'later', 'drop'];
const KIND_BY_LABEL = new Map(Object.entries(LABELS).map(([kind, label]) => [label, kind]));
const ENTRY_MARKER = /^\s*<!--\s*silo-entry:\s*([A-Za-z0-9_.-]+)\s*-->\s*$/m;
const ENTRY_MARKER_LINES = /^\s*<!--\s*silo-entry:[^\n]*-->[ \t]*(\r?\n|$)/gm;

export const LIMITS = { text: 5000, title: 120 };
const STATE_TTL = 60_000;
const STATE_STALE_LIMIT = 10 * 60_000;
const ISSUE_TTL = 30_000;
const ISSUE_CACHE_SIZE = 200;
const PERMISSION_TTL = 5 * 60_000;
const REFRESH_MARGIN = 5 * 60_000;
const MAX_BLOCK = 60 * 60_000;

const PUBLIC_STATE = 'public, s-maxage=60, stale-while-revalidate=300';
const PUBLIC_STALE = 'public, s-maxage=15, stale-while-revalidate=60';
const PUBLIC_ISSUE = 'public, s-maxage=30, stale-while-revalidate=300';
const PRIVATE = 'private, no-store';

const ENDPOINTS = {
  state: 'GET', me: 'GET', issue: 'GET', login: 'GET', callback: 'GET',
  logout: 'POST', comment: 'POST', vote: 'POST', suggest: 'POST', verdict: 'POST', decide: 'POST',
};

class Refusal extends Error {
  constructor(status, code, message) {
    super(message);
    this.status = status;
    this.code = code;
  }
}

const refuse = (status, code, message) => { throw new Refusal(status, code, message); };

export function createWorkshop({ config, fetch = globalThis.fetch, now = Date.now, log = defaultLog }) {
  const github = createGitHub({ apiBase: config.apiBase, fetch, now });
  const app = config.configured ? createAppAuth({ config, github, now }) : null;
  const names = cookieNames(config.secureCookies);
  const repo = config.repo;
  const secure = config.secureCookies;

  let stateCache = null;
  let statePending = null;
  let stateBlockedUntil = 0;
  const issueCache = new Map();
  const permissionCache = new Map();
  const refreshing = new Map();

  async function handle(request) {
    const url = new URL(request.url, 'http://workshop.invalid');
    const endpoint = url.pathname.match(/^\/api\/workshop\/([a-z]+)$/)?.[1];
    const ctx = { request, url, endpoint, cookies: [], headers: {} };
    let response;
    try {
      if (!endpoint || !(endpoint in ENDPOINTS)) refuse(404, 'not_found', 'There is nothing here.');
      if (request.method !== ENDPOINTS[endpoint]) {
        ctx.headers.Allow = ENDPOINTS[endpoint];
        refuse(405, 'method_not_allowed', 'This address does not accept that method.');
      }
      if (!config.configured) refuse(503, 'not_configured', 'Live comments are not set up on this server.');
      if (request.method === 'POST') checkSameSite(request);
      response = await routes[endpoint](ctx);
    } catch (error) {
      response = errorResponse(ctx, error);
    }
    response.headers = { 'Cache-Control': PRIVATE, ...ctx.headers, ...response.headers };
    response.cookies = [...ctx.cookies, ...(response.cookies || [])];
    if (response.cookies.length) response.headers['Cache-Control'] = PRIVATE;
    log({ endpoint: endpoint || 'unknown', status: response.status, ...(ctx.githubStatus !== undefined ? { github: ctx.githubStatus } : {}) });
    return response;
  }

  function errorResponse(ctx, error) {
    if (error instanceof Refusal) return json(error.status, { error: error.code, message: error.message });
    if (error instanceof GitHubError) {
      ctx.githubStatus = error.status;
      if (error.rateLimited) {
        return json(429, { error: 'rate_limited', message: 'GitHub is busy. Try again in a minute.' }, { 'Retry-After': String(Math.min(error.retryAfter || 60, 3600)) });
      }
      if (error.status === 404 || error.status === 410) return json(404, { error: 'not_found', message: 'This is not a Workshop issue.' });
      if (error.status === 401 && ctx.session) {
        ctx.cookies.push(clearCookie(names.session, { secure }));
        return json(401, { error: 'signed_out', message: 'Sign in again.' });
      }
      if (error.status === 403) return json(403, { error: 'forbidden', message: 'GitHub did not allow this.' });
      if (error.status === 422) return json(400, { error: 'rejected', message: 'GitHub did not accept this.' });
      return json(502, { error: 'github_failed', message: 'GitHub did not answer. Try again later.' });
    }
    log({ endpoint: ctx.endpoint || 'unknown', failure: error?.name || 'Error' });
    return json(502, { error: 'github_failed', message: 'Something went wrong. Try again later.' });
  }

  function checkSameSite(request) {
    if (request.headers.origin !== config.origin) refuse(403, 'bad_origin', 'This request did not come from the Workshop.');
    if (request.headers['x-workshop'] !== '1') refuse(403, 'bad_request_header', 'This request did not come from the Workshop.');
  }

  // GitHub data

  async function installation() {
    return app.installationToken();
  }

  function kindOf(issue) {
    if (issue.pull_request) return null;
    for (const label of issue.labels || []) {
      const kind = KIND_BY_LABEL.get(typeof label === 'string' ? label : label.name);
      if (kind) return kind;
    }
    return null;
  }

  function labelNames(issue) {
    return (issue.labels || []).map((label) => (typeof label === 'string' ? label : label.name));
  }

  function verdictOf(issue) {
    const labels = labelNames(issue);
    for (const verdict of VERDICTS) if (labels.includes(`verdict:${verdict}`)) return verdict;
    return null;
  }

  function summary(issue, kind) {
    const reason = issue.state === 'closed' && (issue.state_reason === 'completed' || issue.state_reason === 'not_planned') ? issue.state_reason : null;
    return {
      kind,
      entry: kind === 'entry' ? (issue.body || '').match(ENTRY_MARKER)?.[1] ?? null : null,
      title: issue.title,
      state: issue.state === 'closed' ? 'closed' : 'open',
      reason,
      verdict: verdictOf(issue),
      up: issue.reactions?.['+1'] ?? 0,
      comments: issue.comments ?? 0,
      author: issue.user?.login ?? 'ghost',
      bot: issue.user?.type === 'Bot',
      createdAt: issue.created_at,
      closedAt: issue.closed_at ?? null,
    };
  }

  async function fetchState() {
    const token = await installation();
    const lists = await Promise.all(Object.entries(LABELS).map(([kind, label]) =>
      github.paginate(`/repos/${repo}/issues?labels=${encodeURIComponent(label)}&state=all`, { token }).then((items) => [kind, items])));
    const issues = {};
    for (const [, items] of lists) {
      for (const issue of items) {
        const kind = kindOf(issue);
        if (kind && !(issue.number in issues)) issues[issue.number] = summary(issue, kind);
      }
    }
    return { repo, fetchedAt: new Date(now()).toISOString(), issues };
  }

  function blockFor(error) {
    stateBlockedUntil = now() + Math.min((error.retryAfter || 60) * 1000, MAX_BLOCK);
  }

  async function loadState(ctx) {
    const at = now();
    if (stateCache && at - stateCache.at < STATE_TTL) return { data: stateCache.data, stale: false };
    try {
      if (at < stateBlockedUntil) throw new GitHubError(429, { rateLimited: true, retryAfter: Math.ceil((stateBlockedUntil - at) / 1000) });
      statePending ??= fetchState().then((data) => { stateCache = { data, at: now() }; return data; }).finally(() => { statePending = null; });
      return { data: await statePending, stale: false };
    } catch (error) {
      if (error instanceof GitHubError) {
        ctx.githubStatus = error.status;
        if (error.rateLimited && at >= stateBlockedUntil) blockFor(error);
        if (stateCache && now() - stateCache.at <= STATE_STALE_LIMIT && (error.rateLimited || error.network || error.status >= 500)) {
          return { data: stateCache.data, stale: true };
        }
      }
      throw error;
    }
  }

  async function workshopIssue(n, token) {
    const { data: issue } = await github.request('GET', `/repos/${repo}/issues/${n}`, { token: token ?? await installation() });
    const kind = kindOf(issue);
    if (!kind) refuse(404, 'not_found', 'This is not a Workshop issue.');
    return { issue, kind };
  }

  async function upReactions(n) {
    return github.paginate(`/repos/${repo}/issues/${n}/reactions?content=%2B1`, { token: await installation() });
  }

  async function fetchIssue(n) {
    const token = await installation();
    const { issue } = await workshopIssue(n, token);
    const comments = await github.paginate(`/repos/${repo}/issues/${n}/comments`, { token });
    return {
      number: issue.number,
      title: issue.title,
      body: (issue.body || '').replace(ENTRY_MARKER_LINES, '').replace(/\s+$/, ''),
      url: issue.html_url,
      up: issue.reactions?.['+1'] ?? 0,
      viewerUp: false,
      comments: comments.map((comment) => ({
        id: comment.id,
        author: comment.user?.login ?? 'ghost',
        avatar: comment.user?.avatar_url ?? null,
        bot: comment.user?.type === 'Bot',
        createdAt: comment.created_at,
        text: comment.body || '',
      })),
    };
  }

  function rememberIssue(n, data) {
    issueCache.delete(n);
    issueCache.set(n, { data, at: now() });
    while (issueCache.size > ISSUE_CACHE_SIZE) issueCache.delete(issueCache.keys().next().value);
  }

  function forget(n) {
    stateCache = null;
    if (n !== undefined) issueCache.delete(n);
  }

  async function canDecide(login, { fresh = false } = {}) {
    const cached = permissionCache.get(login);
    if (!fresh && cached && now() - cached.at < PERMISSION_TTL) return cached.value;
    let value = false;
    try {
      const { data } = await github.request('GET', `/repos/${repo}/collaborators/${encodeURIComponent(login)}/permission`, { token: await installation() });
      value = ['admin', 'maintain', 'write'].includes(data?.role_name) || ['admin', 'write'].includes(data?.permission);
    } catch (error) {
      if (!(error instanceof GitHubError && error.status === 404)) throw error;
    }
    permissionCache.set(login, { value, at: now() });
    if (permissionCache.size > 1000) permissionCache.delete(permissionCache.keys().next().value);
    return value;
  }

  // Sessions

  function readSession(ctx) {
    const raw = parseCookies(ctx.request.headers.cookie)[names.session];
    if (raw === undefined) return null;
    const session = unseal(config.sessionSecret, 'session', raw);
    if (!session || session.v !== 1 || typeof session.login !== 'string' || !(session.exp > now())) {
      ctx.cookies.push(clearCookie(names.session, { secure }));
      return null;
    }
    return session;
  }

  function writeSession(ctx, session) {
    const maxAge = Math.max(0, Math.floor((session.exp - now()) / 1000));
    ctx.cookies.push(serializeCookie(names.session, seal(config.sessionSecret, 'session', session), { maxAge, secure }));
  }

  function dropSession(ctx) {
    ctx.session = null;
    ctx.cookies.push(clearCookie(names.session, { secure }));
  }

  async function oauthToken(params) {
    const { data } = await github.request('POST', `${config.webBase}/login/oauth/access_token`, {
      accept: 'application/json',
      body: { client_id: config.clientId, client_secret: config.clientSecret, ...params },
    });
    if (!data || data.error || !data.access_token) return null;
    return data;
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

  async function refresh(session) {
    const key = createHash('sha256').update(session.r).digest('hex');
    let pending = refreshing.get(key);
    if (!pending) {
      pending = oauthToken({ grant_type: 'refresh_token', refresh_token: session.r })
        .then((data) => (data ? tokenFields(data) : null), () => null)
        .finally(() => setTimeout(() => refreshing.delete(key), 30_000).unref?.());
      refreshing.set(key, pending);
    }
    return pending;
  }

  // The signed-in person with a user token that is valid for at least five minutes, or null.
  async function currentUser(ctx) {
    if (ctx.session !== undefined) return ctx.session;
    let session = readSession(ctx);
    if (session && session.te - now() < REFRESH_MARGIN) {
      const fields = session.re > now() ? await refresh(session) : null;
      if (fields) {
        session = { ...session, ...fields };
        writeSession(ctx, session);
      } else {
        dropSession(ctx);
        session = null;
      }
    }
    ctx.session = session;
    return session;
  }

  async function requireUser(ctx) {
    const session = await currentUser(ctx);
    if (!session) refuse(401, 'signed_out', 'Sign in with GitHub first.');
    return session;
  }

  async function requireDecider(ctx) {
    const session = await requireUser(ctx);
    const { data: user } = await github.request('GET', '/user', { token: session.t });
    if (user?.id !== session.id) {
      dropSession(ctx);
      refuse(401, 'signed_out', 'Sign in again.');
    }
    if (!(await canDecide(user.login, { fresh: true }))) refuse(403, 'cannot_decide', 'Only the owner can decide.');
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

  function text(value, { limit = LIMITS.text, required = true, what = 'Text' } = {}) {
    if (value === undefined || value === null) value = '';
    if (typeof value !== 'string') refuse(400, 'bad_input', `${what} must be text.`);
    const trimmed = value.trim();
    if (required && !trimmed) refuse(400, 'empty', `${what} is empty.`);
    if ([...trimmed].length > limit) refuse(400, 'too_long', `${what} is longer than ${limit.toLocaleString('en-US')} characters.`);
    return trimmed;
  }

  // Routes

  const routes = {
    async state(ctx) {
      const { data, stale } = await loadState(ctx);
      return json(200, stale ? { ...data, stale: true } : data, { 'Cache-Control': stale ? PUBLIC_STALE : PUBLIC_STATE });
    },

    async me(ctx) {
      const session = await requireUser(ctx);
      return json(200, { login: session.login, avatar: session.avatar, canDecide: await canDecide(session.login) });
    },

    async issue(ctx) {
      const raw = ctx.url.searchParams.get('n') || '';
      if (!/^[1-9]\d{0,8}$/.test(raw)) refuse(400, 'bad_input', 'That issue number is not valid.');
      const n = Number(raw);
      const hasCookie = parseCookies(ctx.request.headers.cookie)[names.session] !== undefined;
      const session = hasCookie ? await currentUser(ctx) : null;
      if (session) {
        const data = await fetchIssue(n);
        const reactions = await upReactions(n);
        rememberIssue(n, data);
        return json(200, { ...data, viewerUp: reactions.some((reaction) => reaction.user?.id === session.id) }, { 'Cache-Control': PRIVATE, Vary: 'Cookie' });
      }
      const cached = issueCache.get(n);
      if (cached && now() - cached.at < ISSUE_TTL) return json(200, cached.data, { 'Cache-Control': PUBLIC_ISSUE, Vary: 'Cookie' });
      try {
        // Anonymous reads are limited to issues in the cached state, so arbitrary numbers cost no GitHub call.
        if (!((await loadState(ctx)).data.issues[n])) refuse(404, 'not_found', 'This is not a Workshop issue.');
        const data = await fetchIssue(n);
        rememberIssue(n, data);
        return json(200, data, { 'Cache-Control': hasCookie ? PRIVATE : PUBLIC_ISSUE, Vary: 'Cookie' });
      } catch (error) {
        if (error instanceof GitHubError && (error.rateLimited || error.network || error.status >= 500) && cached && now() - cached.at <= STATE_STALE_LIMIT) {
          ctx.githubStatus = error.status;
          return json(200, { ...cached.data, stale: true }, { 'Cache-Control': PUBLIC_STALE, Vary: 'Cookie' });
        }
        throw error;
      }
    },

    async login(ctx) {
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

    async callback(ctx) {
      const raw = parseCookies(ctx.request.headers.cookie)[names.state];
      ctx.cookies.push(clearCookie(names.state, { secure }));
      const saved = raw === undefined ? null : unseal(config.sessionSecret, 'signin', raw);
      const state = ctx.url.searchParams.get('state') || '';
      if (!saved || !(saved.exp > now()) || !sameText(saved.s, state)) refuse(400, 'bad_state', 'Sign-in expired. Try again.');
      const back = returnPath(saved.r);
      if (ctx.url.searchParams.get('error')) return redirect(back);
      const code = ctx.url.searchParams.get('code') || '';
      if (!code || code.length > 512) refuse(400, 'bad_input', 'Sign-in did not finish. Try again.');
      const data = await oauthToken({ code, redirect_uri: callbackUrl(), code_verifier: saved.v, repository_id: String(await app.repoId()) });
      if (!data) refuse(400, 'bad_code', 'Sign-in did not finish. Try again.');
      const fields = tokenFields(data);
      if (!fields) {
        log({ endpoint: 'callback', failure: 'token_does_not_expire' });
        refuse(502, 'github_failed', 'Sign-in is not set up correctly.');
      }
      const { data: user } = await github.request('GET', '/user', { token: fields.t });
      const session = { v: 1, id: user.id, login: user.login, avatar: user.avatar_url, ...fields, exp: now() + SESSION_MAX_AGE * 1000 };
      writeSession(ctx, session);
      return redirect(back);
    },

    async logout(ctx) {
      const session = readSession(ctx);
      ctx.cookies.push(clearCookie(names.session, { secure }));
      if (session) {
        const basic = Buffer.from(`${config.clientId}:${config.clientSecret}`).toString('base64');
        await github.request('DELETE', `/applications/${encodeURIComponent(config.clientId)}/token`, { token: basic, auth: 'Basic', body: { access_token: session.t } }).catch(() => {});
      }
      return json(200, { ok: true });
    },

    async comment(ctx) {
      const session = await requireUser(ctx);
      const input = readJson(ctx);
      const n = issueNumber(input.n);
      const body = text(input.text, { what: 'The comment' });
      await workshopIssue(n);
      const { data } = await github.request('POST', `/repos/${repo}/issues/${n}/comments`, { token: session.t, body: { body } });
      forget(n);
      return json(200, { ok: true, id: data?.id ?? null });
    },

    async vote(ctx) {
      const session = await requireUser(ctx);
      const input = readJson(ctx);
      const n = issueNumber(input.n);
      if (typeof input.on !== 'boolean') refuse(400, 'bad_input', 'The request was not understood.');
      await workshopIssue(n);
      if (input.on) {
        await github.request('POST', `/repos/${repo}/issues/${n}/reactions`, { token: session.t, body: { content: '+1' } });
      } else {
        const mine = (await upReactions(n)).filter((reaction) => reaction.user?.id === session.id);
        for (const reaction of mine) {
          await github.request('DELETE', `/repos/${repo}/issues/${n}/reactions/${reaction.id}`, { token: session.t });
        }
      }
      forget(n);
      return json(200, { ok: true });
    },

    async suggest(ctx) {
      const session = await requireUser(ctx);
      const input = readJson(ctx);
      const title = text(input.title, { limit: LIMITS.title, what: 'The title' });
      const body = text(input.text, { required: false, what: 'The description' });
      const { data } = await github.request('POST', `/repos/${repo}/issues`, { token: session.t, body: { title, body } });
      const number = data.number;
      const label = () => installation().then((token) => github.request('POST', `/repos/${repo}/issues/${number}/labels`, { token, body: { labels: [LABELS.suggestion] } }));
      try {
        await label();
      } catch {
        await label();
      }
      forget(number);
      return json(200, { number });
    },

    async verdict(ctx) {
      const input = readJson(ctx);
      const n = issueNumber(input.n);
      const verdict = input.verdict ?? null;
      if (verdict !== null && !VERDICTS.includes(verdict)) refuse(400, 'bad_input', 'That verdict is not known.');
      const note = text(input.note, { required: verdict === 'refine', what: 'The note' });
      const session = await requireDecider(ctx);
      const { issue, kind } = await workshopIssue(n);
      if (kind !== 'entry') refuse(404, 'not_found', 'This is not a Workshop entry.');
      if (note) await github.request('POST', `/repos/${repo}/issues/${n}/comments`, { token: session.t, body: { body: note } });
      const labels = labelNames(issue).filter((name) => !name.startsWith('verdict:'));
      if (verdict) labels.push(`verdict:${verdict}`);
      const patch = { labels };
      if (verdict === 'drop') Object.assign(patch, { state: 'closed', state_reason: 'not_planned' });
      else if (issue.state === 'closed' && issue.state_reason === 'not_planned') Object.assign(patch, { state: 'open', state_reason: 'reopened' });
      await github.request('PATCH', `/repos/${repo}/issues/${n}`, { token: session.t, body: patch });
      forget(n);
      return json(200, { ok: true });
    },

    async decide(ctx) {
      const input = readJson(ctx);
      const n = issueNumber(input.n);
      const body = text(input.text, { what: 'The decision' });
      const session = await requireDecider(ctx);
      const { issue, kind } = await workshopIssue(n);
      if (kind !== 'decision' || issue.state !== 'open') refuse(404, 'not_found', 'This is not an open Workshop decision.');
      await github.request('POST', `/repos/${repo}/issues/${n}/comments`, { token: session.t, body: { body } });
      await github.request('PATCH', `/repos/${repo}/issues/${n}`, { token: session.t, body: { state: 'closed', state_reason: 'completed' } });
      forget(n);
      return json(200, { ok: true });
    },
  };

  function callbackUrl() {
    return `${config.origin}/api/workshop/callback`;
  }

  return { handle };
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
