import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';
import { fileURLToPath } from 'node:url';
import { ID_SHAPE, noteTargets, splitSections } from '../src/docs.js';
import { createServer } from '../src/server.js';
import { Store, parseAnchor } from '../src/store.js';
import { renderPage } from '../src/views.js';
import { PRODUCT, makeRepo, postJson, request, tempDir, writeDoc } from './helpers.js';

const HERE = path.dirname(fileURLToPath(import.meta.url));

async function serve(repo) {
  const store = new Store(tempDir());
  const server = createServer({ store, repoRoot: repo });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  const port = server.address().port;
  const origin = { Origin: `http://127.0.0.1:${port}` };
  return {
    store,
    port,
    page: async () => (await request(port)).body.toString(),
    note: (body) => postJson(port, '/api/notes', { after: 0, ...body }, origin),
    stop: async () => {
      server.closeAllConnections();
      await new Promise((resolve) => server.close(resolve));
      store.close();
    },
  };
}

const tile = (html, key) => new RegExp(`<article class="tile [^"]*" id="product-${key}">[\\s\\S]*?</article>`).exec(html)?.[0] ?? null;
const orphans = (html) => {
  const section = /<section class="view" id="product"[\s\S]*?<\/section>/.exec(html)[0];
  const list = /<div class="orphans">[\s\S]*$/.exec(section);
  return list ? list[0] : '';
};

test('a note stays with its section when a section is added above it', async () => {
  const repo = makeRepo();
  const running = await serve(repo);
  try {
    assert.equal((await running.note({ anchor: 'product#who-it-is-for', anchorText: 'Who it is for', text: 'Name them' })).status, 201);
    writeDoc(repo, 'PRODUCT.md', PRODUCT.replace('## Why it exists', '## Pricing\n\nFree.\n\n## Why it exists'));
    const html = await running.page();
    assert.match(tile(html, 'who-it-is-for'), /Name them/);
    assert.doesNotMatch(tile(html, 'pricing'), /Name them/);
    assert.equal(orphans(html), '');
  } finally {
    await running.stop();
  }
});

test('renaming a section: its note is kept under "Notes on things that changed", and the old anchor is refused', async () => {
  const repo = makeRepo();
  const running = await serve(repo);
  try {
    await running.note({ anchor: 'product#why-it-exists', anchorText: 'Why it exists', text: 'Shorter' });
    const { reply_id: first, item_id: item } = running.store.ownerReplies()[0];
    assert.equal((await running.note({ anchor: 'product#why-it-exists', anchorText: 'Why it exists', text: 'Much shorter', after: first })).status, 201);
    writeDoc(repo, 'PRODUCT.md', PRODUCT.replace('## Why it exists', '## Why Silo exists'));
    const html = await running.page();
    assert.doesNotMatch(tile(html, 'why-silo-exists'), /Shorter|note from you/, 'the renamed section shows nothing of the old note');
    const list = orphans(html);
    assert.match(list, /Notes on things that changed/);
    assert.match(list, /<div class="orphan" id="orphan-product-why-it-exists">[\s\S]*?<b>Why it exists<\/b>[\s\S]*?Much shorter/);
    assert.match(list, new RegExp(`data-controls data-item="${item}"`), 'the same comment box, on the open note item');
    // A card for the note links to its entry there.
    running.store.addReply(item, { author: 'agent', text: 'Renamed, kept your note.' });
    assert.match(await running.page(), new RegExp(`<article class="card" id="item-${item}"[\\s\\S]*?href="#orphan-product-why-it-exists"`));
    // A page that still shows the old section: 409, nothing recorded.
    const stale = await running.note({ anchor: 'product#why-it-exists', anchorText: 'Why it exists', text: 'late', after: 99 });
    assert.equal(stale.status, 409);
    assert.match(JSON.parse(stale.body).error, /no longer in the product documents/);
    assert.equal(running.store.ownerReplies().length, 2);
    // The new section takes notes under its own anchor.
    assert.equal((await running.note({ anchor: 'product#why-silo-exists', anchorText: 'Why Silo exists', text: 'Good' })).status, 201);
  } finally {
    await running.stop();
  }
});

test('a stale page: anchorText that differs from the current heading is refused and nothing is stored', async () => {
  const long = 'L'.repeat(250);
  const repo = makeRepo({ product: `${PRODUCT}\n## ${long}\n\nText.\n` });
  const running = await serve(repo);
  try {
    const key = 'l'.repeat(80);
    assert.equal((await running.note({ anchor: 'product#why-it-exists', anchorText: 'Why it exist', text: 'x' })).status, 409);
    assert.equal(running.store.ownerReplies().length, 0);
    // The page cuts a long heading to 200 characters; the server compares the cut values.
    assert.match(await running.page(), new RegExp(`data-anchor="product#${key}" data-anchor-text="${'L'.repeat(200)}"`));
    assert.equal((await running.note({ anchor: `product#${key}`, anchorText: 'L'.repeat(200), text: 'x' })).status, 201);
    assert.equal(running.store.ownerReplies()[0].anchor_text, 'L'.repeat(200));
  } finally {
    await running.stop();
  }
});

