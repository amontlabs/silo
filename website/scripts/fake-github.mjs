// A fake GitHub for the Workshop's tests and for `workshop-dev.mjs --fake`:
// the App and installation tokens, the user authorization flow with expiring
// tokens and PKCE, the issue, comment, reaction and label endpoints the
// Workshop uses, and the one GraphQL query it sends. State lives in memory.
import { createHash, generateKeyPairSync, randomBytes, verify } from 'node:crypto';
import { createServer } from 'node:http';

export const FAKE_APP = { appId: '1001', slug: 'silo-workshop', clientId: 'Iv1.fakeclient', installationId: 7, repoId: 4242 };

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
    // Labels the repository lacks: GitHub drops them from a new issue or an added label, until one is created.
    missingLabels: new Set(),
    secondaryLimited: false,
    failNext: 0,
    hook: null,
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

  // `at` (milliseconds) dates the comment; without it, now.
  function addComment(number, author, body, at) {
    const issue = state.issues.get(number);
    const comment = { id: state.nextId++, user: author ? state.users.get(author) : bot, body, created_at: iso(at), updated_at: iso(at), reactions: [] };
    issue.comments.push(comment);
    return comment;
  }

  function editComment(number, id, body) {
    const comment = state.issues.get(number).comments.find((c) => c.id === id);
    Object.assign(comment, { body, updated_at: iso() });
  }

  function deleteComment(number, id) {
    const issue = state.issues.get(number);
    issue.comments = issue.comments.filter((c) => c.id !== id);
  }

  function commentJson(number, comment) {
    return {
      id: comment.id, body: comment.body, created_at: comment.created_at, updated_at: comment.updated_at,
      user: publicUser(comment.user), issue_url: `${baseUrl}/repos/${repo}/issues/${number}`,
      performed_via_github_app: comment.user === bot ? { id: Number(FAKE_APP.appId), slug: FAKE_APP.slug } : null,
      reactions: { '+1': comment.reactions.filter((r) => r.content === '+1').length },
    };
  }

  // The issue and the comment of a comment id, or null.
  function findComment(id) {
    for (const issue of state.issues.values()) {
      const comment = issue.comments.find((c) => c.id === id);
      if (comment) return { issue, comment };
    }
    return null;
  }

  // A 👍 by `login` on comment `id`, as a person gives one.
  function reactToComment(id, login) {
    const { comment } = findComment(id);
    const user = state.users.get(login);
    if (!comment.reactions.some((r) => r.user.id === user.id && r.content === '+1')) comment.reactions.push({ id: state.nextId++, content: '+1', user });
  }

  // One page of a listing, with Link and ETag headers; a matching If-None-Match answers 304.
  function listPage(req, res, url, items, record, perPageOverride) {
    const perPage = perPageOverride || Number(url.searchParams.get('per_page') || 30);
    const page = Number(url.searchParams.get('page') || 1);
    const slice = items.slice((page - 1) * perPage, page * perPage);
    const headers = {};
    if (page * perPage < items.length) {
      const next = new URL(baseUrl + url.pathname + url.search);
      next.searchParams.set('page', String(page + 1));
      headers.Link = `<${next}>; rel="next"`;
    }
    const body = JSON.stringify(slice);
    headers.ETag = `W/"${createHash('sha256').update(body + (headers.Link || '')).digest('hex').slice(0, 32)}"`;
    if (req.headers['if-none-match'] === headers.ETag) {
      record.status = 304;
      res.writeHead(304, { ETag: headers.ETag });
      return res.end();
    }
    record.status = 200;
    res.writeHead(200, { 'Content-Type': 'application/json; charset=utf-8', ...headers });
    return res.end(body);
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
      locked: Boolean(issue.locked),
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
    const record = { method: req.method, path, query: url.search, wall: Date.now(), kind: who?.kind ?? 'anonymous', login: who?.user?.login ?? null };
    state.requests.push(record);
    // When the answer was sent, to see which calls were in flight together.
    res.on('finish', () => { record.done = Date.now(); });
    // A test's hook may delay or replace any answer.
    const hooked = state.hook?.(record);
    if (hooked?.delay) await new Promise((resolve) => setTimeout(resolve, hooked.delay));
    if (hooked?.status) return json(res, hooked.status, hooked.body ?? { message: 'hooked' }, hooked.headers);

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
    if (state.secondaryLimited) {
      const headers = state.secondaryLimited === 'retry-after' ? { 'retry-after': '30' } : {};
      return json(res, 403, { message: 'You have exceeded a secondary rate limit. Please wait a few minutes before you try again.' }, headers);
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
    const appGrant = path.match(/^\/applications\/([^/]+)\/grant$/);
    if (appGrant && req.method === 'DELETE') {
      if (who.kind !== 'client') return json(res, 401, { message: 'Bad credentials' });
      const body = await readBody(req);
      const entry = state.tokens.get(body.access_token);
      if (!entry || entry.kind !== 'user' || entry.expiresAt <= now()) return json(res, 404, { message: 'Not Found' });
      for (const [value, other] of state.tokens) if (other.login === entry.login) state.tokens.delete(value);
      for (const [value, other] of state.refresh) if (other.login === entry.login) state.refresh.delete(value);
      return json(res, 204);
    }
    if (path === `/repos/${repo}/installation` && who.kind === 'app') return json(res, 200, { id: FAKE_APP.installationId, app_slug: FAKE_APP.slug });
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

    if (path === `/repos/${repo}/issues` && req.method === 'GET') {
      const labels = (url.searchParams.get('labels') || '').split(',').filter(Boolean);
      const wanted = url.searchParams.get('state') || 'open';
      const ascending = url.searchParams.get('direction') === 'asc';
      const all = [...state.issues.values()]
        .filter((i) => labels.every((l) => i.labels.includes(l)))
        .filter((i) => wanted === 'all' || i.state === wanted)
        .sort((a, b) => (ascending ? a.number - b.number : b.number - a.number));
      return listPage(req, res, url, all.map(issueJson), record, state.perPage);
    }
    if (path === `/repos/${repo}/issues/comments` && req.method === 'GET') {
      const sort = url.searchParams.get('sort');
      const since = url.searchParams.get('since');
      const key = sort === 'updated' ? 'updated_at' : 'created_at';
      const descending = sort && url.searchParams.get('direction') === 'desc';
      const all = [...state.issues.values()].flatMap((i) => i.comments.map((c) => commentJson(i.number, c)))
        .filter((c) => !since || c.updated_at > since)
        .sort((a, b) => (sort ? (a[key] < b[key] ? -1 : a[key] > b[key] ? 1 : a.id - b.id) : a.id - b.id) * (descending ? -1 : 1));
      return listPage(req, res, url, all, record, state.commentsPerPage);
    }
    if (path === `/repos/${repo}/issues` && req.method === 'POST') {
      const body = await readBody(req);
      if (who.kind !== 'user' && who.kind !== 'installation') return json(res, 403, { message: 'Forbidden' });
      // Labels are silently dropped for anyone without push access, as GitHub documents, and so is a missing label.
      const labels = canWrite(who) ? (body.labels || []).filter((name) => !state.missingLabels.has(name)) : [];
      const issue = addIssue({ title: body.title, body: body.body || '', labels, author: who.kind === 'user' ? who.user.login : null });
      return json(res, 201, issueJson(issue));
    }
    // The one query the Workshop sends: the person's 👍 on the latest 100 comments of an issue.
    if (path === '/graphql' && req.method === 'POST') {
      const body = await readBody(req);
      if (who.kind !== 'user') return json(res, 403, { message: 'Forbidden' });
      const { owner, name, number } = body.variables || {};
      if (!/comments\(last: 100\)/.test(body.query || '') || `${owner}/${name}` !== repo) return json(res, 200, { errors: [{ message: 'unknown query' }] });
      const issue = state.issues.get(number);
      if (!issue || issue.pull_request) return json(res, 200, { data: { repository: { issue: null } }, errors: [{ type: 'NOT_FOUND', message: 'Could not resolve to an Issue.' }] });
      const nodes = issue.comments.slice(-100).map((c) => ({
        fullDatabaseId: String(c.id),
        reactionGroups: [{ content: 'THUMBS_UP', viewerHasReacted: c.reactions.some((r) => r.content === '+1' && r.user.id === who.user.id) }],
      }));
      return json(res, 200, { data: { repository: { issue: { comments: { nodes } } } } });
    }
    if (path === `/repos/${repo}/labels` && req.method === 'POST') {
      const body = await readBody(req);
      if (!canWrite(who)) return json(res, 403, { message: 'Forbidden' });
      if (!state.missingLabels.has(body.name)) return json(res, 422, { message: 'Validation Failed', errors: [{ code: 'already_exists' }] });
      state.missingLabels.delete(body.name);
      return json(res, 201, { name: body.name, color: body.color, description: body.description });
    }
    const commentMatch = path.match(new RegExp(`^/repos/${repo}/issues/comments/(\\d+)(/reactions(?:/(\\d+))?)?$`));
    if (commentMatch) {
      const found = findComment(Number(commentMatch[1]));
      if (!found) return json(res, 404, { message: 'Not Found' });
      const { issue: parent, comment } = found;
      if (!commentMatch[2] && req.method === 'GET') return json(res, 200, commentJson(parent.number, comment));
      if (commentMatch[2] && !commentMatch[3] && req.method === 'GET') {
        const content = url.searchParams.get('content');
        return listPage(req, res, url, comment.reactions.filter((r) => !content || r.content === content).map((r) => ({ id: r.id, content: r.content, user: publicUser(r.user) })), record);
      }
      if (commentMatch[2] && !commentMatch[3] && req.method === 'POST') {
        if (who.kind !== 'user' || (parent.locked && !canWrite(who))) return json(res, 403, { message: 'Forbidden' });
        const body = await readBody(req);
        const existing = comment.reactions.find((r) => r.user.id === who.user.id && r.content === body.content);
        if (existing) return json(res, 200, { id: existing.id, content: existing.content });
        const reaction = { id: state.nextId++, content: body.content, user: who.user };
        comment.reactions.push(reaction);
        return json(res, 201, { id: reaction.id, content: reaction.content });
      }
      if (commentMatch[3] && req.method === 'DELETE') {
        const index = comment.reactions.findIndex((r) => r.id === Number(commentMatch[3]));
        if (index < 0) return json(res, 404, { message: 'Not Found' });
        if (who.kind !== 'user' || comment.reactions[index].user.id !== who.user.id) return json(res, 403, { message: 'Forbidden' });
        comment.reactions.splice(index, 1);
        return json(res, 204);
      }
      return json(res, 404, { message: 'Not Found' });
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
    const labelDelete = rest.match(/^\/labels\/([^/]+)$/);
    if (labelDelete && req.method === 'DELETE') {
      if (!canWrite(who)) return json(res, 403, { message: 'Forbidden' });
      const name = decodeURIComponent(labelDelete[1]);
      if (!issue.labels.includes(name)) return json(res, 404, { message: 'Label does not exist' });
      issue.labels = issue.labels.filter((l) => l !== name);
      return json(res, 200, issue.labels.map((l) => ({ name: l })));
    }
    if (rest === '/labels' && req.method === 'POST') {
      if (!canWrite(who)) return json(res, 403, { message: 'Forbidden' });
      const body = await readBody(req);
      issue.labels = [...new Set([...issue.labels, ...body.labels.filter((name) => !state.missingLabels.has(name))])];
      return json(res, 200, issue.labels.map((name) => ({ name })));
    }
    if (rest === '/comments' && req.method === 'GET') {
      const since = url.searchParams.get('since');
      const items = issue.comments.map((c) => commentJson(issue.number, c)).filter((c) => !since || c.updated_at > since);
      return listPage(req, res, url, items, record);
    }
    if (rest === '/comments' && req.method === 'POST') {
      if (issue.locked && !canWrite(who)) return json(res, 403, { message: 'Issue is locked' });
      const body = await readBody(req);
      const comment = { id: state.nextId++, user: who.user, body: body.body, created_at: iso(), updated_at: iso(), reactions: [] };
      issue.comments.push(comment);
      return json(res, 201, commentJson(issue.number, comment));
    }
    if (rest === '/reactions' && req.method === 'GET') {
      const content = url.searchParams.get('content');
      return json(res, 200, issue.reactions.filter((r) => !content || r.content === content).map((r) => ({ id: r.id, content: r.content, user: publicUser(r.user) })));
    }
    if (rest === '/reactions' && req.method === 'POST') {
      if (who.kind !== 'user' || (issue.locked && !canWrite(who))) return json(res, 403, { message: 'Forbidden' });
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
    editComment,
    deleteComment,
    react,
    reactToComment,
    repo,
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

// A small Workshop: entries from the plan (when given, in build order), two issues with the
// retired label `workshop:decision` (no longer part of the Workshop), a suggestion, comments and
// votes, an unrelated issue and a pull request.
export function seedDemo(fake, features = []) {
  fake.addUser('owner-fake', { permission: 'admin' });
  fake.addUser('visitor-fake', { permission: 'read' });
  const list = features.length ? features : [
    { id: 'engine-service', name: 'The engine runs on its own', what: 'A background engine owns every computer.' },
    { id: 'calls-and-events', name: 'Every call and event defined once', what: 'Each thing the engine can be asked is written down once.' },
    { id: 'vm-runtime', name: 'The VM runtime, packaged', what: 'Silo ships the program that runs each computer.' },
  ];
  const numbers = new Map();
  for (const feature of list) {
    const labels = ['workshop:entry'];
    const closed = feature.built ? 'completed' : null;
    const issue = fake.addIssue({ title: feature.name, body: `${feature.what}\n<!-- silo-entry: ${feature.id} -->`, labels, ...(closed ? { state: 'closed', reason: closed } : {}) });
    numbers.set(feature.id, issue.number);
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
