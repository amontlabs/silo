import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import { createServer } from '../src/server.js';
import { Store, parseAnchor } from '../src/store.js';
import { makeRepo, postJson, request, runCli, tempDir } from './helpers.js';

function withStore(fn) {
  const store = new Store(tempDir());
  try {
    return fn(store);
  } finally {
    store.close();
  }
}

const two = [{ label: 'A' }, { label: 'B' }];

test('options: only decide items, 2 to 6, labels and details bounded, at most one recommended', () =>
  withStore((store) => {
    assert.throws(() => store.createItem({ kind: 'look', title: 'x', defaultText: 'd', options: two }), /only a decide item takes options/);
    assert.throws(() => store.createItem({ kind: 'shipped', title: 'x', options: two }), /only a decide item/);
    assert.throws(() => store.createItem({ kind: 'decide', title: 'x', options: [{ label: 'A' }] }), /at least 2/);
    assert.throws(() => store.createItem({ kind: 'decide', title: 'x', options: Array.from({ length: 7 }, (_, n) => ({ label: `O${n}` })) }), /at most 6/);
    assert.throws(() => store.createItem({ kind: 'decide', title: 'x', options: [{ label: '' }, { label: 'B' }] }), /option 1 label is required/);
    assert.throws(() => store.createItem({ kind: 'decide', title: 'x', options: [{ label: 'a'.repeat(81) }, { label: 'B' }] }), /longer than 80/);
    assert.throws(() => store.createItem({ kind: 'decide', title: 'x', options: [{ label: 'a\nb' }, { label: 'B' }] }), /single line/);
    assert.throws(() => store.createItem({ kind: 'decide', title: 'x', options: [{ label: 'A', detail: 'd'.repeat(301) }, { label: 'B' }] }), /longer than 300/);
    assert.throws(() => store.createItem({ kind: 'decide', title: 'x', options: [{ label: 'A', recommended: true }, { label: 'B', recommended: true }] }), /at most one/);
    assert.throws(() => store.createItem({ kind: 'note', title: 'x' }), /kind must be one of look, decide, shipped/);
    const id = store.createItem({ kind: 'decide', title: 'x', options: [{ label: ' A ', detail: ' why ' }, { label: 'B', recommended: true }, ...Array.from({ length: 4 }, (_, n) => ({ label: `O${n}` }))] });
    assert.deepEqual(store.getItem(id).options.slice(0, 2), [
      { label: 'A', detail: 'why', recommended: false },
      { label: 'B', detail: null, recommended: true },
    ]);
    assert.equal(store.getItem(store.createItem({ kind: 'decide', title: 'free' })).options, null);
  }));

test('a choice records the option index and its label; only the owner, only on decide items with options', () =>
  withStore((store) => {
    const id = store.createItem({ kind: 'decide', title: 'Pick', options: [{ label: 'Keep' }, { label: 'Drop', recommended: true }] });
    const free = store.createItem({ kind: 'decide', title: 'Free' });
    const look = store.createItem({ kind: 'look', title: 'L', defaultText: 'd' });
    for (const choice of [-1, 2, 1.5, '1']) assert.throws(() => store.addReply(id, { author: 'owner', choice }), /choice must be an option number/, String(choice));
    assert.throws(() => store.addReply(id, { author: 'agent', choice: 0 }), /only the owner/);
    assert.throws(() => store.addReply(free, { author: 'owner', choice: 0 }), /only a decide item with options/);
    assert.throws(() => store.addReply(look, { author: 'owner', choice: 0 }), /only a decide item with options/);
    const reply = store.addReply(id, { author: 'owner', choice: 1, text: 'ignored' });
    const saved = store.getItem(id).replies.find((r) => r.id === reply);
    assert.deepEqual([saved.choice, saved.text, saved.choice_label], [1, 'Drop', 'Drop']);
    assert.deepEqual(store.ownerReplies().map((r) => [r.choice, r.choice_label, r.text]), [[1, 'Drop', 'Drop']]);
    assert.equal(store.itemsWithState().find((item) => item.id === id).state, 'answered');
    // Free text stays an answer too.
    store.addReply(id, { author: 'owner', text: 'Actually, ask me again' });
  }));

