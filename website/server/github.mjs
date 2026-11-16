// A small GitHub REST client. Errors carry the HTTP status and whether GitHub's
// rate limit was hit, never the response body or the token.

export class GitHubError extends Error {
  constructor(status, { rateLimited = false, retryAfter = 0, network = false } = {}) {
    super(network ? 'GitHub could not be reached' : `GitHub answered ${status}`);
    this.name = 'GitHubError';
    this.status = status;
    this.rateLimited = rateLimited;
    this.retryAfter = retryAfter;
    this.network = network;
  }
}

const MAX_PAGES = 50;

export function createGitHub({ apiBase, fetch = globalThis.fetch, now = Date.now }) {
  async function request(method, path, { token, auth = 'Bearer', body, accept = 'application/vnd.github+json' } = {}) {
    const url = path.startsWith('http') ? path : apiBase + path;
    const headers = {
      'Accept': accept,
      'User-Agent': 'silo-workshop',
      'X-GitHub-Api-Version': '2022-11-28',
    };
    if (token) headers.Authorization = `${auth} ${token}`;
    if (body !== undefined) headers['Content-Type'] = 'application/json';
    let response;
    try {
      response = await fetch(url, { method, headers, body: body === undefined ? undefined : JSON.stringify(body), redirect: 'error' });
    } catch {
      throw new GitHubError(0, { network: true });
    }
    if (!response.ok) {
      await response.body?.cancel().catch(() => {});
      throw errorFor(response, now());
    }
    if (response.status === 204) return { data: null, headers: response.headers };
    const text = await response.text();
    return { data: text ? JSON.parse(text) : null, headers: response.headers };
  }

  async function paginate(path, options) {
    const items = [];
    let next = path + (path.includes('?') ? '&' : '?') + 'per_page=100';
    for (let page = 0; next && page < MAX_PAGES; page += 1) {
      const { data, headers } = await request('GET', next, options);
      items.push(...data);
      next = nextLink(headers.get('link'));
      if (next && !next.startsWith(apiBase + '/')) throw new GitHubError(502);
    }
    return items;
  }

  return { request, paginate };
}

function errorFor(response, nowMs) {
  const status = response.status;
  const remaining = response.headers.get('x-ratelimit-remaining');
  const retryAfterHeader = Number(response.headers.get('retry-after'));
  const reset = Number(response.headers.get('x-ratelimit-reset'));
  const rateLimited = status === 429 || (status === 403 && (remaining === '0' || retryAfterHeader > 0));
  let retryAfter = 0;
  if (rateLimited) {
    if (retryAfterHeader > 0) retryAfter = retryAfterHeader;
    else if (reset > 0) retryAfter = Math.max(1, Math.ceil(reset - nowMs / 1000));
    else retryAfter = 60;
  }
  return new GitHubError(status, { rateLimited, retryAfter });
}

export function nextLink(header) {
  if (!header) return null;
  for (const part of header.split(',')) {
    const match = part.match(/<([^>]+)>\s*;\s*rel="next"/);
    if (match) return match[1];
  }
  return null;
}
