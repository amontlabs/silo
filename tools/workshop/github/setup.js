// `workshop setup`: registers the "Silo Workshop" App with GitHub's
// app-manifest flow. A server on 127.0.0.1 serves a page whose form posts the
// manifest to GitHub; GitHub sends the browser back to our local callback with
// a one-hour `code` (and our `state`), which is exchanged for the App's id,
// client id, client secret and private key. Those go to the Keychain; the
// variables Vercel needs go to ~/.local/state/silo-workshop/vercel.env (0600).
// https://docs.github.com/en/apps/sharing-github-apps/registering-a-github-app-from-a-manifest
import { randomBytes, timingSafeEqual } from 'node:crypto';
import { chmodSync, existsSync, mkdirSync, readFileSync, renameSync, statSync, writeFileSync } from 'node:fs';
import { createServer } from 'node:http';
import { join } from 'node:path';
import { credentialsFile, stateDir } from './credentials.js';
import { GitHub } from './client.js';
import { Failure } from './errors.js';
import { CREDENTIAL_NAMES } from './names.js';
import { UsageError } from '../cli/args.js';

export const vercelEnvPath = (home) => join(stateDir(home), 'vercel.env');

const escapeHtml = (text) =>
  text.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);

export function manifest({ site, redirectUrl }) {
  return {
    name: 'Silo Workshop',
    url: `${site}/workshop`,
    description: 'Votes, comments and suggestions on the Silo plan, stored as issues of amontlabs/silo, and the owner\'s verdicts, committed to its main.',
    redirect_url: redirectUrl,
    callback_urls: [`${site}/api/workshop/callback`],
    public: true,
    // Contents: the owner's Take and Drop on the Workshop commit to main with the owner's token, which can do
    // no more than the App may. The installation token is still requested with issues and metadata alone.
    default_permissions: { contents: 'write', issues: 'write', metadata: 'read' },
    default_events: [],
    request_oauth_on_install: false,
  };
}

function page(title, inner) {
  return `<!doctype html><html lang="en"><meta charset="utf-8"><meta name="viewport" content="width=device-width"><title>${title}</title>
<style>body{font:16px/1.5 system-ui,sans-serif;max-width:36rem;margin:4rem auto;padding:0 1rem}button{font:inherit;padding:.5rem 1rem}</style>
<body>${inner}</body></html>`;
}

/**
 * What the Keychain holds compared with vercel.env:
 * - env: the four App values from vercel.env, or null when the file does not exist;
 * - held: the names the Keychain has; complete: all four;
 * - matches: every value the Keychain has equals vercel.env's (vacuously true without the file);
 * - appIds: { env, keychain } for messages.
 */
export function credentialState(deps) {
  const path = vercelEnvPath(deps.home);
  const env = existsSync(path) ? readVercelEnv(path) : null;
  if (env) {
    const missing = CREDENTIAL_NAMES.filter((name) => !env[name]);
    if (missing.length) throw new Failure(`${path} has no ${missing.join(', ')}`);
  }
  const keychain = Object.fromEntries(CREDENTIAL_NAMES.map((name) => [name, deps.keychain.get(name)]).filter(([, value]) => value));
  const held = Object.keys(keychain);
  return {
    path,
    env,
    keychain,
    held,
    complete: held.length === CREDENTIAL_NAMES.length,
    matches: !env || held.every((name) => keychain[name] === env[name]),
    appIds: { env: env?.WORKSHOP_APP_ID ?? null, keychain: keychain.WORKSHOP_APP_ID ?? null },
  };
}

