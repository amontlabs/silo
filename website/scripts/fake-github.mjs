// A fake GitHub for the Workshop's tests and for `workshop-dev.mjs --fake`:
// the App and installation tokens, the user authorization flow with expiring
// tokens and PKCE, and the issue, comment, reaction, label and permission
// endpoints the Workshop uses. State lives in memory.
import { createHash, generateKeyPairSync, randomBytes, verify } from 'node:crypto';
import { createServer } from 'node:http';

export const FAKE_APP = { appId: '1001', clientId: 'Iv1.fakeclient', installationId: 7, repoId: 4242 };

function json(res, status, body, headers = {}) {
  res.writeHead(status, { 'Content-Type': 'application/json; charset=utf-8', ...headers });
  res.end(body === undefined ? '' : JSON.stringify(body));
}

const escapeHtml = (value) => String(value).replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);

export function createFakeGitHub({ repo = 'amontlabs/silo', now = Date.now, clientSecret = randomBytes(16).toString('hex') } = {}) {
  const { privateKey, publicKey } = generateKeyPairSync('rsa', { modulusLength: 2048 });
  const state = {
    users: new Map(),
    issues: new Map(),
    tokens: new Map(),
    refresh: new Map(),
    codes: new Map(),
    nextIssue: 1,
    nextId: 1000,
    rateLimited: false,
    failNext: 0,
    perPage: null,
    requests: [],
  };
  let baseUrl = '';
  const pem = privateKey.export({ type: 'pkcs1', format: 'pem' });
  const issuedTokens = [];

  const iso = (ms = now()) => new Date(ms).toISOString();
  const token = (prefix) => {
    const value = `${prefix}_fake${randomBytes(18).toString('base64url')}`;
    issuedTokens.push(value);
    return value;
  };

  function addUser(login, { permission = 'read', type = 'User' } = {}) {
    const user = { id: state.nextId++, login, type, avatar_url: `https://avatars.example.test/${login}.png`, permission };
    state.users.set(login, user);
    return user;
  }

  function publicUser(user) {
    return { id: user.id, login: user.login, type: user.type, avatar_url: user.avatar_url };
  }

  const bot = { id: 900, login: 'silo-workshop[bot]', type: 'Bot', avatar_url: 'https://avatars.example.test/bot.png', permission: 'write' };

  function addIssue({ title, body = '', labels = [], author, state: issueState = 'open', reason = null, pull = false, createdAt }) {
    const number = state.nextIssue++;
    const user = author ? state.users.get(author) : bot;
    const issue = {
      number, title, body, labels: [...labels], user,
      state: issueState, state_reason: issueState === 'closed' ? reason || 'completed' : null,
      created_at: createdAt || iso(), closed_at: issueState === 'closed' ? iso() : null,
      comments: [], reactions: [], pull_request: pull ? {} : undefined,
    };
    state.issues.set(number, issue);
    return issue;
  }

  function addComment(number, author, body) {
    const issue = state.issues.get(number);
    const comment = { id: state.nextId++, user: author ? state.users.get(author) : bot, body, created_at: iso() };
    issue.comments.push(comment);
    return comment;
  }

  function react(number, login) {
    const issue = state.issues.get(number);
    const user = state.users.get(login);
    let reaction = issue.reactions.find((r) => r.user.id === user.id && r.content === '+1');
    if (!reaction) {
      reaction = { id: state.nextId++, content: '+1', user };
      issue.reactions.push(reaction);
    }
    return reaction;
  }

  function issueJson(issue) {
    return {
      number: issue.number,
      title: issue.title,
      body: issue.body,
      html_url: `https://github.com/${repo}/issues/${issue.number}`,
      state: issue.state,
      state_reason: issue.state_reason,
      labels: issue.labels.map((name) => ({ name })),
      user: publicUser(issue.user),
      created_at: issue.created_at,
      closed_at: issue.closed_at,
      comments: issue.comments.length,
      reactions: { '+1': issue.reactions.filter((r) => r.content === '+1').length },
      ...(issue.pull_request ? { pull_request: { url: 'x' } } : {}),
    };
  }

  function verifyJwt(jwt) {
    const [header, payload, signature] = (jwt || '').split('.');
    if (!signature) return false;
    const ok = verify('sha256', Buffer.from(`${header}.${payload}`), publicKey, Buffer.from(signature, 'base64url'));
    if (!ok) return false;
    const claims = JSON.parse(Buffer.from(payload, 'base64url').toString());
    const t = Math.floor(now() / 1000);
    return claims.iss === FAKE_APP.appId && claims.exp > t && claims.iat <= t + 60 && claims.exp - claims.iat <= 600;
  }

  function caller(req) {
    const header = req.headers.authorization || '';
    const [scheme, value] = header.split(' ');
    if (scheme === 'Bearer' && value?.split('.').length === 3) return verifyJwt(value) ? { kind: 'app' } : null;
    if (scheme === 'Basic') {
      const [id, secret] = Buffer.from(value || '', 'base64').toString().split(':');
      return id === FAKE_APP.clientId && secret === clientSecret ? { kind: 'client' } : null;
    }
    const entry = state.tokens.get(value);
    if (!entry || entry.expiresAt <= now()) return null;
    if (entry.kind === 'installation') return { kind: 'installation', user: bot };
    return { kind: 'user', user: state.users.get(entry.login), repositoryId: entry.repositoryId };
  }

  const canWrite = (who) => who.kind === 'installation' || ['write', 'maintain', 'admin'].includes(who.user?.permission);

  function userTokens(login, repositoryId) {
    const access = token('ghu');
    const refreshToken = token('ghr');
    state.tokens.set(access, { kind: 'user', login, expiresAt: now() + 8 * 3600_000, repositoryId });
    state.refresh.set(refreshToken, { login, expiresAt: now() + 15897600_000, repositoryId });
    return { access_token: access, expires_in: 28800, refresh_token: refreshToken, refresh_token_expires_in: 15897600, token_type: 'bearer', scope: '' };
  }

  async function readBody(req) {
    const chunks = [];
    for await (const chunk of req) chunks.push(chunk);
    const text = Buffer.concat(chunks).toString();
    if (!text) return {};
    if ((req.headers['content-type'] || '').includes('application/x-www-form-urlencoded')) return Object.fromEntries(new URLSearchParams(text));
    return JSON.parse(text);
  }

  async function handle(req, res) {
    const url = new URL(req.url, 'http://fake');
    const path = url.pathname;
    const who = caller(req);
    state.requests.push({ method: req.method, path, kind: who?.kind ?? 'anonymous', login: who?.user?.login ?? null });

    // User authorization (github.com)
    if (path === '/login/oauth/authorize') {
      const q = url.searchParams;
      if (q.get('client_id') !== FAKE_APP.clientId) return json(res, 400, { error: 'bad client' });
      const buttons = [...state.users.values()].filter((u) => u.type === 'User').map((u) => {
        const approve = new URL('/login/oauth/fake-approve', 'http://fake');
        for (const key of ['redirect_uri', 'state', 'code_challenge']) approve.searchParams.set(key, q.get(key) || '');
        approve.searchParams.set('login', u.login);
        return `<p><a href="${escapeHtml(approve.pathname + approve.search)}">Sign in as ${escapeHtml(u.login)} (${escapeHtml(u.permission)})</a></p>`;
      }).join('');
      res.writeHead(200, { 'Content-Type': 'text/html; charset=utf-8' });
      return res.end(`<!doctype html><title>Fake GitHub sign-in</title><h1>Fake GitHub</h1>${buttons}<p><a href="${escapeHtml((q.get('redirect_uri') || '') + '?error=access_denied&state=' + encodeURIComponent(q.get('state') || ''))}">Cancel</a></p>`);
    }
    if (path === '/login/oauth/fake-approve') {
      const q = url.searchParams;
      const code = randomBytes(10).toString('hex');
      state.codes.set(code, { login: q.get('login'), redirectUri: q.get('redirect_uri'), challenge: q.get('code_challenge'), expiresAt: now() + 600_000 });
      const target = new URL(q.get('redirect_uri'));
      target.searchParams.set('code', code);
      target.searchParams.set('state', q.get('state'));
      res.writeHead(302, { Location: target.toString() });
      return res.end();
    }
    if (path === '/login/oauth/access_token' && req.method === 'POST') {
      const body = await readBody(req);
      if (body.client_id !== FAKE_APP.clientId || body.client_secret !== clientSecret) return json(res, 200, { error: 'incorrect_client_credentials' });
      if (body.grant_type === 'refresh_token') {
        const entry = state.refresh.get(body.refresh_token);
        if (!entry || entry.expiresAt <= now()) return json(res, 200, { error: 'bad_refresh_token' });
        state.refresh.delete(body.refresh_token);
        return json(res, 200, userTokens(entry.login, entry.repositoryId));
      }
      const entry = state.codes.get(body.code);
      state.codes.delete(body.code);
      if (!entry || entry.expiresAt <= now() || entry.redirectUri !== body.redirect_uri) return json(res, 200, { error: 'bad_verification_code' });
      if (createHash('sha256').update(body.code_verifier || '').digest('base64url') !== entry.challenge) return json(res, 200, { error: 'bad_verification_code' });
      return json(res, 200, userTokens(entry.login, body.repository_id ? Number(body.repository_id) : undefined));
    }

    if (state.rateLimited) {
      return json(res, 403, { message: 'API rate limit exceeded' }, { 'x-ratelimit-remaining': '0', 'x-ratelimit-reset': String(Math.floor(now() / 1000) + 120) });
    }
    if (state.failNext > 0) {
      state.failNext -= 1;
      return json(res, 500, { message: 'fake failure' });
    }
    if (!who) return json(res, 401, { message: 'Bad credentials' });

    const appToken = path.match(/^\/applications\/([^/]+)\/token$/);
    if (appToken && req.method === 'DELETE') {
      if (who.kind !== 'client') return json(res, 401, { message: 'Bad credentials' });
      const body = await readBody(req);
      state.tokens.delete(body.access_token);
      return json(res, 204);
    }
    if (path === `/repos/${repo}/installation` && who.kind === 'app') return json(res, 200, { id: FAKE_APP.installationId });
    if (path === `/app/installations/${FAKE_APP.installationId}/access_tokens` && req.method === 'POST' && who.kind === 'app') {
      const value = token('ghs');
      const expiresAt = now() + 3600_000;
      state.tokens.set(value, { kind: 'installation', expiresAt });
      return json(res, 201, { token: value, expires_at: iso(expiresAt) });
    }
    if (who.kind === 'app' || who.kind === 'client') return json(res, 403, { message: 'Forbidden' });
    if (who.kind === 'user' && who.repositoryId && who.repositoryId !== FAKE_APP.repoId) return json(res, 403, { message: 'Forbidden' });
    if (path === '/user' && who.kind === 'user') return json(res, 200, publicUser(who.user));
    if (path === `/repos/${repo}`) return json(res, 200, { id: FAKE_APP.repoId, full_name: repo });

    const permission = path.match(new RegExp(`^/repos/${repo}/collaborators/([^/]+)/permission$`));
    if (permission) {
      if (who.kind !== 'installation') return json(res, 403, { message: 'Forbidden' });
      const user = state.users.get(decodeURIComponent(permission[1]));
      if (!user) return json(res, 404, { message: 'Not Found' });
      const role = user.permission;
      return json(res, 200, { permission: role === 'maintain' ? 'write' : role, role_name: role, user: publicUser(user) });
    }

    if (path === `/repos/${repo}/issues` && req.method === 'GET') {
      const labels = (url.searchParams.get('labels') || '').split(',').filter(Boolean);
      const wanted = url.searchParams.get('state') || 'open';
      const all = [...state.issues.values()]
        .filter((i) => labels.every((l) => i.labels.includes(l)))
        .filter((i) => wanted === 'all' || i.state === wanted)
        .sort((a, b) => b.number - a.number);
      const perPage = state.perPage || Number(url.searchParams.get('per_page') || 30);
      const page = Number(url.searchParams.get('page') || 1);
      const slice = all.slice((page - 1) * perPage, page * perPage);
      const headers = {};
      if (page * perPage < all.length) {
        const next = new URL(baseUrl + url.pathname + url.search);
        next.searchParams.set('page', String(page + 1));
        headers.Link = `<${next}>; rel="next"`;
      }
      return json(res, 200, slice.map(issueJson), headers);
    }
    if (path === `/repos/${repo}/issues` && req.method === 'POST') {
      const body = await readBody(req);
      if (who.kind !== 'user') return json(res, 403, { message: 'Forbidden' });
      const issue = addIssue({ title: body.title, body: body.body || '', labels: canWrite(who) ? body.labels || [] : [], author: who.user.login });
      return json(res, 201, issueJson(issue));
    }

    const issueMatch = path.match(new RegExp(`^/repos/${repo}/issues/(\\d+)(/.*)?$`));
    if (!issueMatch) return json(res, 404, { message: 'Not Found' });
    const issue = state.issues.get(Number(issueMatch[1]));
    if (!issue) return json(res, 404, { message: 'Not Found' });
    const rest = issueMatch[2] || '';

    if (!rest && req.method === 'GET') return json(res, 200, issueJson(issue));
    if (!rest && req.method === 'PATCH') {
      if (!canWrite(who)) return json(res, 403, { message: 'Forbidden' });
      const body = await readBody(req);
      if (body.labels) issue.labels = [...new Set(body.labels)];
      if (body.state === 'closed') {
        if (issue.state !== 'closed') issue.closed_at = iso();
        issue.state = 'closed';
        issue.state_reason = body.state_reason || 'completed';
      }
      if (body.state === 'open' && issue.state !== 'open') {
        issue.state = 'open';
        issue.closed_at = null;
        issue.state_reason = 'reopened';
      }
      return json(res, 200, issueJson(issue));
    }
    if (rest === '/labels' && req.method === 'POST') {
      if (!canWrite(who)) return json(res, 403, { message: 'Forbidden' });
      const body = await readBody(req);
      issue.labels = [...new Set([...issue.labels, ...body.labels])];
      return json(res, 200, issue.labels.map((name) => ({ name })));
    }
    if (rest === '/comments' && req.method === 'GET') {
      const perPage = state.perPage || Number(url.searchParams.get('per_page') || 30);
      const page = Number(url.searchParams.get('page') || 1);
      const headers = {};
      if (page * perPage < issue.comments.length) {
        const next = new URL(baseUrl + url.pathname + url.search);
        next.searchParams.set('page', String(page + 1));
        headers.Link = `<${next}>; rel="next"`;
      }
      return json(res, 200, issue.comments.slice((page - 1) * perPage, page * perPage).map((c) => ({ id: c.id, body: c.body, created_at: c.created_at, user: publicUser(c.user) })), headers);
    }
    if (rest === '/comments' && req.method === 'POST') {
      const body = await readBody(req);
      const comment = { id: state.nextId++, user: who.user, body: body.body, created_at: iso() };
      issue.comments.push(comment);
      return json(res, 201, { id: comment.id, body: comment.body, user: publicUser(comment.user) });
    }
    if (rest === '/reactions' && req.method === 'GET') {
      const content = url.searchParams.get('content');
      return json(res, 200, issue.reactions.filter((r) => !content || r.content === content).map((r) => ({ id: r.id, content: r.content, user: publicUser(r.user) })));
    }
    if (rest === '/reactions' && req.method === 'POST') {
      if (who.kind !== 'user') return json(res, 403, { message: 'Forbidden' });
      const body = await readBody(req);
      const existing = issue.reactions.find((r) => r.user.id === who.user.id && r.content === body.content);
      if (existing) return json(res, 200, { id: existing.id, content: existing.content });
      const reaction = { id: state.nextId++, content: body.content, user: who.user };
      issue.reactions.push(reaction);
      return json(res, 201, { id: reaction.id, content: reaction.content });
    }
    const reactionDelete = rest.match(/^\/reactions\/(\d+)$/);
    if (reactionDelete && req.method === 'DELETE') {
      const index = issue.reactions.findIndex((r) => r.id === Number(reactionDelete[1]));
      if (index < 0) return json(res, 404, { message: 'Not Found' });
      if (who.kind !== 'user' || issue.reactions[index].user.id !== who.user.id) return json(res, 403, { message: 'Forbidden' });
      issue.reactions.splice(index, 1);
      return json(res, 204);
    }
    return json(res, 404, { message: 'Not Found' });
  }

  const server = createServer((req, res) => {
    handle(req, res).catch((error) => { if (process.env.FAKE_GITHUB_DEBUG) console.error(error); json(res, 500, { message: 'fake error' }); });
  });

  return {
    state,
    clientSecret,
    privateKeyPem: pem,
    issuedTokens,
    addUser,
    addIssue,
    addComment,
    react,
    get url() { return baseUrl; },
    async listen(port = 0, host = '127.0.0.1') {
      await new Promise((resolve) => server.listen(port, host, resolve));
      baseUrl = `http://${host}:${server.address().port}`;
      return baseUrl;
    },
    async close() {
      server.closeAllConnections?.();
      await new Promise((resolve) => server.close(resolve));
    },
    // Settings for createWorkshop that point at this fake.
    workshopEnv(origin) {
      return {
        WORKSHOP_APP_ID: FAKE_APP.appId,
        WORKSHOP_CLIENT_ID: FAKE_APP.clientId,
        WORKSHOP_CLIENT_SECRET: clientSecret,
        WORKSHOP_PRIVATE_KEY: pem,
        WORKSHOP_SESSION_SECRET: randomBytes(32).toString('base64'),
        WORKSHOP_REPO: repo,
        WORKSHOP_SITE_ORIGIN: origin,
      };
    },
  };
}

