import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import { basePlan } from './helpers.js';
import { fakeKeychain, world } from './github-helpers.js';
import { defaultDeps } from '../cli/main.js';
import { readEntryMarker, withoutMarkers } from '../model/markers.js';

/**
 * The Workshop's API over the fake GitHub, in the shape website/server answers today:
 * `state` gives every Workshop issue's summary, `issue?n=` one issue's comments.
 */
async function startSite(t, fake, { status = 200 } = {}) {
  const site = { requests: [], status };
  const kindOf = (issue) => (issue.labels.includes('workshop:entry') ? 'entry' : issue.labels.includes('workshop:suggestion') ? 'suggestion' : null);
  const server = createServer((req, res) => {
    const url = new URL(req.url, 'http://site');
    site.requests.push(url.pathname + url.search);
    const send = (code, body) => {
      res.writeHead(code, { 'content-type': 'application/json' });
      res.end(JSON.stringify(body));
    };
    if (site.status !== 200) return send(site.status, { error: 'down' });
    if (url.pathname === '/api/workshop/state') {
      const issues = {};
      for (const issue of fake.issues.values()) {
        const kind = kindOf(issue);
        if (!kind) continue;
        const verdict = ['take', 'later', 'drop'].find((v) => issue.labels.includes(`verdict:${v}`)) ?? null;
        const building = kind === 'entry' && issue.labels.includes('workshop:building') && issue.state === 'open' && verdict === 'take';
        issues[issue.number] = {
          kind, entry: kind === 'entry' ? readEntryMarker(issue.body) : null, title: issue.title, state: issue.state, reason: null, verdict, up: issue.up ?? 0,
          comments: fake.commentsOn(issue.number).length, author: issue.user.login, bot: issue.user.type === 'Bot', createdAt: issue.created_at, closedAt: null,
          ...(building ? { building: true } : {}),
        };
      }
      return send(200, { repo: fake.repo, fetchedAt: '2026-11-20T12:00:00Z', issues });
    }
    if (url.pathname === '/api/workshop/issue') {
      const issue = fake.issues.get(Number(url.searchParams.get('n')));
      if (!issue) return send(404, { error: 'not_found' });
      const comments = fake.commentsOn(issue.number).map((c) => ({ id: c.id, author: c.user.login, avatar: null, bot: c.user.type === 'Bot', createdAt: c.created_at, text: c.body }));
      return send(200, { number: issue.number, title: issue.title, body: withoutMarkers(issue.body), url: `https://github.com/${fake.repo}/issues/${issue.number}`, up: issue.up ?? 0, more: false, comments });
    }
    return send(404, { error: 'not_found' });
  });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  site.url = `http://127.0.0.1:${server.address().port}`;
  t.after(() => new Promise((resolve) => { server.closeAllConnections(); server.close(resolve); }));
  return site;
}

/** One built, one building (taken, claimed), one taken and not started, one undecided with comments and a history line. */
async function scene(t) {
  const files = basePlan();
  files['features/core.json'] = { ...files['features/core.json'], verdict: 'take', built: '2026-11-01' };
  files['features/cli.json'] = { ...files['features/cli.json'], verdict: 'take' };
  files['features/fork.json'] = { ...files['features/fork.json'], verdict: 'take' };
  const w = await world(t, files);
  assert.equal((await w.run(['sync'])).code, 0);
  for (const id of ['core', 'cli', 'fork']) w.fake.setVerdictLabel(w.fake.entry(id).number, 'take');
  const fork = w.fake.entry('fork');
  w.fake.addComment(fork.number, 'ann', 'Looks right.\nMore.');
  w.change({ fork: { what: 'Copy a computer into a new one.' } });
  assert.equal((await w.run(['sync'])).code, 0);
  w.fake.addComment(fork.number, 'bob', 'Ship it.');
  fork.up = 3;
  assert.equal((await w.run(['start', 'cli', '--branch', 'feat/cli'])).code, 0);
  w.fake.addIssue({ title: 'Loose idea', labels: ['workshop:suggestion'], user: { login: 'visitor', type: 'User' }, up: 2 });
  w.fake.log.length = 0;
  return w;
}

