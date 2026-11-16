// A small GitHub REST client. Every call has a time limit. Errors carry the
// HTTP status, whether GitHub asked us to wait, and which identity made the
// call; never the response body or the token.

export class GitHubError extends Error {
  constructor(status, { rateLimited = false, retryAfter = 0, network = false, timeout = false, as = null } = {}) {
    super(timeout ? 'GitHub did not answer in time' : network ? 'GitHub could not be reached' : `GitHub answered ${status}`);
    this.name = 'GitHubError';
    this.status = status;
    this.rateLimited = rateLimited;
    this.retryAfter = retryAfter;
    this.network = network || timeout;
    this.timeout = timeout;
    this.as = as;
  }

  // A failure that a cached copy may stand in for.
  get transient() {
    return this.rateLimited || this.network || this.status >= 500;
  }
}

export const DEFAULT_TIMEOUT = 8000;
const MAX_PAGES = 50;

// `as` names the identity a call is made with: 'user', 'installation', 'app' or 'client'.
export function createGitHub({ apiBase, fetch = globalThis.fetch, now = Date.now, timeoutMs = DEFAULT_TIMEOUT }) {
  async function request(method, path, { token, auth = 'Bearer', body, accept = 'application/vnd.github+json', as = null } = {}) {
    const url = path.startsWith('http') ? path : apiBase + path;
    const headers = {
      'Accept': accept,
      'User-Agent': 'silo-workshop',
      'X-GitHub-Api-Version': '2022-11-28',
    };
    if (token) headers.Authorization = `${auth} ${token}`;
    if (body !== undefined) headers['Content-Type'] = 'application/json';
    const signal = AbortSignal.timeout(timeoutMs);
    try {
      const response = await fetch(url, { method, headers, body: body === undefined ? undefined : JSON.stringify(body), redirect: 'error', signal });
      if (!response.ok) throw await errorFor(response, now(), as);
      if (response.status === 204) {
        await response.body?.cancel().catch(() => {});
        return { data: null, status: response.status, headers: response.headers };
      }
      const text = await response.text();
      return { data: text ? JSON.parse(text) : null, status: response.status, headers: response.headers };
    } catch (error) {
      if (error instanceof GitHubError) throw error;
      const timeout = signal.aborted || error?.name === 'TimeoutError' || error?.name === 'AbortError';
      throw new GitHubError(0, { network: true, timeout, as });
    }
  }

  // Every page, following GitHub's Link header, never leaving apiBase.
  async function paginate(path, options) {
    const items = [];
    let next = withPerPage(path);
    for (let page = 0; next && page < MAX_PAGES; page += 1) {
      const { data, headers } = await request('GET', next, options);
      items.push(...data);
      next = nextLink(headers.get('link'));
      if (next && !next.startsWith(apiBase + '/')) throw new GitHubError(502, { as: options?.as });
    }
    return items;
  }

  return { request, paginate };
}

function withPerPage(path) {
  return /[?&]per_page=/.test(path) ? path : path + (path.includes('?') ? '&' : '?') + 'per_page=100';
}

async function errorFor(response, nowMs, as) {
  const status = response.status;
  let message = '';
  if (status === 403 || status === 429) {
    message = (await response.text().catch(() => '')).slice(0, 2000);
  } else {
    await response.body?.cancel().catch(() => {});
  }
  const remaining = response.headers.get('x-ratelimit-remaining');
  const retryAfterHeader = Number(response.headers.get('retry-after'));
  const reset = Number(response.headers.get('x-ratelimit-reset'));
  const rateLimited = (status === 403 || status === 429)
    && (status === 429 || remaining === '0' || retryAfterHeader > 0 || /rate limit/i.test(message));
  let retryAfter = 0;
  if (rateLimited) {
    if (retryAfterHeader > 0) retryAfter = retryAfterHeader;
    else if (remaining === '0' && reset > 0) retryAfter = Math.max(1, Math.ceil(reset - nowMs / 1000));
    else retryAfter = 60;
  }
  return new GitHubError(status, { rateLimited, retryAfter, as });
}

export function nextLink(header) {
  if (!header) return null;
  for (const part of header.split(',')) {
    const match = part.match(/<([^>]+)>\s*;\s*rel="next"/);
    if (match) return match[1];
  }
  return null;
}
