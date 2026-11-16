// An in-memory GitHub for the tests: the REST endpoints the tool uses, the
// web page the manifest form posts to, App JWT verification with the App's
// public key, installation tokens, pagination and a switchable rate limit.
import { createPublicKey, generateKeyPairSync, randomBytes, verify } from 'node:crypto';
import { createServer } from 'node:http';

let keys;
export function appKeys() {
  keys ??= generateKeyPairSync('rsa', {
    modulusLength: 2048,
    publicKeyEncoding: { type: 'spki', format: 'pem' },
    privateKeyEncoding: { type: 'pkcs8', format: 'pem' },
  });
  return keys;
}

export function verifyJwt(jwt, publicKeyPem) {
  const [header, payload, signature] = jwt.split('.');
  const ok = verify('RSA-SHA256', Buffer.from(`${header}.${payload}`), createPublicKey(publicKeyPem), Buffer.from(signature, 'base64url'));
  return ok ? { header: JSON.parse(Buffer.from(header, 'base64url')), claims: JSON.parse(Buffer.from(payload, 'base64url')) } : null;
}

const json = (res, status, body, headers = {}) => {
  res.writeHead(status, { 'content-type': 'application/json', ...headers });
  res.end(body === undefined ? '' : JSON.stringify(body));
};

export async function startFake({ appId = '4242', slug = 'silo-workshop', repo = 'amontlabs/silo', publicKey = appKeys().publicKey } = {}) {
  const fake = {
    appId,
    slug,
    repo,
    publicKey,
    pageSize: 100,
    installed: true,
    public: true,
    installationPermissions: { issues: 'write', metadata: 'read' },
    labels: [],
    issues: new Map(),
    comments: [],
    events: [],
    permissions: { owner: 'admin', helper: 'triage', maintainer: 'write' },
    roleNames: { maintainer: 'maintain' },
    tokens: new Set(),
    jwts: [],
    secrets: [],
    log: [],
    writes: 0,
    limitAfterWrites: null,
    manifests: [],
    conversions: new Map(),
    nextNumber: 1,
    nextId: 1000,
    clock: Date.parse('2026-11-16T12:00:00Z'),
  };
  const bot = { login: `${slug}[bot]`, type: 'Bot' };
  const tick = () => new Date((fake.clock += 1000)).toISOString();
  const issuePath = `/repos/${repo}`;

  fake.issueJson = (issue) => ({
    number: issue.number,
    title: issue.title,
    body: issue.body,
    state: issue.state,
    state_reason: issue.state_reason,
    labels: issue.labels.map((name) => ({ name })),
    user: issue.user,
    created_at: issue.created_at,
    closed_at: issue.closed_at,
    html_url: `${fake.url}/${repo}/issues/${issue.number}`,
    reactions: { '+1': issue.up ?? 0 },
    ...(issue.pull_request ? { pull_request: {} } : {}),
  });
  fake.addIssue = ({ title, body = '', labels = [], user = { login: 'owner', type: 'User' }, state = 'open', state_reason = null, up = 0, pull_request = false }) => {
    const issue = { number: fake.nextNumber++, title, body, labels: [...labels], user, state, state_reason, up, pull_request, created_at: tick(), closed_at: state === 'closed' ? tick() : null };
    fake.issues.set(issue.number, issue);
    for (const name of labels) fake.events.push({ issue: issue.number, event: 'labeled', label: { name }, actor: user, created_at: issue.created_at });
    return issue;
  };
  fake.addComment = (number, login, body, { type = 'User', at } = {}) => {
    const created_at = at ?? tick();
    const comment = { id: fake.nextId++, issue: number, user: { login, type }, body, created_at, updated_at: created_at };
    fake.comments.push(comment);
    return comment;
  };
  fake.setVerdictLabel = (number, verdict, login = 'owner') => {
    const issue = fake.issues.get(number);
    const actor = { login, type: login.endsWith('[bot]') ? 'Bot' : 'User' };
    for (const name of issue.labels.filter((n) => n.startsWith('verdict:') && n !== `verdict:${verdict}`)) {
      fake.events.push({ issue: number, event: 'unlabeled', label: { name }, actor, created_at: tick() });
    }
    issue.labels = issue.labels.filter((name) => !name.startsWith('verdict:'));
    if (verdict) {
      issue.labels.push(`verdict:${verdict}`);
      fake.events.push({ issue: number, event: 'labeled', label: { name: `verdict:${verdict}` }, actor: { login, type: login.endsWith('[bot]') ? 'Bot' : 'User' }, created_at: tick() });
    }
  };
  fake.close = (number, reason, login = 'owner') => {
    const issue = fake.issues.get(number);
    issue.state = 'closed';
    issue.state_reason = reason;
    issue.closed_at = tick();
    fake.events.push({ issue: number, event: 'closed', actor: { login, type: 'User' }, created_at: issue.closed_at });
  };
  fake.reopen = (number, login = 'owner') => {
    const issue = fake.issues.get(number);
    Object.assign(issue, { state: 'open', state_reason: 'reopened', closed_at: null });
    fake.events.push({ issue: number, event: 'reopened', actor: { login, type: 'User' }, created_at: tick() });
  };
  fake.snapshot = () => [...fake.issues.values()].map((i) => ({ number: i.number, labels: [...i.labels].sort(), state: i.state, state_reason: i.state_reason }));

  const page = (res, items, url) => {
    const per = Math.min(Number(url.searchParams.get('per_page') ?? 30), fake.pageSize);
    const n = Number(url.searchParams.get('page') ?? 1);
    const slice = items.slice((n - 1) * per, n * per);
    const headers = {};
    if (n * per < items.length) {
      const next = new URL(url.href);
      next.searchParams.set('page', String(n + 1));
      headers.link = `<${fake.url}${next.pathname}${next.search}>; rel="next"`;
    }
    json(res, 200, slice, headers);
  };

  const readBody = (req) =>
    new Promise((resolve) => {
      let data = '';
      req.on('data', (chunk) => (data += chunk));
      req.on('end', () => resolve(data));
    });

  const server = createServer(async (req, res) => {
    const url = new URL(req.url, 'http://fake');
    const raw = await readBody(req);
    const auth = req.headers.authorization ?? '';
    let body = null;
    if (raw && (req.headers['content-type'] ?? '').includes('json')) body = JSON.parse(raw);
    fake.log.push({ method: req.method, path: url.pathname + url.search, body, auth: auth.split(' ')[0] || null });
    const p = url.pathname;

    // The web form of the manifest flow.
    const form = p.match(/^\/organizations\/([^/]+)\/settings\/apps\/new$/);
    if (form && req.method === 'POST') {
      const manifest = JSON.parse(new URLSearchParams(raw).get('manifest'));
      fake.manifests.push({ org: form[1], state: url.searchParams.get('state'), manifest, contentType: req.headers['content-type'] });
      const code = randomBytes(10).toString('hex');
      fake.conversions.set(code, manifest);
      const back = new URL(manifest.redirect_url);
      back.searchParams.set('code', code);
      back.searchParams.set('state', url.searchParams.get('state'));
      res.writeHead(302, { location: back.href });
      return res.end();
    }
    const conversion = p.match(/^\/app-manifests\/([^/]+)\/conversions$/);
    if (conversion && req.method === 'POST') {
      const manifest = fake.conversions.get(conversion[1]);
      if (!manifest) return json(res, 404, { message: 'Not Found' });
      fake.conversions.delete(conversion[1]);
      const secret = `secret-${randomBytes(16).toString('hex')}`;
      fake.secrets.push(secret);
      fake.createdApp = { id: Number(appId), slug, client_id: 'Iv23liFAKECLIENT', client_secret: secret, pem: keys.privateKey, manifest };
      return json(res, 201, {
        id: Number(appId),
        slug,
        name: manifest.name,
        client_id: 'Iv23liFAKECLIENT',
        client_secret: secret,
        webhook_secret: null,
        pem: keys.privateKey,
        html_url: `${fake.url}/apps/${slug}`,
        owner: { login: 'amontlabs' },
        permissions: manifest.default_permissions,
      });
    }

    // Reading an App without authentication: a public App answers, a private one is not found.
    const appBySlug = p.match(/^\/apps\/([^/]+)$/);
    if (appBySlug && req.method === 'GET' && !auth) {
      return fake.public && appBySlug[1] === slug ? json(res, 200, { id: Number(appId), slug, name: 'Silo Workshop' }) : json(res, 404, { message: 'Not Found' });
    }

    // App authentication.
    const needsJwt = p === '/app' || p === `${issuePath}/installation` || /^\/app\/installations\/\d+\/access_tokens$/.test(p);
    if (needsJwt) {
      const jwt = auth.startsWith('Bearer ') ? auth.slice(7) : '';
      const decoded = jwt && verifyJwt(jwt, fake.publicKey);
      const now = Math.floor(Date.now() / 1000);
      if (!decoded || decoded.header.alg !== 'RS256' || decoded.claims.iss !== appId || decoded.claims.exp <= now || decoded.claims.exp - decoded.claims.iat > 600 || decoded.claims.iat > now) {
        return json(res, 401, { message: 'A JSON web token could not be decoded' });
      }
      fake.jwts.push(jwt);
      if (p === '/app') return json(res, 200, { id: Number(appId), slug, name: 'Silo Workshop' });
      if (p === `${issuePath}/installation`) {
        return fake.installed ? json(res, 200, { id: 77, permissions: fake.installationPermissions }) : json(res, 404, { message: 'Not Found' });
      }
      const token = `ghs_${randomBytes(18).toString('hex')}`;
      fake.tokens.add(token);
      fake.tokenRequests = [...(fake.tokenRequests ?? []), body];
      return json(res, 201, { token, expires_at: new Date(Date.now() + 3600_000).toISOString(), permissions: body?.permissions });
    }

    if (!auth.startsWith('token ') || !fake.tokens.has(auth.slice(6))) return json(res, 401, { message: 'Bad credentials' });
    if (!p.startsWith(`${issuePath}/`)) return json(res, 404, { message: 'Not Found' });
    if (req.method !== 'GET') {
      if (fake.limitAfterWrites !== null && fake.writes >= fake.limitAfterWrites) {
        return json(res, 403, { message: 'API rate limit exceeded' }, { 'x-ratelimit-remaining': '0', 'x-ratelimit-reset': String(Math.floor(Date.now() / 1000) + 600) });
      }
      fake.writes += 1;
    }
    const rest = p.slice(issuePath.length);

    if (rest === '/labels' && req.method === 'GET') return page(res, fake.labels, url);
    if (rest === '/labels' && req.method === 'POST') {
      if (fake.labels.some((l) => l.name.toLowerCase() === body.name.toLowerCase())) return json(res, 422, { message: 'already_exists' });
      fake.labels.push({ name: body.name, color: body.color, description: body.description });
      return json(res, 201, body);
    }
    if (rest === '/issues' && req.method === 'GET') {
      const state = url.searchParams.get('state') ?? 'open';
      const wanted = (url.searchParams.get('labels') ?? '').split(',').filter(Boolean);
      const items = [...fake.issues.values()]
        .filter((i) => state === 'all' || i.state === state)
        .filter((i) => wanted.every((name) => i.labels.includes(name)))
        .sort((a, b) => b.number - a.number)
        .map(fake.issueJson);
      return page(res, items, url);
    }
    if (rest === '/issues' && req.method === 'POST') {
      const issue = fake.addIssue({ title: body.title, body: body.body ?? '', labels: body.labels ?? [], user: bot });
      return json(res, 201, fake.issueJson(issue));
    }
    if (rest === '/issues/comments' && req.method === 'GET') {
      const since = url.searchParams.get('since');
      const items = fake.comments
        .filter((c) => !since || Date.parse(c.updated_at) >= Date.parse(since))
        .sort((a, b) => Date.parse(a.created_at) - Date.parse(b.created_at))
        .map((c) => ({ id: c.id, user: c.user, body: c.body, created_at: c.created_at, updated_at: c.updated_at, issue_url: `${fake.url}${issuePath}/issues/${c.issue}` }));
      return page(res, items, url);
    }
    const unlabel = rest.match(/^\/issues\/(\d+)\/labels\/([^/]+)$/);
    if (unlabel && req.method === 'DELETE') {
      const issue = fake.issues.get(Number(unlabel[1]));
      const name = decodeURIComponent(unlabel[2]);
      if (!issue || !issue.labels.includes(name)) return json(res, 404, { message: 'Label does not exist' });
      issue.labels = issue.labels.filter((n) => n !== name);
      return json(res, 200, issue.labels.map((n) => ({ name: n })));
    }
    const one = rest.match(/^\/issues\/(\d+)(\/comments|\/events|\/labels)?$/);
    if (one) {
      const issue = fake.issues.get(Number(one[1]));
      if (!issue) return json(res, 404, { message: 'Not Found' });
      if (!one[2] && req.method === 'GET') return json(res, 200, fake.issueJson(issue));
      if (!one[2] && req.method === 'PATCH') {
        if ('title' in body) issue.title = body.title;
        if ('body' in body) issue.body = body.body;
        if ('labels' in body) issue.labels = body.labels;
        if (body.state === 'closed' && issue.state !== 'closed') {
          issue.state = 'closed';
          issue.state_reason = body.state_reason ?? 'completed';
          issue.closed_at = tick();
          fake.events.push({ issue: issue.number, event: 'closed', actor: bot, created_at: issue.closed_at });
        } else if (body.state === 'open') {
          issue.state = 'open';
          issue.state_reason = 'reopened';
          issue.closed_at = null;
        }
        return json(res, 200, fake.issueJson(issue));
      }
      if (one[2] === '/comments' && req.method === 'GET') {
        return page(res, fake.comments.filter((c) => c.issue === issue.number).map((c) => ({ id: c.id, user: c.user, body: c.body, created_at: c.created_at })), url);
      }
      if (one[2] === '/comments' && req.method === 'POST') {
        const c = fake.addComment(issue.number, bot.login, body.body, { type: 'Bot' });
        return json(res, 201, { id: c.id, html_url: `${fake.url}/${repo}/issues/${issue.number}#issuecomment-${c.id}` });
      }
      if (one[2] === '/labels' && req.method === 'POST') {
        for (const name of body.labels) if (!issue.labels.includes(name)) issue.labels.push(name);
        return json(res, 200, issue.labels.map((name) => ({ name })));
      }
      if (one[2] === '/events' && req.method === 'GET') return page(res, fake.events.filter((e) => e.issue === issue.number), url);
    }
    const perm = rest.match(/^\/collaborators\/([^/]+)\/permission$/);
    if (perm && req.method === 'GET') {
      const login = decodeURIComponent(perm[1]);
      const permission = fake.permissions[login] ?? 'none';
      const mapped = permission === 'triage' ? 'read' : permission;
      return json(res, 200, { permission: mapped, role_name: fake.roleNames[login] ?? permission, user: { login } });
    }
    return json(res, 404, { message: 'Not Found' });
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  fake.url = `http://127.0.0.1:${server.address().port}`;
  fake.stop = () => {
    server.closeAllConnections();
    return new Promise((resolve) => server.close(resolve));
  };
  return fake;
}