test('anchors: three shapes, nothing else', () => {
  assert.deepEqual(parseAnchor('journeys#live/3'), { kind: 'step', journey: 'live', step: 3 });
  assert.deepEqual(parseAnchor('journeys#a-computer-per-task'), { kind: 'journey', journey: 'a-computer-per-task' });
  assert.deepEqual(parseAnchor('product#the-problem'), { kind: 'section', section: 'the-problem' });
  for (const bad of ['', 'journeys#', 'journeys#Live', 'journeys#live/0', 'journeys#live/01', 'journeys#live/x', 'journeys#live/3/4', 'journeys#../x', 'product#', 'product#a b', 'product#A', 'decisions#x', 'journeys#live/3 ', null, 42]) {
    assert.throws(() => parseAnchor(bad), /anchor must be/, String(bad));
  }
});

test('notes: one open item per anchor; reactions replace and clear; text and reaction rules', () =>
  withStore((store) => {
    const step = { anchor: 'journeys#live/3', anchorText: 'T3 Code connects', journey: 'live' };
    assert.throws(() => store.addNote({ ...step }), /send text or a reaction/);
    assert.throws(() => store.addNote({ ...step, reaction: 'love' }), /reaction must be one of/);
    assert.throws(() => store.addNote({ ...step, reaction: null }), /no reaction to clear/);
    assert.throws(() => store.addNote({ ...step, journey: 'task', text: 'x' }), /does not match/);
    assert.throws(() => store.addNote({ anchor: 'product#the-problem', anchorText: 'P', reaction: 'nice' }), /only a journey step takes a reaction/);
    assert.throws(() => store.addNote({ anchor: 'product#the-problem', anchorText: 'P', journey: 'live', text: 'x' }), /no journey/);
    assert.throws(() => store.addNote({ ...step, anchorText: '', text: 'x' }), /anchorText is required/);
    const first = store.addNote({ ...step, reaction: 'essential' });
    const second = store.addNote({ ...step, reaction: 'nice' });
    const third = store.addNote({ ...step, text: 'Need it first' });
    assert.equal(second.item, first.item);
    assert.equal(third.item, first.item);
    const item = store.getItem(first.item);
    assert.deepEqual([item.kind, item.journey, item.anchor, item.anchor_text], ['note', 'live', 'journeys#live/3', 'T3 Code connects']);
    assert.throws(() => store.addNote({ ...step, reaction: null, text: 'x' }), /carries no text/);
    store.addNote({ ...step, reaction: null });
    assert.throws(() => store.addNote({ ...step, reaction: null }), /no reaction to clear/);
    assert.deepEqual(store.ownerReplies().map((r) => [r.reaction, r.text, r.anchor_text]), [
      ['essential', '', 'T3 Code connects'],
      ['nice', '', 'T3 Code connects'],
      [null, 'Need it first', 'T3 Code connects'],
      [null, '', 'T3 Code connects'],
    ]);
    // An owner note never waits; an agent reply brings it to For you; Got it puts it away.
    const state = () => store.itemsWithState().find((entry) => entry.id === first.item).state;
    assert.equal(state(), 'answered');
    const agent = store.addReply(first.item, { author: 'agent', text: 'Noted' });
    assert.equal(state(), 'waiting');
    assert.throws(() => store.addNote({ ...step, text: 'stale', after: agent - 1 }), (error) => error.status === 409);
    store.markShippedSeen(first.item, { after: agent });
    assert.equal(state(), 'answered');
    // After an agent closes it, a new note opens a new item.
    store.closeItem(first.item);
    const fresh = store.addNote({ ...step, text: 'Again' });
    assert.notEqual(fresh.item, first.item);
  }));

let store;
let server;
let port;
before(async () => {
  store = new Store(tempDir());
  server = createServer({ store, repoRoot: makeRepo() });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  port = server.address().port;
});
after(async () => {
  server.closeAllConnections();
  await new Promise((resolve) => server.close(resolve));
  store.close();
});

