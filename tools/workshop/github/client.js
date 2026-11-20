// A small GitHub REST client: the App JWT, the repository's installation
// token (so every write is the bot's), pagination, rate-limit detection and
// pacing of writes (one a second by default). Error messages carry the status
// and path only, never a token or a response body; a request whose path holds a
// secret names a step instead, and its errors never carry the path.
import { appJwt } from './jwt.js';
import { RateLimited, Unavailable } from './errors.js';

const API_VERSION = '2022-11-28';

/** @param {Response} response */
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

/** The label names an item (an issue, an event's label) carries, lowercased: GitHub's label names are compared ignoring capitalisation. @param {any} item @returns {string[]} */
export const labelNames = (item) => (item?.labels ?? []).map((label) => String((typeof label === 'string' ? label : label?.name) ?? '').toLowerCase());

/** The name an item's label really has on GitHub, matched ignoring capitalisation, or null. @param {any} item @param {string} name */
export function labelOn(item, name) {
  const found = (item?.labels ?? []).map((label) => String((typeof label === 'string' ? label : label?.name) ?? '')).find((label) => label.toLowerCase() === name.toLowerCase());
  return found ?? null;
}

export class GitHub {
  /** @param {{ api: string, repo: string, fetch?: typeof fetch, writeDelayMs?: number, sleep?: (ms: number) => Promise<void> }} options */
  constructor({ api, repo, fetch: fetchImpl = globalThis.fetch, writeDelayMs = 1000, sleep }) {
    this.api = api.replace(/\/$/, '');
    this.repo = repo;
    this.fetch = fetchImpl;
    this.writeDelayMs = writeDelayMs;
    this.sleep = sleep ?? ((ms) => new Promise((resolve) => setTimeout(resolve, ms)));
    /** @type {string | null} */
    this.token = null;
    this.lastWrite = 0;
    /** @type {any} */
    this.app = null;
    /** @type {string | null} */
    this.botLogin = null;
    /** @type {any} */
    this.installation = null;
  }

  /**
   * @param {string} method @param {string} path
   * @param {{ body?: unknown, auth?: string | null, allow?: number[], step?: string }} [options]
   * `step` names the request in errors in place of its path, for a path that holds a secret.
   */
  async request(method, path, { body, auth = this.token ? `token ${this.token}` : null, allow = [], step } = {}) {
    const url = path.startsWith('http') ? path : `${this.api}${path}`;
    const where = step ?? `${method} ${new URL(url).pathname}`;
    if (method !== 'GET' && this.writeDelayMs > 0) {
      const wait = this.lastWrite + this.writeDelayMs - Date.now();
      if (wait > 0) await this.sleep(wait);
    }
    /** @type {Record<string, string>} */
    const headers = { accept: 'application/vnd.github+json', 'x-github-api-version': API_VERSION, 'user-agent': 'silo-workshop' };
    if (auth) headers.authorization = auth;
    if (body !== undefined) headers['content-type'] = 'application/json';
    let response;
    try {
      response = await this.fetch(url, { method, headers, body: body === undefined ? undefined : JSON.stringify(body) });
    } catch {
      throw new Unavailable(`could not reach GitHub (${where})`);
    } finally {
      if (method !== 'GET') this.lastWrite = Date.now();
    }
    const limited = rateLimitOf(response);
    if (limited) throw limited;
    if (allow.includes(response.status)) return { status: response.status, data: null, headers: response.headers };
    if (!response.ok) {
      if (response.status === 403 && /rate limit/i.test(await response.text())) throw new RateLimited(new Date(Date.now() + 60_000));
      throw new Unavailable(`GitHub answered ${response.status} to ${where}`);
    }
    const text = await response.text();
    let data = null;
    try {
      data = text ? JSON.parse(text) : null;
    } catch {
      throw new Unavailable(`GitHub answered ${where} with something that is not JSON`);
    }
    return { status: response.status, data, headers: response.headers };
  }

  /** @param {string} path @param {{ allow?: number[] }} [options] */
  async get(path, options) {
    return (await this.request('GET', path, options)).data;
  }

  /** Every page of a list endpoint, following the Link header. @param {string} path @returns {Promise<any[]>} */
  async paginate(path) {
    const items = [];
    /** @type {string | null} */
    let next = `${path}${path.includes('?') ? '&' : '?'}per_page=100`;
    while (next) {
      const { data, headers } = await this.request('GET', next);
      if (!Array.isArray(data)) throw new Unavailable(`GitHub answered GET ${path.split('?')[0]} with something that is not a list`);
      items.push(...data);
      const link = headers.get('link') ?? '';
      const match = link.split(',').map((part) => part.match(/<([^>]+)>;\s*rel="next"/)).find(Boolean);
      next = match ? match[1] : null;
    }
    return items;
  }

  /** Authenticates as the App, then as its installation on the repository. @param {{ appId: string, privateKey: string }} credentials */
  async authenticate({ appId, privateKey }) {
    const jwt = appJwt(appId, privateKey);
    const app = await this.get('/app', { auth: `Bearer ${jwt}` });
    this.app = app;
    this.botLogin = `${app.slug}[bot]`;
    const found = await this.request('GET', `/repos/${this.repo}/installation`, { auth: `Bearer ${jwt}`, allow: [404] });
    if (found.status === 404) throw new Unavailable(`the App is not installed on ${this.repo}: install it from https://github.com/apps/${app.slug}/installations/new`);
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

  /**
   * True for this App's own bot: the user's login is `<slug>[bot]`, or the item (a comment, an
   * event, an issue) says it was made through this App (`performed_via_github_app`).
   * @param {any} user @param {any} [item]
   */
  isOwnBot(user, item) {
    if (user && this.botLogin && user.login === this.botLogin) return true;
    const via = item?.performed_via_github_app;
    return Boolean(via && this.app && String(via.id) === String(this.app.id) && user?.type === 'Bot');
  }

  /** @param {any} user */
  isBot(user) {
    return !user || user.type === 'Bot' || user.login === this.botLogin;
  }

  /** @param {string} rest */
  repoPath(rest) {
    return `/repos/${this.repo}${rest}`;
  }

  /** Every issue this App's bot created, open and closed, pull requests left out. */
  async botIssues() {
    const issues = await this.paginate(this.repoPath(`/issues?state=all&creator=${encodeURIComponent(/** @type {string} */ (this.botLogin))}`));
    return issues.filter((issue) => !issue.pull_request);
  }

  /** Issues carrying a label, open and closed, pull requests left out. @param {string} label */
  async issuesWithLabel(label) {
    const issues = await this.paginate(this.repoPath(`/issues?state=all&labels=${encodeURIComponent(label)}`));
    return issues.filter((issue) => !issue.pull_request);
  }

  /** @param {number} number @returns {Promise<any | null>} the issue, or null when there is none (or it is a pull request). */
  async issue(number) {
    const found = await this.request('GET', this.repoPath(`/issues/${number}`), { allow: [404, 410] });
    return found.data && !found.data.pull_request ? found.data : null;
  }
}
