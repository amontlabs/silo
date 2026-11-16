import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { createServer } from '../src/server.js';
import { Store } from '../src/store.js';
import { commitAll, DECISIONS, git, makeRepo, postJson, request, tempDir, writeDoc } from './helpers.js';

let store;
let server;
let port;
let repo;

async function start(options = {}) {
  const home = options.home ?? tempDir();
  const s = new Store(home);
  const srv = createServer({ store: s, repoRoot: options.repo ?? makeRepo() });
  await new Promise((resolve) => srv.listen(0, '127.0.0.1', resolve));
  return { store: s, server: srv, port: srv.address().port };
}

async function stop(running) {
  running.server.closeAllConnections();
  await new Promise((resolve) => running.server.close(resolve));
  running.store.close();
}

before(async () => {
  repo = makeRepo();
  const running = await start({ repo });
  ({ store, server, port } = running);
});

after(() => stop({ store, server }));

const page = async (p = port) => (await request(p)).body.toString();

test('the page renders every section against the fixture', async () => {
  const id = store.createItem({ kind: 'decide', title: 'Pick a default shell', journey: 'first-run' });
  const response = await request(port);
  assert.equal(response.status, 200);
  assert.match(response.headers['content-type'], /text\/html/);
  assert.match(response.headers['content-security-policy'], /script-src 'self'/);
  const html = response.body.toString();
  for (const region of ['nav', 'for-you', 'product', 'journeys', 'decisions', 'history']) assert.match(html, new RegExp(`data-region="${region}"`));
  assert.doesNotMatch(html, /data-region="private"/, 'no private notes: section hidden');
  assert.match(html, /Silo gives every agent its own computer/);
  assert.match(html, /id="journey-first-run"/);
  assert.match(html, /id="journey-share-a-workspace"/);
  assert.match(html, /<span class="chip step-built">built<\/span> Install the app/);
  assert.match(html, /<span class="chip step-untested">untested<\/span> Restore a computer/);
  assert.match(html, /aria-label="1 built, 1 partial, 1 missing, 1 untested"/);
  assert.match(html, /<pre class="mermaid">/);
  assert.match(html, /data-total="3">3 decisions/);
  assert.match(html, new RegExp(`id="item-${id}"`));
  assert.match(html, /Changed since you last looked/);
  store.closeItem(id);
});

test('missing product docs render empty states', async () => {
  const empty = tempDir();
  git(empty, 'init', '-q');
  const running = await start({ repo: empty });
  try {
    const html = await page(running.port);
    assert.match(html, /PRODUCT\.md is missing/);
    assert.match(html, /JOURNEYS\.md is missing/);
    assert.match(html, /DECISIONS\.md is missing/);
  } finally {
    await stop(running);
  }
});

test('private notes render grouped by folder; links out of the folder are not shown', async () => {
  const home = tempDir();
  fs.mkdirSync(path.join(home, 'private', 'pricing'), { recursive: true });
  fs.writeFileSync(path.join(home, 'private', 'b.md'), '# Bee note\n\n<em>raw html stays</em>');
  fs.writeFileSync(path.join(home, 'private', 'a.md'), '# Ay note');
  fs.writeFileSync(path.join(home, 'private', 'pricing', 'plans.md'), '- Plan **one**');
  const outside = tempDir();
  fs.writeFileSync(path.join(outside, 'secret.md'), 'OUTSIDE SECRET');
  fs.symlinkSync(path.join(outside, 'secret.md'), path.join(home, 'private', 'escape.md'));
  const running = await start({ home });
  try {
    let html = await page(running.port);
    assert.match(html, /data-region="private"/);
    assert.match(html, /href="#private">Private<span class="count-badge ">3<\/span>/);
    const region = /<section id="private"[\s\S]*?<\/section>/.exec(html)[0];
    assert.ok(region.indexOf('Ay note') < region.indexOf('Bee note'));
    assert.ok(region.indexOf('Bee note') < region.indexOf('pricing/</h3>'));
    assert.match(region, /&lt;em&gt;raw html stays&lt;\/em&gt;/);
    assert.match(region, /Plan <strong>one<\/strong>/);
    assert.match(region, /id="note-pricing-plans-md"/);
    assert.doesNotMatch(html, /OUTSIDE SECRET/);

    // Read live: a new file shows on the next request, an emptied folder hides the section.
    fs.writeFileSync(path.join(home, 'private', 'c.md'), 'Fresh note');
    assert.match(await page(running.port), /Fresh note/);
    fs.rmSync(path.join(home, 'private'), { recursive: true });
    html = await page(running.port);
    assert.doesNotMatch(html, /data-region="private"/);
    assert.doesNotMatch(html, /href="#private"/);
  } finally {
    await stop(running);
  }
});