/** Why setup must not register an App now, or null. */
function setupRefusal(state, replace, deps) {
  if (state.env) {
    if (state.complete && state.matches) {
      return replace ? null : `Workshop credentials already exist in the Keychain; pass --replace to register a new App`;
    }
    if (state.complete) {
      return `${state.path} holds App ${state.appIds.env} but the Keychain holds App ${state.appIds.keychain}: run \`workshop store --replace\` to put the App of ${state.path} in the Keychain, or delete ${state.path} to keep the Keychain's, then run setup again`;
    }
    // A setup that made the App but did not finish storing it: finishing never needs a second App.
    return `${state.path} holds the credentials of App ${state.appIds.env}, which the Keychain does not fully hold: run \`workshop store\` to finish storing them`;
  }
  if (replace) return null;
  if (state.held.length) return `Workshop credentials already exist in the Keychain; pass --replace to register a new App`;
  if (existsSync(credentialsFile(deps.home))) return `Workshop credentials already exist in ${credentialsFile(deps.home)}; pass --replace to register a new App`;
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
  const refusal = setupRefusal(credentialState(deps), replace, deps);
  if (refusal) throw new Failure(refusal);

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
    const { port } = server.address();
    if (req.headers.host !== `127.0.0.1:${port}` && req.headers.host !== `localhost:${port}`) {
      return send(421, page('Wrong address', '<p>Open this page at 127.0.0.1.</p>'));
    }
    if (req.method === 'GET' && url.pathname === '/') {
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
  // The path holds the code, which GitHub exchanges for the App's private key
  // and client secret: errors name the step, never the path.
  const { data } = await gh.request('POST', `/app-manifests/${encodeURIComponent(code)}/conversions`, { auth: null, step: 'the App conversion' });
  const values = {
    WORKSHOP_APP_ID: String(data?.id ?? ''),
    WORKSHOP_CLIENT_ID: data?.client_id ?? '',
    WORKSHOP_CLIENT_SECRET: data?.client_secret ?? '',
    WORKSHOP_PRIVATE_KEY: data?.pem ?? '',
  };
  if (Object.values(values).some((value) => !value)) throw new Failure('GitHub did not return the App credentials');
  const env = {
    ...values,
    WORKSHOP_SESSION_SECRET: randomBytes(32).toString('base64'),
    WORKSHOP_REPO: deps.repo,
    WORKSHOP_SITE_ORIGIN: origin,
  };
  // The file first: once GitHub has made the App, its credentials exist nowhere
  // else, so a Keychain failure must not lose them.
  const dir = stateDir(deps.home);
  mkdirSync(dir, { recursive: true, mode: 0o700 });
  const path = vercelEnvPath(deps.home);
  writeText(path, Object.entries(env).map(([name, value]) => `${name}=${JSON.stringify(value)}\n`).join(''), 0o600);
  chmodSync(path, 0o600);
  const slug = data.slug;
  const installUrl = `${(data.html_url ?? `${web}/apps/${slug}`).replace(/\/$/, '')}/installations/new`;
  try {
    storeAll(values, deps.keychain);
  } catch (error) {
    throw new Failure(
      `the App ${slug} exists on GitHub and its credentials are in ${path}, but the Keychain failed (${error.message}); ` +
        `run \`workshop store\` to finish, then install it: ${installUrl}`,
    );
  }
  return { app: slug, installUrl, envFile: path };
}

function storeAll(values, keychain) {
  for (const [name, value] of Object.entries(values)) keychain.set(name, value);
}

/** Reads vercel.env as setup wrote it: NAME="json string" per line. */
export function readVercelEnv(path) {
  let stat;
  try {
    stat = statSync(path);
  } catch {
    throw new Failure(`${path} does not exist: run \`workshop setup\` first`);
  }
  if ((stat.mode & 0o077) !== 0) throw new Failure(`${path} must be readable only by you (chmod 600); refusing to read it`);
  const values = {};
  for (const line of readFileSync(path, 'utf8').split('\n')) {
    if (!line.trim()) continue;
    const match = line.match(/^([A-Z_]+)=(".*")$/);
    let value;
    try {
      value = match && JSON.parse(match[2]);
    } catch {
      value = null;
    }
    if (typeof value !== 'string') throw new Failure(`${path} has a line that is not NAME="value"`);
    values[match[1]] = value;
  }
  return values;
}

/** `workshop store`: puts the four App credentials from vercel.env into the Keychain. */
export function store({ replace, deps }) {
  if (!deps.keychain.available()) throw new Failure('the Keychain is locked or unavailable');
  const state = credentialState(deps);
  if (!state.env) throw new Failure(`${state.path} does not exist: run \`workshop setup\` first`);
  if (state.complete && state.matches) return { stored: [], from: state.path, already: true };
  if (state.complete && !replace) {
    throw new Failure(`the Keychain holds a complete set for App ${state.appIds.keychain}, not the App ${state.appIds.env} of ${state.path}; pass --replace to overwrite it`);
  }
  // A partial set (a store or setup cut short) is completed or overwritten without --replace.
  const stored = CREDENTIAL_NAMES.filter((name) => state.keychain[name] !== state.env[name]);
  storeAll(Object.fromEntries(stored.map((name) => [name, state.env[name]])), deps.keychain);
  return { stored, from: state.path, already: false };
}

/** Replaces a file in one step, so an interrupted run never leaves half a file. */
function writeText(path, text, mode) {
  const temp = `${path}.workshop-${process.pid}.tmp`;
  writeFileSync(temp, text, mode === undefined ? undefined : { mode });
  renameSync(temp, path);
}
