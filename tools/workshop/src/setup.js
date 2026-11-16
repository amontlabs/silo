// `workshop setup`: registers the "Silo Workshop" App with GitHub's
// app-manifest flow. A server on 127.0.0.1 serves a page whose form posts the
// manifest to GitHub; GitHub sends the browser back to our local callback with
// a one-hour `code` (and our `state`), which is exchanged for the App's id,
// client id, client secret and private key. Those go to the Keychain; the
// variables Vercel needs go to ~/.local/state/silo-workshop/vercel.env (0600).
// https://docs.github.com/en/apps/sharing-github-apps/registering-a-github-app-from-a-manifest
import { randomBytes, timingSafeEqual } from 'node:crypto';
import { chmodSync, existsSync, mkdirSync } from 'node:fs';
import { createServer } from 'node:http';
import { join } from 'node:path';
import { credentialsFile, stateDir } from './credentials.js';
import { writeText } from './files.js';
import { GitHub } from './github.js';
import { CREDENTIAL_NAMES, Failure, UsageError } from './names.js';

export const vercelEnvPath = (home) => join(stateDir(home), 'vercel.env');

const escapeHtml = (text) =>
  text.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);

export function manifest({ site, redirectUrl }) {
  return {
    name: 'Silo Workshop',
    url: `${site}/workshop`,
    description: 'Votes, comments and the owner’s decisions on the Silo plan, stored as issues of amontlabs/silo.',
    redirect_url: redirectUrl,
    callback_urls: [`${site}/api/workshop/callback`],
    public: false,
    default_permissions: { issues: 'write', metadata: 'read' },
    default_events: [],
    request_oauth_on_install: false,
  };
}

function page(title, inner) {
  return `<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>${title}</title>
<style>body{font:16px/1.5 system-ui,sans-serif;max-width:36rem;margin:4rem auto;padding:0 1rem}button{font:inherit;padding:.5rem 1rem}</style>
<body>${inner}</body></html>`;
}

function existing(deps) {
  const found = CREDENTIAL_NAMES.filter((name) => deps.keychain.get(name));
  if (found.length) return 'the Keychain';
  if (existsSync(credentialsFile(deps.home))) return credentialsFile(deps.home);
  if (existsSync(vercelEnvPath(deps.home))) return vercelEnvPath(deps.home);
  return null;
}

export async function setup({ org, site, replace, deps }) {
  if (!/^[A-Za-z0-9](?:[A-Za-z0-9-]{0,38})$/.test(org)) throw new UsageError(`--org ${org} is not an organization name`);
  let origin;
  try {
    const url = new URL(site);
    if (url.protocol !== 'https:' && url.hostname !== '127.0.0.1') throw new Error();
    origin = url.origin;
  } catch {
    throw new UsageError(`--site ${site} is not an https address`);
  }
  if (!deps.keychain.available()) throw new Failure('the macOS Keychain is not available; setup stores the App credentials there');
  const already = existing(deps);
  if (already && !replace) throw new Failure(`Workshop credentials already exist in ${already}; pass --replace to register a new App`);

  const state = randomBytes(16).toString('hex');
  const web = deps.web.replace(/\/$/, '');
  const gh = new GitHub({ api: deps.api, repo: deps.repo, fetch: deps.fetch, writeDelayMs: 0 });
  let finish;
  let fail;
  const done = new Promise((resolve, reject) => {
    finish = resolve;
    fail = reject;
  });
  let exchanging = false;

  const server = createServer(async (req, res) => {
    const url = new URL(req.url, 'http://127.0.0.1');
    const send = (status, html) => {
      res.writeHead(status, { 'content-type': 'text/html; charset=utf-8', 'cache-control': 'no-store', 'referrer-policy': 'no-referrer' });
      res.end(html);
    };
    if (req.method === 'GET' && url.pathname === '/') {
      const { port } = server.address();
      const json = JSON.stringify(manifest({ site: origin, redirectUrl: `http://127.0.0.1:${port}/callback` }));
      const action = `${web}/organizations/${encodeURIComponent(org)}/settings/apps/new?state=${state}`;
      return send(
        200,
        page(
          'Create the Silo Workshop App',
          `<form id="f" method="post" action="${escapeHtml(action)}"><input type="hidden" name="manifest" value="${escapeHtml(json)}">
<p>Creating the Silo Workshop App on GitHub for ${escapeHtml(org)}…</p><button type="submit">Continue on GitHub</button></form>
<script>document.getElementById('f').submit()</script>`,
        ),
      );
    }
    if (req.method === 'GET' && url.pathname === '/callback') {
      const given = Buffer.from(url.searchParams.get('state') ?? '');
      const code = url.searchParams.get('code') ?? '';
      if (given.length !== state.length || !timingSafeEqual(given, Buffer.from(state)) || !/^[A-Za-z0-9_-]{1,200}$/.test(code)) {
        return send(400, page('Not this request', '<p>This link does not belong to the setup that is running.</p>'));
      }
      if (exchanging) return send(409, page('Already done', '<p>This setup already received its code.</p>'));
      exchanging = true;
      try {
        const result = await exchange({ gh, code, origin, deps, web });
        send(
          200,
          page('Silo Workshop created', `<p>The App is created. Install it on ${escapeHtml(deps.repo)}:</p><p><a href="${escapeHtml(result.installUrl)}">${escapeHtml(result.installUrl)}</a></p>`),
        );
        finish(result);
      } catch (error) {
        send(502, page('Setup failed', `<p>${escapeHtml(error.message)}</p>`));
        fail(error);
      }
      return undefined;
    }
    return send(404, page('Not found', '<p>Not found.</p>'));
  });

  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const localUrl = `http://127.0.0.1:${server.address().port}/`;
  deps.log(`Opening ${localUrl} in your browser. On GitHub, click "Create GitHub App".`);
  deps.openBrowser(localUrl);
  const timer = setTimeout(() => fail(new Failure('setup timed out waiting for GitHub (the code is valid for one hour)')), deps.setupTimeoutMs ?? 3_600_000);
  try {
    return await done;
  } finally {
    clearTimeout(timer);
    server.closeAllConnections?.();
    await new Promise((resolve) => server.close(resolve));
  }
}

async function exchange({ gh, code, origin, deps, web }) {
  const { data } = await gh.request('POST', `/app-manifests/${encodeURIComponent(code)}/conversions`, { auth: null });
  const values = {
    WORKSHOP_APP_ID: String(data?.id ?? ''),
    WORKSHOP_CLIENT_ID: data?.client_id ?? '',
    WORKSHOP_CLIENT_SECRET: data?.client_secret ?? '',
    WORKSHOP_PRIVATE_KEY: data?.pem ?? '',
  };
  if (Object.values(values).some((value) => !value)) throw new Failure('GitHub did not return the App credentials');
  for (const [name, value] of Object.entries(values)) deps.keychain.set(name, value);
  const env = {
    ...values,
    WORKSHOP_SESSION_SECRET: randomBytes(32).toString('base64'),
    WORKSHOP_REPO: deps.repo,
    WORKSHOP_SITE_ORIGIN: origin,
  };
  const dir = stateDir(deps.home);
  mkdirSync(dir, { recursive: true, mode: 0o700 });
  const path = vercelEnvPath(deps.home);
  writeText(path, Object.entries(env).map(([name, value]) => `${name}=${JSON.stringify(value)}\n`).join(''), 0o600);
  chmodSync(path, 0o600);
  const slug = data.slug;
  const installUrl = `${(data.html_url ?? `${web}/apps/${slug}`).replace(/\/$/, '')}/installations/new`;
  return { app: slug, installUrl, envFile: path };
}
