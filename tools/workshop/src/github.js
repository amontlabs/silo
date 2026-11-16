// A small GitHub REST client: the App JWT, the repository's installation
// token (so every write is the bot's), pagination, rate-limit detection and
// pacing of content-creating requests. Error messages carry the status and
// path only, never a token or a response body.
import { appJwt } from './jwt.js';
import { Failure, RateLimited } from './names.js';

const API_VERSION = '2022-11-28';

function rateLimitOf(response) {
  if (response.status !== 403 && response.status !== 429) return null;
  const remaining = response.headers.get('x-ratelimit-remaining');
  const retryAfter = response.headers.get('retry-after');
  const reset = response.headers.get('x-ratelimit-reset');
  if (remaining === '0') return new RateLimited(reset ? new Date(Number(reset) * 1000) : null);
  if (retryAfter) return new RateLimited(new Date(Date.now() + Number(retryAfter) * 1000));
  if (response.status === 429) return new RateLimited(null);
  return null;
}

export class GitHub {
  constructor({ api, repo, fetch: fetchImpl = globalThis.fetch, writeDelayMs = 1000, sleep }) {
    this.api = api.replace(/\/$/, '');
    this.repo = repo;
    this.fetch = fetchImpl;
    this.writeDelayMs = writeDelayMs;
    this.sleep = sleep ?? ((ms) => new Promise((resolve) => setTimeout(resolve, ms)));
    this.token = null;
    this.lastWrite = 0;
  }

  async request(method, path, { body, auth = this.token ? `token ${this.token}` : null, accept, allow = [] } = {}) {
    const url = path.startsWith('http') ? path : `${this.api}${path}`;
    if (method !== 'GET' && this.writeDelayMs > 0) {
      const wait = this.lastWrite + this.writeDelayMs - Date.now();
      if (wait > 0) await this.sleep(wait);
    }
    const headers = {
      accept: accept ?? 'application/vnd.github+json',
      'x-github-api-version': API_VERSION,
      'user-agent': 'silo-workshop',
    };
    if (auth) headers.authorization = auth;
    if (body !== undefined) headers['content-type'] = 'application/json';
    let response;
    try {
      response = await this.fetch(url, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
    } catch {
      throw new Failure(`could not reach GitHub (${method} ${new URL(url).pathname})`);
    } finally {
      if (method !== 'GET') this.lastWrite = Date.now();
    }
    const limited = rateLimitOf(response);
    if (limited) throw limited;
    if (allow.includes(response.status)) return { status: response.status, data: null, headers: response.headers };
    if (!response.ok) {
      throw new Failure(`GitHub answered ${response.status} to ${method} ${new URL(url).pathname}`);
    }
    const text = await response.text();
    return { status: response.status, data: text ? JSON.parse(text) : null, headers: response.headers };
  }

  async get(path, options) {
    return (await this.request('GET', path, options)).data;
  }

  /** Every page of a list endpoint, following the Link header. */
  async paginate(path) {
    const items = [];
    let next = `${path}${path.includes('?') ? '&' : '?'}per_page=100`;
    while (next) {
      const { data, headers } = await this.request('GET', next);
      items.push(...data);
      const link = headers.get('link') ?? '';
      const match = link.split(',').map((part) => part.match(/<([^>]+)>;\s*rel="next"/)).find(Boolean);
      next = match ? match[1] : null;
    }
    return items;
  }

  /** Authenticates as the App, then as its installation on the repository. */
  async authenticate({ appId, privateKey }) {
    const jwt = appJwt(appId, privateKey);
    const app = await this.get('/app', { auth: `Bearer ${jwt}` });
    this.app = app;
    this.botLogin = `${app.slug}[bot]`;
    const found = await this.request('GET', `/repos/${this.repo}/installation`, { auth: `Bearer ${jwt}`, allow: [404] });
    if (found.status === 404) {
      throw new Failure(`the App is not installed on ${this.repo}: install it from https://github.com/apps/${app.slug}/installations/new`);
    }
    const installation = found.data;
    const name = this.repo.split('/')[1];
    const { data: token } = await this.request('POST', `/app/installations/${installation.id}/access_tokens`, {
      auth: `Bearer ${jwt}`,
      body: { repositories: [name], permissions: { issues: 'write', metadata: 'read' } },
    });
    this.token = token.token;
    this.installation = installation;
    return { app, installation };
  }

  isBot(user) {
    return !user || user.type === 'Bot' || user.login === this.botLogin;
  }

  repoPath(rest) {
    return `/repos/${this.repo}${rest}`;
  }

  /** Issues carrying a label, open and closed, pull requests left out. */
  async issuesWithLabel(label) {
    const issues = await this.paginate(this.repoPath(`/issues?state=all&labels=${encodeURIComponent(label)}`));
    return issues.filter((issue) => !issue.pull_request);
  }
}