test('Host and Origin: other names and other origins are refused', async () => {
  assert.equal((await request(port, { headers: { Host: 'evil.example' } })).status, 403);
  assert.equal((await request(port, { headers: { Host: `evil.example:${port}` } })).status, 403);
  assert.equal((await request(port, { headers: { Host: `localhost:${port}` } })).status, 200);
  const id = store.createItem({ kind: 'decide', title: 'Origin check' });
  const url = `/api/items/${id}/replies`;
  for (const origin of ['http://evil.example', `http://127.0.0.1:${port + 1}`, 'null', `https://127.0.0.1:${port}`]) {
    assert.equal((await postJson(port, url, { text: 'x', after: 0 }, { Origin: origin })).status, 403, origin);
  }
  assert.equal((await postJson(port, url, { text: 'x', after: 0 }, { Host: 'evil.example', Origin: 'http://evil.example' })).status, 403);
  assert.equal((await postJson(port, url, { text: 'x', after: 0 }, { 'Sec-Fetch-Site': 'cross-site' })).status, 403);
  // A form post (not JSON) is refused even from the page's own origin.
  const form = await request(port, { method: 'POST', url, headers: { Origin: `http://127.0.0.1:${port}`, 'Content-Type': 'application/x-www-form-urlencoded' }, body: 'text=x&after=0' });
  assert.equal(form.status, 400);
  assert.equal(store.getItem(id).replies.length, 0);
  assert.equal((await postJson(port, url, { text: 'fine', after: 0 }, { Origin: `http://localhost:${port}` })).status, 201);
  assert.equal(store.getItem(id).replies.length, 1);
  assert.equal((await request(port, { method: 'DELETE', url })).status, 405);
  store.closeItem(id);
});

test('owner actions: agree, disagree needs text, decide needs words, seen for shipped, mark docs seen', async () => {
  const origin = { Origin: `http://127.0.0.1:${port}` };
  const look = store.createItem({ kind: 'look', title: 'Look', defaultText: 'd' });
  const decide = store.createItem({ kind: 'decide', title: 'Decide' });
  const shipped = store.createItem({ kind: 'shipped', title: 'Shipped' });
  const reply = (id, body) => postJson(port, `/api/items/${id}/replies`, { after: 0, ...body }, origin);
  assert.equal((await reply(look, { verdict: 'disagree', text: ' ' })).status, 400);
  assert.equal((await reply(look, { verdict: 'agree' })).status, 201);
  assert.equal((await reply(decide, { verdict: 'agree' })).status, 400);
  assert.equal((await reply(decide, { text: '' })).status, 400);
  assert.equal((await reply(decide, { text: 'Use zsh' })).status, 201);
  assert.equal((await postJson(port, `/api/items/${look}/seen`, { after: 1e9 }, origin)).status, 400);
  assert.equal((await postJson(port, `/api/items/${shipped}/seen`, { after: 0 }, origin)).status, 200);
  assert.equal((await reply(9999, { text: 'x' })).status, 404);
  const states = Object.fromEntries(store.itemsWithState().map((item) => [item.id, item.state]));
  assert.deepEqual([states[look], states[decide], states[shipped]], ['answered', 'answered', 'answered']);
  for (const id of [look, decide, shipped]) store.closeItem(id);
});