test('API: a choice is an option number without text or verdict', async () => {
  const origin = { Origin: `http://127.0.0.1:${port}` };
  const id = store.createItem({ kind: 'decide', title: 'Pick', options: [{ label: 'Keep' }, { label: 'Drop', recommended: true }] });
  const html = (await request(port)).body.toString();
  assert.match(html, new RegExp(`data-post="/api/items/${id}/replies" data-body="\\{&quot;choice&quot;:1\\}">Pick the recommended`));
  assert.match(html, /<strong>Drop<\/strong><span class="rec">recommended<\/span>/);
  for (const body of [{ choice: '1' }, { choice: 1, text: 'and words' }, { choice: 1, verdict: 'agree' }, { choice: 5 }, { choice: -1 }]) {
    assert.equal((await postJson(port, `/api/items/${id}/replies`, { after: 0, ...body }, origin)).status, 400, JSON.stringify(body));
  }
  assert.equal((await postJson(port, `/api/items/${id}/replies`, { after: 0, choice: 0 }, { Origin: 'http://evil.example' })).status, 403);
  assert.equal(store.getItem(id).replies.length, 0);
  assert.equal((await postJson(port, `/api/items/${id}/replies`, { after: 0, choice: 1 }, origin)).status, 201);
  assert.equal(store.getItem(id).replies[0].choice_label, 'Drop');
  store.closeItem(id);
});

