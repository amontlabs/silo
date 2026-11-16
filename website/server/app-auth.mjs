// The GitHub App's own identity: an RS256 JWT signed with the App's private
// key, exchanged for an installation token limited to the Workshop repository.
import { createPrivateKey, sign } from 'node:crypto';

const TOKEN_MARGIN_MS = 5 * 60 * 1000;

function base64url(value) {
  return Buffer.from(value).toString('base64url');
}

export function createAppJwt({ appId, privateKey, nowMs }) {
  const key = typeof privateKey === 'string' ? createPrivateKey(privateKey) : privateKey;
  const iat = Math.floor(nowMs / 1000) - 60;
  const header = base64url(JSON.stringify({ alg: 'RS256', typ: 'JWT' }));
  const payload = base64url(JSON.stringify({ iat, exp: iat + 9 * 60, iss: String(appId) }));
  const signature = sign('sha256', Buffer.from(`${header}.${payload}`), key).toString('base64url');
  return `${header}.${payload}.${signature}`;
}

export function createAppAuth({ config, github, now = Date.now }) {
  let cached = null;
  let pending = null;
  let repositoryId = null;

  function appJwt() {
    return createAppJwt({ appId: config.appId, privateKey: config.privateKey, nowMs: now() });
  }

  async function fetchToken() {
    const jwt = appJwt();
    const { data: installation } = await github.request('GET', `/repos/${config.repo}/installation`, { token: jwt, as: 'app' });
    const name = config.repo.split('/')[1];
    const { data } = await github.request('POST', `/app/installations/${installation.id}/access_tokens`, {
      token: jwt,
      as: 'app',
      body: { repositories: [name], permissions: { issues: 'write', metadata: 'read' } },
    });
    return { token: data.token, expiresAt: Date.parse(data.expires_at) };
  }

  async function installationToken() {
    if (cached && cached.expiresAt - now() > TOKEN_MARGIN_MS) return cached.token;
    pending ??= fetchToken().then((value) => { cached = value; return value; }).finally(() => { pending = null; });
    return (await pending).token;
  }

  // Forgets the installation token after GitHub refused it.
  function dropToken() {
    cached = null;
  }

  // The repository's numeric id, read once; `read` makes the call when it is not known yet.
  async function repoId(read) {
    repositoryId ??= await read();
    return repositoryId;
  }

  return { installationToken, dropToken, repoId, appJwt };
}