test('owner actions carry the newest reply the page showed; a newer one gives 409 and changes nothing', async () => {
  const origin = { Origin: `http://127.0.0.1:${port}` };
  const look = store.createItem({ kind: 'look', title: 'Stale look', defaultText: 'd' });
  const shipped = store.createItem({ kind: 'shipped', title: 'Stale shipped' });

  const html = await page();
  assert.match(html, new RegExp(`data-controls data-item="${look}" data-after="0"`));
  const agentReply = store.addReply(look, { author: 'agent', text: 'Changed my mind: now it is blue.' });
  store.addReply(shipped, { author: 'agent', text: 'One more fix landed.' });

  // Missing or malformed: refused.
  for (const after of [undefined, -1, 'x', 1.5]) {
    assert.equal((await postJson(port, `/api/items/${look}/replies`, { verdict: 'agree', after }, origin)).status, 400, String(after));
  }
  // Stale: 409 for agree, disagree, an answer, a comment and Seen.
  for (const body of [{ verdict: 'agree' }, { verdict: 'disagree', text: 'no' }, { text: 'comment' }]) {
    const response = await postJson(port, `/api/items/${look}/replies`, { ...body, after: 0 }, origin);
    assert.equal(response.status, 409);
    assert.match(JSON.parse(response.body).error, /newer reply/);
  }
  assert.equal((await postJson(port, `/api/items/${shipped}/seen`, { after: 0 }, origin)).status, 409);
  assert.equal(store.getItem(look).replies.length, 1);
  assert.equal(store.itemsWithState().find((item) => item.id === shipped).state, 'waiting');

  // With what the page shows now, both go through.
  assert.match(await page(), new RegExp(`data-item="${look}" data-after="${agentReply}"`));
  assert.equal((await postJson(port, `/api/items/${look}/replies`, { verdict: 'agree', after: agentReply }, origin)).status, 201);
  const shippedAfter = store.getItem(shipped).replies.at(-1).id;
  assert.equal((await postJson(port, `/api/items/${shipped}/seen`, { after: shippedAfter }, origin)).status, 200);
  store.closeItem(look);
  store.closeItem(shipped);
});

test('Mark all as seen records the commit the page showed, not the current one', async () => {
  const origin = { Origin: `http://127.0.0.1:${port}` };
  const shown = git(repo, 'rev-parse', 'HEAD').trim();
  assert.match(await page(), new RegExp(`data-post="/api/docs/seen" data-body="\\{&quot;commit&quot;:&quot;${shown}&quot;\\}"`));
  // A commit lands between rendering and the click.
  writeDoc(repo, 'DECISIONS.md', `${DECISIONS}\n## Later\n\n- Landed after the page was rendered.\n`);
  const later = commitAll(repo, 'later decision');
  for (const commit of [undefined, 'abc', shown.slice(0, 12), `${shown}; rm -rf /`, 'f'.repeat(40)]) {
    assert.equal((await postJson(port, '/api/docs/seen', { commit }, origin)).status, 400, String(commit));
  }
  const response = await postJson(port, '/api/docs/seen', { commit: shown }, origin);
  assert.equal(response.status, 200);
  assert.equal(store.getSeen('docs_commit'), shown);
  const html = await page();
  assert.match(html, /1 section of the product documents changed since you last looked/);
  assert.match(html, /Landed after the page was rendered/);
  assert.equal((await postJson(port, '/api/docs/seen', { commit: later }, origin)).status, 200);
  assert.match(await page(), /No changes in the product documents since you last looked/);
});

test('item bodies, replies, titles and defaults are escaped', async () => {
  const id = store.createItem({
    kind: 'look',
    title: '<img src=x onerror=alert(1)>',
    body: 'Hello <script>alert(1)</script>\n\n<iframe src="http://evil"></iframe>\n\n[click](javascript:alert(1)) ![i](http://evil/x.png)',
    defaultText: '<b onclick="x()">bold</b>',
    source: '<source>',
  });
  store.addReply(id, { author: 'agent', text: '<style>body{display:none}</style>' });
  const html = await page();
  const card = new RegExp(`<article class="item[^"]*" id="item-${id}">[\\s\\S]*?</article>`).exec(html)[0];
  assert.doesNotMatch(card, /<script|<iframe|<img src=x|<b onclick|<style>|javascript:|<source>/);
  assert.match(card, /&lt;script&gt;alert\(1\)&lt;\/script&gt;/);
  assert.match(card, /&lt;img src=x onerror=alert\(1\)&gt;/);
  assert.match(card, /<p>Hello [^<]*<\/p>/);
  assert.doesNotMatch(card, /<a [^>]*>click/);
  store.closeItem(id);
});