test('API: /api/notes checks Host, Origin, the anchor and what the page showed', async () => {
  const origin = { Origin: `http://127.0.0.1:${port}` };
  const note = (body, headers = origin) => postJson(port, '/api/notes', { after: 0, anchorText: 'Install the app', ...body }, headers);
  assert.equal((await note({ anchor: 'journeys#first-run/1', reaction: 'essential' }, { Origin: 'http://evil.example' })).status, 403);
  assert.equal((await note({ anchor: 'journeys#first-run/1', reaction: 'essential' }, { Host: 'evil.example', ...origin })).status, 403);
  assert.equal((await note({ anchor: 'journeys#first-run/1', reaction: 'essential' }, { 'Sec-Fetch-Site': 'cross-site' })).status, 403);
  const form = await request(port, { method: 'POST', url: '/api/notes', headers: { ...origin, 'Content-Type': 'text/plain' }, body: '{}' });
  assert.equal(form.status, 400);
  for (const body of [{ anchor: 'journeys#first-run/x', text: 'x' }, { anchor: 'elsewhere', text: 'x' }, { anchor: 'journeys#first-run/1' }, { anchor: 'journeys#first-run/1', text: 5 }, { anchor: 'journeys#first-run/1', text: 'x', after: undefined }, { anchor: 'journeys#first-run/1', text: 'x', anchorText: undefined }]) {
    assert.equal((await note(body)).status, 400, JSON.stringify(body));
  }
  assert.equal(store.ownerReplies({ all: true }).filter((r) => r.kind === 'note').length, 0);

  const created = await note({ anchor: 'journeys#first-run/1', journey: 'first-run', reaction: 'essential' });
  assert.equal(created.status, 201);
  const { item } = JSON.parse(created.body);
  let html = (await request(port)).body.toString();
  assert.match(html, /id="journey-first-run-step-1"[\s\S]*?<span class="mine essential">essential<\/span>/);
  assert.match(html, /data-anchor="journeys#first-run\/1" data-anchor-text="Install the app" data-journey="first-run" data-after="\d+"/);
  assert.match(html, /<button class="pill on" type="button" data-reaction="essential" aria-pressed="true">/);
  assert.doesNotMatch(html, new RegExp(`<article class="card" id="item-${item}"`), 'an owner note is not in For you');

  // An agent replies: the step's next note needs the page to have shown that reply.
  const agent = store.addReply(item, { author: 'agent', text: 'Noted, it goes first.' });
  assert.equal((await note({ anchor: 'journeys#first-run/1', text: 'More' })).status, 409);
  html = (await request(port)).body.toString();
  assert.match(html, new RegExp(`<article class="card" id="item-${item}"[\\s\\S]*?An agent replied to your note[\\s\\S]*?href="#journey-first-run-step-1"`));
  assert.equal((await note({ anchor: 'journeys#first-run/1', text: 'More', after: agent })).status, 201);
  assert.equal((await note({ anchor: 'journeys#first-run/1', reaction: null, after: agent + 1 })).status, 201);
  html = (await request(port)).body.toString();
  const row = /id="journey-first-run-step-1" data-step="1">\s*<summary>[\s\S]*?<\/summary>/.exec(html)[0];
  assert.doesNotMatch(row, /class="mine/, 'a cleared reaction leaves the row');
  assert.match(row, /<span class="has-notes"/);

  assert.equal((await note({ anchor: 'product#why-it-exists', anchorText: 'Why it exists', text: 'Say it shorter' })).status, 201);
  assert.equal((await note({ anchor: 'journeys#first-run', anchorText: 'First run', text: 'Good order' })).status, 201);
  html = (await request(port)).body.toString();
  assert.match(html, /id="product-why-it-exists"[\s\S]*?1 note from you[\s\S]*?Say it shorter/);
});

test('CLI: options and recommend; answers and show print choices, reactions and notes', async () => {
  const home = tempDir();
  const json = (result) => {
    assert.equal(result.code, 0, result.stderr);
    return JSON.parse(result.stdout);
  };
  const usage = async (args, pattern) => {
    const result = await runCli(args, { home });
    assert.equal(result.code, 2, args.join(' '));
    assert.match(result.stderr, pattern);
  };
  await usage(['post', '--kind', 'look', '--title', 'x', '--default', 'd', '--option', 'A', '--option', 'B'], /--option is for --kind decide/);
  await usage(['post', '--kind', 'decide', '--title', 'x', '--recommend', '1'], /--recommend needs --option/);
  await usage(['post', '--kind', 'decide', '--title', 'x', '--option', 'A', '--option', 'B', '--recommend', '3'], /from 1 to 2/);
  await usage(['post', '--kind', 'decide', '--title', 'x', '--option', 'A'], /at least 2/);
  await usage(['post', '--kind', 'note', '--title', 'x'], /--kind must be one of look, decide, shipped/);
  const { id } = json(await runCli(['post', '--kind', 'decide', '--title', 'After a task', '--option', 'Remove it::Nothing piles up', '--option', 'Keep it', '--option', 'Leave it::As today::really', '--recommend', '1', '--json'], { home }));
  const item = json(await runCli(['show', String(id), '--json'], { home }));
  assert.deepEqual(item.options, [
    { label: 'Remove it', detail: 'Nothing piles up', recommended: true },
    { label: 'Keep it', detail: null, recommended: false },
    { label: 'Leave it', detail: 'As today::really', recommended: false },
  ]);
  const help = await runCli(['--help'], { home });
  assert.match(help.stdout, /--option "Label::Detail"/);

  const local = new Store(home);
  local.addReply(id, { author: 'owner', choice: 2 });
  local.addNote({ anchor: 'journeys#live/3', anchorText: 'T3 Code connects', reaction: 'essential' });
  local.addNote({ anchor: 'journeys#live/3', anchorText: 'T3 Code connects', text: 'First, please' });
  local.addNote({ anchor: 'journeys#live/3', anchorText: 'T3 Code connects', reaction: null });
  local.close();

  const answers = json(await runCli(['answers', '--json'], { home }));
  assert.deepEqual(answers.map((a) => [a.kind, a.choice, a.choice_label, a.anchor, a.anchor_text, a.reaction, a.text]), [
    ['decide', 2, 'Leave it', null, null, null, 'Leave it'],
    ['note', null, null, 'journeys#live/3', 'T3 Code connects', 'essential', ''],
    ['note', null, null, 'journeys#live/3', 'T3 Code connects', null, 'First, please'],
    ['note', null, null, 'journeys#live/3', 'T3 Code connects', null, ''],
  ]);
  const plain = (await runCli(['answers'], { home })).stdout;
  assert.match(plain, /item 1 \(decide\) "After a task" · chose 3 "Leave it"\n/);
  assert.match(plain, /note on journeys#live\/3 "T3 Code connects" · reaction essential\n/);
  assert.match(plain, /note on journeys#live\/3 "T3 Code connects"\n {2}First, please/);
  assert.match(plain, /note on journeys#live\/3 "T3 Code connects" · reaction cleared/);
  const shown = (await runCli(['show', String(id)], { home })).stdout;
  assert.match(shown, /Options:\n {2}1\. Remove it \(recommended\) — Nothing piles up\n {2}2\. Keep it\n {2}3\. Leave it — As today::really/);
  assert.match(shown, /· owner · chose 3 "Leave it"/);
  // An agent replies to a note and acknowledges the owner's replies.
  const noteId = answers[1].item_id;
  assert.equal((await runCli(['reply', String(noteId), '--text', 'Noted'], { home })).code, 0);
  assert.equal((await runCli(['ack', ...answers.map((a) => String(a.reply_id))], { home })).code, 0);
  assert.match((await runCli(['list'], { home })).stdout, new RegExp(`#${noteId} {2}note {4}waiting`));
});
