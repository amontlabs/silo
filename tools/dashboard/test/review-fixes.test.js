import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import vm from 'node:vm';
import { fileURLToPath } from 'node:url';
import { ID_SHAPE, parseJourneys, noteTargets, splitSections } from '../src/docs.js';
import { createServer } from '../src/server.js';
import { Store, parseAnchor } from '../src/store.js';
import { renderPage } from '../src/views.js';
import { JOURNEYS, PRODUCT, makeRepo, postJson, request, tempDir, writeDoc } from './helpers.js';

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

const stepRow = (html, journey, key) => {
  const match = new RegExp(`<details class="step [^"]*" id="journey-${journey}-step-${key}" data-step="(\\d+)">\\s*<summary>[\\s\\S]*?</summary>`).exec(html);
  return match && { n: Number(match[1]), row: match[0] };
};
const orphans = (html, view) => {
  const section = new RegExp(`<section class="view" id="${view}"[\\s\\S]*?</section>`).exec(html)[0];
  const list = /<div class="orphans">[\s\S]*$/.exec(section);
  return list ? list[0] : '';
};

test('a note stays with its step when a step is inserted above it', async () => {
  const repo = makeRepo();
  const running = await serve(repo);
  try {
    assert.equal((await running.note({ anchor: 'journeys#first-run/install-the-app', anchorText: 'Install the app', reaction: 'essential' })).status, 201);
    writeDoc(repo, 'JOURNEYS.md', JOURNEYS.replace('- [built] Install the app', '- [missing] Download the installer\n- [built] Install the app'));
    const html = await running.page();
    const moved = stepRow(html, 'first-run', 'install-the-app');
    assert.equal(moved.n, 2, 'the step is now second');
    assert.match(moved.row, /<span class="mine essential">essential<\/span>/);
    assert.doesNotMatch(stepRow(html, 'first-run', 'download-the-installer').row, /class="mine/);
    assert.equal(orphans(html, 'journeys'), '');
  } finally {
    await running.stop();
  }
});

test('renaming a step: its note is kept under "Notes on things that changed", and the old anchor is refused', async () => {
  const repo = makeRepo();
  const running = await serve(repo);
  try {
    await running.note({ anchor: 'journeys#first-run/install-the-app', anchorText: 'Install the app', reaction: 'nice' });
    const { reply_id: first, item_id: item } = running.store.ownerReplies()[0];
    assert.equal((await running.note({ anchor: 'journeys#first-run/install-the-app', anchorText: 'Install the app', text: 'Make it one click', after: first })).status, 201);
    writeDoc(repo, 'JOURNEYS.md', JOURNEYS.replace('Install the app', 'Install Silo'));
    const html = await running.page();
    assert.doesNotMatch(stepRow(html, 'first-run', 'install-silo').row, /class="mine|has-notes/, 'the renamed step shows nothing of the old note');
    const list = orphans(html, 'journeys');
    assert.match(list, /Notes on things that changed/);
    assert.match(list, /<div class="orphan" id="orphan-journeys-first-run-install-the-app">[\s\S]*?<b>Install the app<\/b><span class="mine nice">nice to have<\/span>[\s\S]*?Make it one click/);
    assert.match(list, new RegExp(`data-controls data-item="${item}"`), 'the same comment box, on the open note item');
    // A card for the note links to its entry there.
    running.store.addReply(item, { author: 'agent', text: 'Renamed, kept your note.' });
    assert.match(await running.page(), new RegExp(`<article class="card" id="item-${item}"[\\s\\S]*?href="#orphan-journeys-first-run-install-the-app"`));
    // A page that still shows the old step: 409, nothing recorded; answers unchanged.
    const stale = await running.note({ anchor: 'journeys#first-run/install-the-app', anchorText: 'Install the app', text: 'late', after: 99 });
    assert.equal(stale.status, 409);
    assert.match(JSON.parse(stale.body).error, /no longer in the product documents/);
    assert.deepEqual(running.store.ownerReplies().map((r) => [r.anchor, r.anchor_text]), [
      ['journeys#first-run/install-the-app', 'Install the app'],
      ['journeys#first-run/install-the-app', 'Install the app'],
    ]);
    // The new step takes notes under its own anchor.
    assert.equal((await running.note({ anchor: 'journeys#first-run/install-silo', anchorText: 'Install Silo', reaction: 'essential' })).status, 201);
  } finally {
    await running.stop();
  }
});

test('renaming headings: a product section note moves to the list; a journey keeps its id but refuses the old title', async () => {
  const repo = makeRepo();
  const running = await serve(repo);
  try {
    assert.equal((await running.note({ anchor: 'product#why-it-exists', anchorText: 'Why it exists', text: 'Shorter' })).status, 201);
    assert.equal((await running.note({ anchor: 'journeys#first-run', anchorText: 'First run', text: 'Good' })).status, 201);
    assert.equal((await running.note({ anchor: 'journeys#share-a-workspace', anchorText: 'Share a workspace', text: 'Later' })).status, 201);
    writeDoc(repo, 'PRODUCT.md', PRODUCT.replace('## Why it exists', '## Why Silo exists'));
    writeDoc(repo, 'JOURNEYS.md', JOURNEYS.replace('## First run', '## First start').replace('## Share a workspace', '## Share a computer'));
    const html = await running.page();
    assert.match(orphans(html, 'product'), /<b>Why it exists<\/b>[\s\S]*?Shorter/);
    assert.doesNotMatch(/id="product-why-silo-exists"[\s\S]*?<\/article>/.exec(html)[0], /Shorter/);
    // first-run has an id comment: the journey keeps the note.
    assert.match(/<article class="journey" id="journey-first-run"[\s\S]*?<\/article>/.exec(html)[0], /Good/);
    assert.match(orphans(html, 'journeys'), /<b>Share a workspace<\/b>[\s\S]*?Later/, 'a journey without an id comment changes id with its heading');
    // A page that shows the old title is stale.
    assert.equal((await running.note({ anchor: 'journeys#first-run', anchorText: 'First run', text: 'x', after: 99 })).status, 409);
    const fresh = running.store.ownerReplies().find((r) => r.anchor === 'journeys#first-run');
    assert.equal((await running.note({ anchor: 'journeys#first-run', anchorText: 'First start', text: 'x', after: fresh.reply_id })).status, 201);
  } finally {
    await running.stop();
  }
});

test('a stale page: anchorText that differs from the current title is refused and nothing is stored', async () => {
  const long = 'L'.repeat(250);
  const repo = makeRepo({ journeys: JOURNEYS.replace('Restore a computer', long) });
  const running = await serve(repo);
  try {
    const key = 'l'.repeat(80);
    assert.equal((await running.note({ anchor: 'journeys#first-run/install-the-app', anchorText: 'Install an app', reaction: 'nice' })).status, 409);
    assert.equal(running.store.ownerReplies().length, 0);
    // The page cuts a long title to 200 characters; the server compares the cut values.
    assert.match(await running.page(), new RegExp(`data-anchor="journeys#first-run/${key}" data-anchor-text="${'L'.repeat(200)}"`));
    assert.equal((await running.note({ anchor: `journeys#first-run/${key}`, anchorText: 'L'.repeat(200), reaction: 'nice' })).status, 201);
    assert.equal(running.store.ownerReplies()[0].anchor_text, 'L'.repeat(200));
  } finally {
    await running.stop();
  }
});

test('ids: one shape for journeys, steps and sections; bad id comments are ignored; long ones are cut; empty steps get a title', () => {
  const longHeading = `${'Very long heading '.repeat(8)}`;
  const { journeys } = parseJourneys(`# J

## Bad id
<!-- id: Bad_ID -->

- [built] Same
- [built] Same
- [missing]
- [partial] \`*_\`

## Good id
<!-- id: good-id -->

- [built] One

## ${longHeading}

- [built] ${'step title '.repeat(12)}

## ${longHeading}

- [built] x
`);
  assert.deepEqual(journeys.map((j) => j.id).slice(0, 2), ['bad-id', 'good-id']);
  assert.equal(journeys[2].id.length, 80);
  assert.ok(journeys[3].id.length <= 80 && journeys[3].id.endsWith('-2'));
  assert.deepEqual(journeys[0].steps.map((s) => [s.title, s.key]), [['Same', 'same'], ['Same', 'same-2'], ['Step 3', 'step-3'], ['`*_`', 'step-4']]);
  assert.equal(journeys[2].steps[0].key.length, 80);
  assert.ok(splitSections(`## ${longHeading}`).sections[0].key.length <= 80);
  const targets = noteTargets({ journeys: `## ${longHeading}\n\n- [built] a\n`, product: `## ${longHeading}\n\nx\n` });
  for (const anchor of targets.keys()) assert.doesNotThrow(() => parseAnchor(anchor), anchor);
  // The real product documents give only anchors the server accepts.
  const docs = path.resolve(HERE, '../../../docs/product');
  const real = noteTargets({ journeys: fs.readFileSync(path.join(docs, 'JOURNEYS.md'), 'utf8'), product: fs.readFileSync(path.join(docs, 'PRODUCT.md'), 'utf8') });
  assert.ok(real.size > 10);
  for (const [anchor, target] of real) {
    assert.doesNotThrow(() => parseAnchor(anchor), anchor);
    assert.ok(target.text && target.text.length <= 200, anchor);
  }
  assert.ok(ID_SHAPE.test('a'.repeat(80)) && !ID_SHAPE.test('a'.repeat(81)));
  const store = new Store(tempDir());
  try {
    assert.ok(store.createItem({ kind: 'decide', title: 'x', journey: 'a'.repeat(80) }));
    assert.throws(() => store.createItem({ kind: 'decide', title: 'x', journey: 'a'.repeat(81) }), /journey must be/);
  } finally {
    store.close();
  }
});

test('private notes whose paths slugify alike get unique ids, and so do their heading anchors', () => {
  const html = renderPage({
    items: [],
    docs: { product: null, journeys: null, decisions: null },
    changes: { state: 'nogit', changes: [] },
    notes: [{ folder: '', notes: [{ path: 'a b.md', name: 'a b.md', text: '# One\n\n## Part\n\nx' }, { path: 'a-b.md', name: 'a-b.md', text: '# Two\n\n## Part\n\ny' }] }],
  }).toString();
  assert.deepEqual([...html.matchAll(/<article class="reader" id="([^"]+)"/g)].map((m) => m[1]), ['note-a-b-md', 'note-a-b-md-2']);
  assert.deepEqual([...html.matchAll(/class="tile note-tile" href="#([^"]+)"/g)].map((m) => m[1]), ['note-a-b-md', 'note-a-b-md-2']);
  assert.match(html, /<section id="note-a-b-md--part">/);
  assert.match(html, /<section id="note-a-b-md-2--part">/);
  assert.match(html, /href="#note-a-b-md-2--part">Part/);
});

test('boot.js: any address gives a view, a malformed one For you', () => {
  const source = fs.readFileSync(path.resolve(HERE, '../public/boot.js'), 'utf8');
  const run = (hash) => {
    const root = { dataset: {}, classList: { names: new Set(), add(name) { this.names.add(name); } } };
    const context = { window: {}, location: { hash }, document: { documentElement: root }, decodeURIComponent };
    vm.runInNewContext(source, context);
    return [root.dataset.view, root.classList.names.has('js')];
  };
  assert.deepEqual(run('#%E0%A4%A'), ['for-you', true]);
  assert.deepEqual(run('#%'), ['for-you', true]);
  assert.deepEqual(run('#journey-live-step-x'), ['journeys', true]);
  assert.deepEqual(run('#orphan-product-the-problem'), ['product', true]);
  assert.deepEqual(run('#nonsense'), ['for-you', true]);
  assert.deepEqual(run(''), ['for-you', true]);
});