test('attachments: served from the store with ranges; inline only for media', async () => {
  const id = store.createItem({ kind: 'shipped', title: 'Video' });
  const source = tempDir();
  fs.writeFileSync(path.join(source, 'clip.mp4'), Buffer.from('0123456789'));
  fs.writeFileSync(path.join(source, 'page.html'), '<script>alert(1)</script>');
  const video = store.addAttachment(id, path.join(source, 'clip.mp4'));
  const htmlFile = store.addAttachment(id, path.join(source, 'page.html'));

  const full = await request(port, { url: `/attachments/${video.id}/clip.mp4` });
  assert.equal(full.status, 200);
  assert.equal(full.headers['content-type'], 'video/mp4');
  assert.equal(full.headers['accept-ranges'], 'bytes');
  assert.match(full.headers['content-disposition'], /^inline;/);
  assert.equal(full.body.toString(), '0123456789');

  const part = await request(port, { url: `/attachments/${video.id}`, headers: { Range: 'bytes=2-5' } });
  assert.equal(part.status, 206);
  assert.equal(part.headers['content-range'], 'bytes 2-5/10');
  assert.equal(part.body.toString(), '2345');
  const tail = await request(port, { url: `/attachments/${video.id}`, headers: { Range: 'bytes=-3' } });
  assert.equal(tail.body.toString(), '789');
  const open = await request(port, { url: `/attachments/${video.id}`, headers: { Range: 'bytes=8-' } });
  assert.equal(open.body.toString(), '89');
  const bad = await request(port, { url: `/attachments/${video.id}`, headers: { Range: 'bytes=20-30' } });
  assert.equal(bad.status, 416);
  assert.equal(bad.headers['content-range'], 'bytes */10');

  const download = await request(port, { url: `/attachments/${htmlFile.id}` });
  assert.equal(download.status, 200);
  assert.equal(download.headers['content-type'], 'application/octet-stream');
  assert.match(download.headers['content-disposition'], /^attachment;/);
  assert.match(download.headers['content-security-policy'], /sandbox/);

  // A file on disk that the database does not record is not served.
  fs.writeFileSync(path.join(store.home, 'attachments', String(id), 'unrecorded.txt'), 'x');
  assert.equal((await request(port, { url: `/attachments/${id}/unrecorded.txt` })).status, 404);
  assert.equal((await request(port, { url: '/attachments/99999' })).status, 404);
  store.closeItem(id);
});

test('path escapes get nothing', async () => {
  const secret = path.join(store.home, 'dashboard.db');
  assert.ok(fs.existsSync(secret));
  for (const url of [
    '/attachments/../dashboard.db',
    '/attachments/%2e%2e/dashboard.db',
    '/attachments/1/..%2f..%2fdashboard.db',
    '/vendor/mermaid/../../../package.json',
    '/vendor/mermaid/..%2f..%2f..%2fpackage.json',
    '/vendor/mermaid/%2e%2e%2fpackage.json',
    '/vendor/mermaid/..%2f..%2f..%2f..%2f..%2fsrc%2fserver.js',
    '/vendor/mermaid/%E0%A4%A',
    '/docs/product/PRODUCT.md',
    '/../docs/product/PRODUCT.md',
    '/app.css/../../src/server.js',
    '/public/app.js',
  ]) {
    const response = await request(port, { url });
    assert.notEqual(response.status, 200, url);
    assert.doesNotMatch(response.body.toString(), /SQLite format|"name": "silo-owner-dashboard"|createServer/, url);
  }
});

test('static files and mermaid are served', async () => {
  const css = await request(port, { url: '/app.css' });
  assert.equal(css.status, 200);
  assert.match(css.headers['content-type'], /text\/css/);
  assert.match(css.body.toString(), /--untested/);
  assert.equal((await request(port, { url: '/app.js' })).status, 200);
  const mermaid = await request(port, { url: '/vendor/mermaid/mermaid.esm.min.mjs', method: 'HEAD' });
  assert.equal(mermaid.status, 200);
  assert.match(mermaid.headers['content-type'], /javascript/);
});