test('live: the site first, one call for the state and one per issue read, no token, no GitHub', async (t) => {
  const w = await scene(t);
  const site = await startSite(t, w.fake);
  const status = await w.run(['status', '--json'], { deps: { site: site.url } });
  assert.equal(status.code, 0, status.err);
  const json = JSON.parse(status.out);
  assert.deepEqual(json.building, [{ id: 'cli', name: 'CLI', branch: 'feat/cli' }]);
  assert.deepEqual(json.taken, [{ id: 'fork', name: 'Fork', ready: true, waiting: [] }]);
  assert.deepEqual(json.ideas.map((i) => i.title), ['Loose idea']);
  assert.deepEqual(site.requests, ['/api/workshop/state', `/api/workshop/issue?n=${w.fake.entry('cli').number}`]);
  assert.equal(w.fake.log.length, 0, 'GitHub is not called');

  site.requests.length = 0;
  const show = JSON.parse((await w.run(['show', 'fork', '--json'], { deps: { site: site.url } })).out);
  assert.equal(show.issue.up, 3);
  assert.deepEqual(show.issue.thread.map((item) => (item.kind === 'comment' ? `${item.author}: ${item.body}` : `history ${item.change.fields}`)), ['ann: Looks right.\nMore.', 'history what', 'bob: Ship it.']);
  assert.ok(site.requests.includes(`/api/workshop/issue?n=${w.fake.entry('fork').number}`));
  assert.equal(w.fake.log.length, 0);
});

test('live: GitHub with the bot\'s token when the site does not answer; the same answer', async (t) => {
  const w = await scene(t);
  const site = await startSite(t, w.fake, { status: 503 });
  const viaSite = await startSite(t, w.fake);
  const fromGitHub = JSON.parse((await w.run(['show', 'fork', '--json'], { deps: { site: site.url } })).out);
  assert.ok(w.fake.log.some((r) => r.path.startsWith('/app')), 'GitHub was read');
  const fromSite = JSON.parse((await w.run(['show', 'fork', '--json'], { deps: { site: viaSite.url } })).out);
  assert.deepEqual(fromGitHub.issue.thread.map(({ kind, author, body, change }) => ({ kind, author, body, change })), fromSite.issue.thread.map(({ kind, author, body, change }) => ({ kind, author, body, change })));
  assert.equal(fromGitHub.status, fromSite.status);
  const status = JSON.parse((await w.run(['status', '--json'], { deps: { site: site.url } })).out);
  assert.deepEqual(status.building, [{ id: 'cli', name: 'CLI', branch: 'feat/cli' }]);
});

test('live: not available when neither answers; offline parts printed, exit 0 when GitHub is not configured, 3 when it failed', async (t) => {
  const w = await scene(t);
  for (const [deps, code] of [[{ keychain: fakeKeychain() }, 0], [{ api: 'http://127.0.0.1:9' }, 3]]) {
    const status = await w.run(['status'], { deps });
    assert.equal(status.code, code);
    assert.match(status.out, /Being built:\n {2}not available: the site did not answer/);
    assert.match(status.out, /Taken, not started:\n {2}cli, CLI: ready\n {2}fork, Fork: ready/);
    const show = await w.run(['show', 'fork'], { deps });
    assert.equal(show.code, code);
    assert.match(show.out, /Issue:\n {2}not available: /);
  }
});

test('PLAN_SITE overrides the site', () => {
  const saved = process.env.PLAN_SITE;
  process.env.PLAN_SITE = 'http://127.0.0.1:4321';
  try {
    assert.equal(defaultDeps().site, 'http://127.0.0.1:4321');
    delete process.env.PLAN_SITE;
    assert.equal(defaultDeps().site, 'https://silo.amontlabs.com');
  } finally {
    if (saved === undefined) delete process.env.PLAN_SITE;
    else process.env.PLAN_SITE = saved;
  }
});