// A small Workshop: entries from features.jsonl (when given), two decisions, a
// suggestion, comments and votes, an unrelated issue and a pull request.
export function seedDemo(fake, features = []) {
  fake.addUser('owner-fake', { permission: 'admin' });
  fake.addUser('visitor-fake', { permission: 'read' });
  const list = features.length ? features : [
    { id: 'engine-service', name: 'The engine runs on its own', what: 'A background engine owns every computer.' },
    { id: 'calls-and-events', name: 'Every call and event defined once', what: 'Each thing the engine can be asked is written down once.' },
    { id: 'vm-runtime', name: 'The VM runtime, packaged', what: 'Silo ships the program that runs each computer.' },
  ];
  for (const [index, feature] of list.entries()) {
    const labels = ['workshop:entry'];
    if (index === 0) labels.push('verdict:take');
    if (index === 2) labels.push('verdict:later');
    fake.addIssue({ title: feature.name, body: `${feature.what}\n<!-- silo-entry: ${feature.id} -->`, labels });
  }
  fake.addComment(1, 'visitor-fake', 'Looking forward to this.\nSee https://example.com for `context`.');
  fake.addComment(1, null, 'Posted by an agent.');
  fake.react(1, 'visitor-fake');
  fake.react(2, 'owner-fake');
  const open = fake.addIssue({ title: 'Which Linux distribution do computers start with?', body: 'Ubuntu or Debian.', labels: ['workshop:decision'] });
  fake.addComment(open.number, 'visitor-fake', 'Debian, please.');
  const decided = fake.addIssue({ title: 'Is the engine written in Zig?', body: 'Zig or Rust.', labels: ['workshop:decision'], state: 'closed', reason: 'completed' });
  fake.addComment(decided.number, 'owner-fake', 'Zig.');
  const suggestion = fake.addIssue({ title: 'Share a computer with a link', body: 'A read-only link to watch an agent work.', labels: ['workshop:suggestion'], author: 'visitor-fake' });
  fake.react(suggestion.number, 'visitor-fake');
  fake.addIssue({ title: 'An ordinary bug report', body: 'Not part of the Workshop.', author: 'visitor-fake' });
  fake.addIssue({ title: 'A pull request with a Workshop label', labels: ['workshop:entry'], pull: true });
}
