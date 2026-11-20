// Shared setup for the Workshop server tests: a fake GitHub on a controllable
// clock, a workshop wired to it, and a client with a cookie jar.
import assert from 'node:assert/strict';
import { readConfig } from '../server/config.mjs';
import { BUDGET, createWorkshop } from '../server/workshop.mjs';
import { SNAPSHOT } from '../server/snapshot.mjs';
import { createFakeGitHub, seedDemo } from './fake-github.mjs';
import { readPlanEntries } from '../server/entries.mjs';

// Tests run with a short floor between write-started refreshes and short waits so they do not wait
// on the real clock; the tests of those numbers set them explicitly.
SNAPSHOT.writeGap = 50;
SNAPSHOT.writeWait = 200;
SNAPSHOT.ageWait = 200;

export const ORIGIN = 'https://silo.test';
export const START = Date.parse('2026-11-16T12:00:00Z');
export const SESSION = '__Host-workshop-session';
export const FAILED = '/workshop?signin=failed';

// The plan's entries the server knows in the tests: the three seedDemo opens issues for (#1 to #3),
// two without an issue, and one written from the suggestion seedDemo opens (#6).
export const PLAN_ENTRIES = [
  { id: 'engine-service', kind: 'feature', title: 'The engine runs on its own', part: 'engine', suggestedIn: null },
  { id: 'calls-and-events', kind: 'feature', title: 'Every call and event defined once', part: 'engine', suggestedIn: null },
  { id: 'vm-runtime', kind: 'feature', title: 'The VM runtime, packaged', part: 'computers', suggestedIn: null },
  { id: 'fork-computers', kind: 'feature', title: 'Fork a computer', part: 'computers', suggestedIn: null },
  { id: 'which-distro', kind: 'decision', title: 'Which Linux distribution do computers start with?', part: null, suggestedIn: null },
  { id: 'share-link', kind: 'feature', title: 'Share a computer with a link', part: 'computers', suggestedIn: 6 },
];

export async function setup({ configured = true, seed = true, timeoutMs, entries = PLAN_ENTRIES } = {}) {
  const clock = { t: START };
  const now = () => clock.t;
  const fake = createFakeGitHub({ now });
  await fake.listen();
  if (seed) seedDemo(fake);
  const logs = [];
  const outputs = [];
  const env = configured ? fake.workshopEnv(ORIGIN) : { WORKSHOP_SITE_ORIGIN: ORIGIN };
  const config = readConfig(env, { apiBase: fake.url, webBase: fake.url });
  const workshop = createWorkshop({ config, entries: readPlanEntries(entries), now, log: (entry) => logs.push(JSON.stringify(entry)), ...(timeoutMs ? { timeoutMs } : {}) });
  const secrets = () => [...fake.issuedTokens, fake.clientSecret, env.WORKSHOP_SESSION_SECRET, ...[...fake.state.codes.keys()]].filter(Boolean);

  // A client of `target`, this setup's workshop unless another instance is given.
  function client(target = workshop) {
    const jar = new Map();
    async function call(method, path, { body, headers = {} } = {}) {
      const h = {};
      if (method === 'POST') Object.assign(h, { origin: ORIGIN, 'x-workshop': '1', 'content-type': 'application/json' });
      if (jar.size) h.cookie = [...jar].map(([name, value]) => `${name}=${value}`).join('; ');
      for (const [name, value] of Object.entries(headers)) {
        if (value === undefined) delete h[name];
        else h[name] = value;
      }
      const response = await target.handle({ method, url: `/api/workshop/${path}`, headers: h, body: body === undefined ? '' : typeof body === 'string' ? body : JSON.stringify(body) });
      outputs.push(JSON.stringify(response));
      for (const cookie of response.cookies) {
        const [pair, ...attributes] = cookie.split('; ');
        const index = pair.indexOf('=');
        if (attributes.includes('Max-Age=0')) jar.delete(pair.slice(0, index));
        else jar.set(pair.slice(0, index), pair.slice(index + 1));
      }
      return { ...response, json: response.body ? JSON.parse(response.body) : null };
    }
    async function signIn(login) {
      const start = await call('GET', 'login?return=/workshop?open=3');
      assert.equal(start.status, 302);
      const authorize = new URL(start.headers.Location);
      const approve = new URL('/login/oauth/fake-approve', fake.url);
      for (const key of ['redirect_uri', 'state', 'code_challenge']) approve.searchParams.set(key, authorize.searchParams.get(key));
      approve.searchParams.set('login', login);
      const back = new URL((await fetch(approve, { redirect: 'manual' })).headers.get('location'));
      assert.equal(back.origin + back.pathname, `${ORIGIN}/api/workshop/callback`);
      const done = await call('GET', `callback${back.search}`);
      assert.equal(done.status, 302, done.body);
      assert.equal(done.headers.Location, '/workshop?open=3');
      return done;
    }
    return { call, signIn, jar };
  }

  return { fake, workshop, config, clock, logs, outputs, secrets, client, close: () => fake.close() };
}

export const count = (fake, method, pattern, kind) => fake.state.requests.filter((r) => r.method === method && pattern.test(r.path) && (!kind || r.kind === kind)).length;
export const issueOf = (fake, n) => fake.state.issues.get(n);
export const entryNumber = 2;
export const retiredNumber = 4;

export async function withBudget(values, run) {
  const saved = { ...BUDGET };
  Object.assign(BUDGET, values);
  try {
    await run();
  } finally {
    Object.assign(BUDGET, saved);
  }
}