test('a heading whose 200th character is a space: the page and the server agree', async () => {
  const heading = `${'a'.repeat(199)} tail words`;
  const repo = makeRepo({ product: `${PRODUCT}\n## ${heading}\n\nText.\n` });
  const running = await serve(repo);
  try {
    const shown = `${'a'.repeat(199)} `;
    const html = await running.page();
    const key = /data-anchor="product#(a{60,}[a-z-]*)"/.exec(html)[1];
    assert.match(html, new RegExp(`data-anchor="product#${key}" data-anchor-text="${shown}"`));
    assert.equal((await running.note({ anchor: `product#${key}`, anchorText: shown, text: 'x' })).status, 201);
    assert.equal((await running.note({ anchor: `product#${key}`, anchorText: shown.trim(), text: 'x', after: 99 })).status, 409, 'compared as sent');
    assert.equal((await running.note({ anchor: `product#${key}`, anchorText: `${shown}x`, text: 'x' })).status, 400, 'longer than 200');
  } finally {
    await running.stop();
  }
});

test('ids: one shape for section keys and entries; long headings are cut; any PRODUCT.md gives anchors the server accepts', () => {
  const longHeading = `${'Very long heading '.repeat(8)}`;
  assert.ok(splitSections(`## ${longHeading}`).sections[0].key.length <= 80);
  const keys = splitSections(`## ${longHeading}\n\n## ${longHeading}\n`).sections.map((section) => section.key);
  assert.ok(keys[1].length <= 80 && keys[1].endsWith('-2'));
  const targets = noteTargets({ product: `## ${longHeading}\n\nx\n` });
  for (const anchor of targets.keys()) assert.doesNotThrow(() => parseAnchor(anchor), anchor);
  const product = `# Silo\n\n## In one sentence\n\nHero.\n\n## Who it is *for*\n\nx\n\n## Rules — that hold \`everywhere\`\n\nx\n\n## Ünïcödé & ümlauts!\n\nx\n\n## ${'Long '.repeat(60)}\n\nx\n\n## Who it is for\n\nx\n`;
  const all = noteTargets({ product });
  assert.equal(all.size, 5);
  for (const [anchor, target] of all) {
    assert.doesNotThrow(() => parseAnchor(anchor), anchor);
    assert.ok(target.text && target.text.length <= 200, anchor);
  }
  assert.ok(ID_SHAPE.test('a'.repeat(80)) && !ID_SHAPE.test('a'.repeat(81)));
});

test('private notes whose paths slugify alike get unique ids, and so do their heading anchors', () => {
  const html = renderPage({
    items: [],
    docs: { product: null, decisions: null, restart: null },
    changes: { state: 'nogit', changes: [] },
    notes: [{ folder: '', notes: [{ path: 'a b.md', name: 'a b.md', text: '# One\n\n## Part\n\nx' }, { path: 'a-b.md', name: 'a-b.md', text: '# Two\n\n## Part\n\ny' }] }],
  }).toString();
  assert.deepEqual([...html.matchAll(/<article class="reader" id="([^"]+)"/g)].map((m) => m[1]), ['note-a-b-md', 'note-a-b-md-2']);
  assert.deepEqual([...html.matchAll(/class="tile note-tile" href="#([^"]+)"/g)].map((m) => m[1]), ['note-a-b-md', 'note-a-b-md-2']);
  assert.match(html, /<section id="note-a-b-md--part">/);
  assert.match(html, /<section id="note-a-b-md-2--part">/);
  assert.match(html, /href="#note-a-b-md-2--part">Part/);
});

test('boot.js: any address gives a view; an empty, unknown or malformed one gives the page default', () => {
  const source = fs.readFileSync(path.resolve(HERE, '../public/boot.js'), 'utf8');
  const run = (hash, fallback) => {
    const root = { dataset: fallback ? { default: fallback } : {}, classList: { names: new Set(), add(name) { this.names.add(name); } } };
    const context = { window: {}, location: { hash }, document: { documentElement: root }, decodeURIComponent };
    vm.runInNewContext(source, context);
    return [root.dataset.view, root.classList.names.has('js')];
  };
  for (const fallback of ['stack', 'for-you']) {
    for (const hash of ['', '#', '#%E0%A4%A', '#%', '#nonsense', '#journeys', '#journey-live-step-x']) assert.deepEqual(run(hash, fallback), [fallback, true], `${hash} with ${fallback}`);
  }
  assert.deepEqual(run('', undefined), ['for-you', true], 'no default written: For you');
  assert.deepEqual(run('', 'other'), ['for-you', true], 'only the Stack can be the other default');
  assert.deepEqual(run('#orphan-product-the-problem', 'stack'), ['product', true]);
  assert.deepEqual(run('#change-product-md-intro', 'stack'), ['product', true]);
  assert.deepEqual(run('#changed', 'stack'), ['product', true]);
  assert.deepEqual(run('#changed', 'for-you'), ['product', true]);
  assert.deepEqual(run('#item-3', 'stack'), ['for-you', true]);
  assert.deepEqual(run('#earlier-items', 'stack'), ['for-you', true]);
  assert.deepEqual(run('#feature-engine-service', 'for-you'), ['stack', true]);
  assert.deepEqual(run('#where-done', 'for-you'), ['stack', true]);
  assert.deepEqual(run('#verdicts-2026-11-16', 'stack'), ['decisions', true]);
  assert.deepEqual(run('#decisions', 'stack'), ['decisions', true]);
});
