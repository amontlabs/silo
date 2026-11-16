import { test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { createServer } from '../src/server.js';
import { Store } from '../src/store.js';
import {
  appWords,
  currentVerdicts,
  entryDigest,
  nextUndecided,
  parseFeatures,
  parseStages,
  readFeatures,
  schematicParts,
  stackTotals,
} from '../src/features.js';
import { prepareStack, stackView } from '../src/stack-view.js';
import { renderPage } from '../src/views.js';
import { makeRepo, postJson, request, runCli, tempDir, writeDoc } from './helpers.js';

const line = (fields) => JSON.stringify(fields);
const feature = (n, id, stage, extra = {}) => ({ n, id, stage, name: `Name ${id}`, what: `What ${id}`, old: 'built', old_lines: 100, reuse: 'reuse', suggest: 'take', why: `Why ${id}`, app: null, cli: null, verdict: null, ...extra });

const FEATURES = [
  feature(1, 'engine', 'shell', { app: 'window', cli: 'silo' }),
  feature(2, 'create', 'shell', { app: 'button:New computer', cli: 'silo create', suggest: 'refine' }),
  feature(3, 'terminal', 'shell', { app: 'button:Terminal', cli: 'ssh main.silo', verdict: 'take' }),
  feature(4, 'fork', 'loop', { app: 'button:Fork', cli: 'silo fork', suggest: 'later' }),
  feature(5, 'changes', 'loop', { app: 'tab:Changes' }),
  feature(6, 'tree', 'loop', { app: 'tree', suggest: 'drop' }),
  feature(7, 'secrets', 'living', { app: 'side:Secrets', cli: 'silo secrets' }),
  feature(8, 'later-one', 'extras', { app: 'menubar' }),
];
const STAGES = [
  { id: 'shell', title: 'Shell', line: 'One computer.' },
  { id: 'loop', title: 'The loop', line: 'Fork and merge.' },
  { id: 'living', title: 'Living in it', line: 'Daily use.' },
  { id: 'extras', title: 'Extras', line: 'More.' },
];

function stackRepo({ features = FEATURES, stages = STAGES } = {}) {
  const repo = makeRepo({ commit: false });
  if (features !== null) writeDoc(repo, 'features.jsonl', features.map(line).join('\n') + '\n');
  if (stages !== null) writeDoc(repo, 'stages.json', JSON.stringify(stages));
  return repo;
}

// ---- parsing ----

test('features.jsonl: list order by n, a malformed line skipped and counted, unknown fields ignored', () => {
  const source = [
    line({ ...feature(3, 'third', 'b'), extra: 'ignored' }),
    '',
    '{not json',
    line(feature(1, 'first', 'a')),
    line({ n: 2, id: 'Bad Id', stage: 'a', name: 'x' }),
    line({ n: 2, id: 'no-name', stage: 'a' }),
    '[1, 2]',
    line(feature(2, 'second', 'a', { old: 'weird', reuse: 'what', suggest: 'maybe', verdict: 'yes', app: 'side:', old_lines: -4 })),
    line(feature(9, 'first', 'a')),
  ].join('\n');
  const { features, skipped } = parseFeatures(source);
  assert.deepEqual(features.map((entry) => [entry.n, entry.id]), [[1, 'first'], [2, 'second'], [3, 'third']]);
  assert.equal(skipped, 5, 'bad json, bad id, no name, an array, a repeated id');
  assert.equal(Object.hasOwn(features[2], 'extra'), false);
  const second = features[1];
  assert.deepEqual([second.old, second.reuse, second.suggest, second.verdict, second.app, second.old_lines], [null, 'unknown', null, null, null, 0]);
});

test('stages: named ones in order, a stage a feature names but the file lacks is added, an unused one is left out', () => {
  const features = parseFeatures([feature(1, 'a1', 'a'), feature(2, 'z1', 'zed'), feature(3, 'b1', 'b')].map(line).join('\n')).features;
  const stages = parseStages(JSON.stringify([{ id: 'b', title: 'Bee' }, { id: 'unused', title: 'Unused' }, { id: 'a', title: 'Ay', line: 'first' }]), features);
  assert.deepEqual(stages.map((stage) => [stage.id, stage.title]), [['b', 'Bee'], ['a', 'Ay'], ['zed', 'zed']]);
  assert.deepEqual(parseStages('not json', features).map((stage) => stage.id), ['a', 'zed', 'b']);
  assert.deepEqual(parseStages(null, features).map((stage) => stage.id), ['a', 'zed', 'b']);
});

test('readFeatures: a missing file is null; a missing stages.json falls back to the features', () => {
  assert.equal(readFeatures(stackRepo({ features: null })), null);
  const list = readFeatures(stackRepo({ stages: null }));
  assert.equal(list.features.length, FEATURES.length);
  assert.deepEqual(list.stages.map((stage) => stage.title), ['shell', 'loop', 'living', 'extras']);
});

test('the real feature list parses with nothing skipped', () => {
  const list = readFeatures(path.resolve(import.meta.dirname, '../../..'));
  if (list === null) return;
  assert.equal(list.skipped, 0);
  assert.equal(list.stages[0].id, 'shell');
  assert.deepEqual(list.features.map((entry) => entry.n), list.features.map((_, index) => index + 1));
});

// ---- verdicts, totals, schematic ----

const parsed = () => parseFeatures(FEATURES.map(line).join('\n')).features;

test('current verdict: the store over the file, the newest row wins, a cleared row overrides the file', () => {
  const features = parsed();
  const rows = [
    { id: 4, feature_id: 'engine', verdict: 'drop', note: '', created_at: 't4' },
    { id: 5, feature_id: 'terminal', verdict: null, note: '', created_at: 't5' },
  ];
  const verdicts = currentVerdicts(features, rows);
  assert.deepEqual(verdicts.get('engine'), { verdict: 'drop', note: '', source: 'store', shown: 4, at: 't4' });
  assert.equal(verdicts.get('terminal').verdict, null, 'cleared in the store although the file says take');
  assert.equal(verdicts.get('create').source, 'file');
  assert.equal(currentVerdicts(features, []).get('terminal').verdict, 'take');
  // The store gives the newest row per feature; with history the newest wins.
  const store = new Store(tempDir());
  store.addVerdict('engine', { verdict: 'take', shown: 0 });
  store.addVerdict('engine', { verdict: 'later', shown: 1 });
  store.addVerdict('fork', { verdict: 'refine', note: 'smaller', shown: 0 });
  assert.deepEqual(
    [...currentVerdicts(features, store.currentVerdictRows())].filter(([, value]) => value.source === 'store').map(([id, value]) => [id, value.verdict, value.shown]),
    [['engine', 'later', 2], ['fork', 'refine', 3]],
  );
  store.close();
});

test('totals and per-stage counts', () => {
  const features = parsed();
  const verdicts = currentVerdicts(features, [
    { id: 1, feature_id: 'engine', verdict: 'take', note: '' },
    { id: 2, feature_id: 'create', verdict: 'refine', note: 'x' },
    { id: 3, feature_id: 'fork', verdict: 'later', note: '' },
    { id: 4, feature_id: 'tree', verdict: 'drop', note: '' },
  ]);
  const { all, stages } = stackTotals(features, verdicts);
  assert.deepEqual(all, { take: 2, refine: 1, later: 1, drop: 1, undecided: 3, total: 8 });
  assert.deepEqual(stages.get('shell'), { take: 2, refine: 1, later: 0, drop: 0, undecided: 0, total: 3 });
  assert.deepEqual(stages.get('loop'), { take: 0, refine: 0, later: 1, drop: 1, undecided: 1, total: 3 });
  assert.equal(nextUndecided(features, verdicts).id, 'changes');
  assert.equal(nextUndecided(features, verdicts, 6).id, 'secrets');
});

test('the schematic: parts of taken entries are drawn, the current entry is highlighted, nothing else', () => {
  const features = parsed();
  const verdicts = currentVerdicts(features, [
    { id: 1, feature_id: 'create', verdict: 'refine', note: 'x' },
    { id: 2, feature_id: 'fork', verdict: 'later', note: '' },
    { id: 3, feature_id: 'tree', verdict: 'drop', note: '' },
    { id: 4, feature_id: 'secrets', verdict: 'take', note: '' },
  ]);
  const drawing = schematicParts(features, verdicts, 'changes');
  const drawn = (list) => list.filter((part) => part.state).map((part) => [part.key ?? part.text, part.state]);
  assert.deepEqual(drawn(drawing.parts), [
    ['side:Secrets', 'taken'],
    ['button:New computer', 'taken'],
    ['button:Terminal', 'taken'],
    ['tab:Changes', 'current'],
  ]);
  assert.deepEqual(drawn(drawing.cli), [['silo create', 'taken'], ['ssh main.silo', 'taken'], ['silo secrets', 'taken']]);
  assert.equal(drawing.empty, false);
  const fork = drawing.parts.find((part) => part.key === 'button:Fork');
  assert.deepEqual([fork.kind, fork.label, fork.features, fork.taken, fork.state], ['button', 'Fork', ['fork'], false, null]);
  // A taken entry that is also current is drawn as current.
  assert.equal(schematicParts(features, verdicts, 'secrets').parts.find((part) => part.key === 'side:Secrets').state, 'current');
  // Nothing taken: empty, only the current entry's parts drawn.
  const none = schematicParts(features, currentVerdicts(features, [{ id: 9, feature_id: 'terminal', verdict: null, note: '' }]), 'engine');
  assert.equal(none.empty, true);
  assert.deepEqual(drawn(none.parts), [['window', 'current']]);
  assert.equal(appWords('button:Fork'), "a Fork button on a computer's page");
  assert.equal(appWords('side:GitHub'), 'a GitHub item in the sidebar');
  assert.equal(appWords('menubar'), 'the menu bar item');
  assert.equal(appWords(null), null);
});

// ---- the page ----

const EMPTY_CHANGES = { changes: [], state: 'nogit', head: null };
const page = (stack, items = []) => renderPage({ items, docs: { product: null, journeys: null, decisions: null }, changes: EMPTY_CHANGES, notes: [], stack }).toString();

test('every features.jsonl and stages.json field is escaped', () => {
  const evil = '<img src=x onerror=alert(1)>"\'&';
  const features = parseFeatures(
    line({ n: 1, id: 'evil', stage: 'shell', name: evil, what: evil, why: evil, cli: evil, app: `button:${evil}`, old: 'built', reuse: 'reuse', suggest: 'take' }),
  ).features;
  const stages = [{ id: 'shell', title: evil, line: evil }];
  const html = page({ features, stages, skipped: 0, rows: [{ id: 1, feature_id: 'evil', verdict: 'refine', note: evil, created_at: 't' }] });
  assert.doesNotMatch(html, /<img src=x/);
  assert.doesNotMatch(html, /"'&(?!amp;|#39;|quot;|lt;|gt;)/);
  const count = (needle) => html.split(needle).length - 1;
  // name, stage title, cli, app label (button text and words), note, what and why each show escaped.
  assert.ok(count('&lt;img src=x onerror=alert(1)&gt;') >= 8, `escaped ${count('&lt;img src=x onerror=alert(1)&gt;')} times`);
  assert.doesNotMatch(html, /onerror=alert\(1\)>/);
});

test('without JavaScript: a Stack tab, every entry readable in order, the controls inside their own wrapper', () => {
  const repo = stackRepo();
  const list = readFeatures(repo);
  const html = page({ ...list, rows: [] });
  assert.match(html, /<a class="tab" href="#stack" data-view="stack">Stack <span class="badge quiet">7<\/span><\/a>/);
  assert.ok(html.indexOf('data-view="for-you"') < html.indexOf('data-view="stack"') && html.indexOf('data-view="stack"') < html.indexOf('data-view="product"'));
  assert.match(html, /<section class="view" id="stack" data-region="stack"/);
  assert.doesNotMatch(html, /<script(?! (?:type="module" )?src="\/(?:boot|app)\.js"><\/script>)/, 'no inline script');
  assert.doesNotMatch(html, / on[a-z]+="/, 'no event handler attributes');
  const names = [...html.matchAll(/<h2>(Name [a-z-]+)<\/h2>/g)].map((match) => match[1]);
  assert.deepEqual(names, FEATURES.map((entry) => `Name ${entry.id}`));
  // The start is the first undecided entry; terminal is decided in the file.
  assert.match(html, /data-entry="engine" data-digest="[0-9a-f]{16}" data-stage="shell" data-decided="" data-start/);
  assert.match(html, /data-entry="terminal" data-digest="[0-9a-f]{16}" data-stage="shell" data-decided="take"/);
  // Each verdict button sits in the wrapper of its own entry, with what the page showed.
  for (const entry of FEATURES) {
    const card = html.slice(html.indexOf(`data-entry="${entry.id}"`));
    const controls = card.slice(card.indexOf('<div class="controls'), card.indexOf('</article>'));
    const digest = entryDigest(list.features.find((other) => other.id === entry.id));
    assert.match(controls, new RegExp(`data-controls data-feature="${entry.id}" data-shown="0" data-seen="${digest}"`));
    assert.equal((controls.match(/data-pick="(take|refine|later|drop)"/g) || []).length, 4);
    assert.equal((controls.match(/data-primary/g) || []).length, 1);
  }
  assert.match(html, /data-pick="refine" data-primary>Refine…/);
  // The stack: stages in order, with counts and one segment per entry; the cut line follows Living in it.
  assert.match(html, /data-stage-jump="shell"[\s\S]*?<span class="tier-count">1\/3<\/span>[\s\S]*?<i class="v-none"><\/i><i class="v-none"><\/i><i class="v-take"><\/i>/);
  assert.match(html, /data-stage-jump="living"[\s\S]*?<li class="cut"><span>Starter: you can use it daily<\/span><\/li>[\s\S]*?data-stage-jump="extras"/);
  assert.match(html, /<p class="totals">.*1 taken.*7 undecided/);
  // In the old code, suggestion and where it shows, in words.
  assert.match(html, /built, never proven<\/p>\s*<small>about 100 lines · carry over<\/small>/);
  assert.match(html, /A New computer button on a computer&#39;s page/);
  // For you links to it.
  assert.match(html, /<a class="mini stack-mini" href="#stack">[\s\S]*?7 entries without a verdict/);
});

test('no feature list: no Stack tab, no section, no link', () => {
  const html = page(null);
  assert.doesNotMatch(html, /data-view="stack"|data-region="stack"|stack-mini/);
});

test('a malformed line shows a small warning', () => {
  const list = parseFeatures(`${line(feature(1, 'a', 'shell'))}\n{oops`);
  const prep = prepareStack({ ...list, stages: parseStages(null, list.features), rows: [] });
  assert.match(stackView(prep).toString(), /<p class="stack-warn" role="note">1 line of features.jsonl could not be read and is not shown.<\/p>/);
});

// ---- the API ----

let store;
let server;
let port;
let repo;

before(async () => {
  repo = stackRepo();
  store = new Store(tempDir());
  server = createServer({ store, repoRoot: repo });
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  port = server.address().port;
});

after(async () => {
  server.closeAllConnections();
  await new Promise((resolve) => server.close(resolve));
  store.close();
});

// What the page sends as `seen` for an entry of the fixture list.
const SEEN = new Map(parsed().map((entry) => [entry.id, entryDigest(entry)]));
const verdict = (id, body, headers) => postJson(port, `/api/features/${id}/verdict`, { seen: SEEN.get(id) ?? '0000000000000000', ...body }, headers);
const error = (response) => JSON.parse(response.body.toString()).error;

test('the entry digest covers what the card shows: name, what, old, suggest, why', () => {
  const [base] = parsed();
  const digest = entryDigest(base);
  assert.match(digest, /^[0-9a-f]{16}$/);
  for (const [field, value] of [['name', 'Other'], ['what', 'Other'], ['old', 'idea'], ['suggest', 'drop'], ['why', 'Other']]) {
    assert.notEqual(entryDigest({ ...base, [field]: value }), digest, field);
  }
  assert.equal(entryDigest({ ...base, cli: 'other', app: 'menubar', verdict: 'drop', n: 40 }), digest, 'other fields do not count');
});

test('verdict API: validation, unknown id, refine needs a note, 409 on a newer row, clear', async () => {
  assert.equal((await verdict('engine', { verdict: 'yes', shown: 0 })).status, 400);
  assert.equal((await verdict('engine', { shown: 0 })).status, 400, 'verdict is required (null clears)');
  assert.equal((await verdict('engine', { verdict: 'take' })).status, 400, 'shown is required');
  assert.equal((await verdict('engine', { verdict: 'take', shown: -1 })).status, 400);
  assert.equal((await verdict('engine', { verdict: 'take', shown: 0, note: 7 })).status, 400);
  assert.equal((await verdict('engine', { verdict: 'take', shown: 0, seen: undefined })).status, 400, 'seen is required');
  assert.equal((await verdict('engine', { verdict: 'take', shown: 0, seen: 'xyz' })).status, 400);
  const unknown = await verdict('no-such-entry', { verdict: 'take', shown: 0 });
  assert.equal(unknown.status, 409);
  assert.match(error(unknown), /no-such-entry is no longer in/);
  assert.equal((await verdict('..%2F..', { verdict: 'take', shown: 0 })).status, 409);
  const otherText = await verdict('engine', { verdict: 'take', shown: 0, seen: SEEN.get('create') });
  assert.equal(otherText.status, 409, 'a digest of other text');
  assert.match(error(otherText), /engine changed since the page showed it/);
  const refine = await verdict('engine', { verdict: 'refine', note: '  ', shown: 0 });
  assert.equal(refine.status, 400);
  assert.match(error(refine), /refine needs a note/);
  assert.equal((await verdict('engine', { verdict: null, shown: 0 })).status, 400, 'nothing to clear');

  const first = await verdict('engine', { verdict: 'take', shown: 0 });
  assert.equal(first.status, 201);
  const firstId = JSON.parse(first.body.toString()).id;
  const stale = await verdict('engine', { verdict: 'drop', shown: 0 });
  assert.equal(stale.status, 409);
  const changed = await verdict('engine', { verdict: 'refine', note: 'Only the service part', shown: firstId });
  assert.equal(changed.status, 201);
  const changedId = JSON.parse(changed.body.toString()).id;
  assert.equal((await verdict('engine', { verdict: null, shown: changedId, note: 'why' })).status, 400, 'a clear carries no note');
  assert.equal((await verdict('engine', { verdict: null, shown: changedId })).status, 201);
  // The file says terminal is taken: clearing it is allowed once.
  assert.equal((await verdict('terminal', { verdict: null, shown: 0 })).status, 201);

  const html = (await request(port)).body.toString();
  assert.match(html, /data-entry="engine" data-digest="[0-9a-f]{16}" data-stage="shell" data-decided=""/);
  assert.match(html, /data-entry="terminal" data-digest="[0-9a-f]{16}" data-stage="shell" data-decided=""/);
  const rows = store.db.prepare("SELECT name, verdict, note FROM verdicts WHERE feature_id = 'engine' ORDER BY id").all();
  assert.deepEqual(rows.map((row) => [row.name, row.verdict, row.note]), [['Name engine', 'take', ''], ['Name engine', 'refine', 'Only the service part'], ['Name engine', null, '']]);
});

test('verdict API keeps the page rules: Host, Origin, cross-site, JSON only', async () => {
  assert.equal((await verdict('create', { verdict: 'take', shown: 0 }, { Origin: 'http://evil.example' })).status, 403);
  assert.equal((await verdict('create', { verdict: 'take', shown: 0 }, { 'Sec-Fetch-Site': 'cross-site' })).status, 403);
  assert.equal((await verdict('create', { verdict: 'take', shown: 0 }, { Host: 'evil.example' })).status, 403);
  const form = await request(port, { method: 'POST', url: '/api/features/create/verdict', headers: { 'Content-Type': 'text/plain' }, body: JSON.stringify({ verdict: 'take', shown: 0, seen: SEEN.get('create') }) });
  assert.equal(form.status, 400);
  assert.equal(store.db.prepare("SELECT count(*) AS n FROM verdicts WHERE feature_id = 'create'").get().n, 0);
});

test('a verdict on text the owner never saw, or on an entry the list no longer has, is refused; the history stays', async () => {
  const local = stackRepo();
  const s = new Store(tempDir());
  const srv = createServer({ store: s, repoRoot: local });
  await new Promise((resolve) => srv.listen(0, '127.0.0.1', resolve));
  const p = srv.address().port;
  try {
    const send = (id, body) => postJson(p, `/api/features/${id}/verdict`, { seen: SEEN.get(id), ...body });
    assert.equal((await send('fork', { verdict: 'later', shown: 0 })).status, 201);
    // The orchestrator rewrites the entry's reason: the page's digest is stale.
    writeDoc(local, 'features.jsonl', FEATURES.map((entry) => (entry.id === 'create' ? { ...entry, why: 'A new reason' } : entry)).map(line).join('\n'));
    assert.equal((await send('create', { verdict: 'take', shown: 0 })).status, 409);
    const fresh = entryDigest(readFeatures(local).features.find((entry) => entry.id === 'create'));
    assert.match((await request(p)).body.toString(), new RegExp(`data-feature="create" data-shown="0" data-seen="${fresh}"`));
    assert.equal((await postJson(p, '/api/features/create/verdict', { seen: fresh, verdict: 'take', shown: 0 })).status, 201);
    // A change outside what the card shows (here, cli) keeps the digest.
    writeDoc(local, 'features.jsonl', FEATURES.map((entry) => (entry.id === 'fork' ? { ...entry, cli: 'silo fork --from' } : entry)).map(line).join('\n'));
    assert.equal((await send('fork', { verdict: 'take', shown: 1 })).status, 201);
    writeDoc(local, 'features.jsonl', FEATURES.filter((entry) => entry.id !== 'fork').map(line).join('\n'));
    assert.equal((await send('fork', { verdict: 'drop', shown: 3 })).status, 409);
    assert.equal(s.currentVerdictRows().length, 2);
    fs.rmSync(path.join(local, 'docs/product/features.jsonl'));
    assert.doesNotMatch((await request(p)).body.toString(), /data-region="stack"/);
  } finally {
    srv.closeAllConnections();
    await new Promise((resolve) => srv.close(resolve));
    s.close();
  }
});

// ---- the rule on every path: a reference model ----

// Random verdicts, clears, stale sends and acks against a model of the rules:
// the newest row per feature is current (store over file), a send that did
// not show the newest row is refused, refine needs a note, a clear needs
// something to clear, and ack hides exactly the rows it covers.
test('model: verdicts, clears, stale sends and acks keep the current-verdict rule after every step', () => {
  let seed = 7;
  const random = (n) => {
    seed = (seed * 1103515245 + 12345) % 2 ** 31;
    return seed % n;
  };
  const features = parsed();
  const ids = features.map((entry) => entry.id);
  const fileVerdict = new Map(features.map((entry) => [entry.id, entry.verdict]));
  const s = new Store(tempDir());
  const model = { rows: [], next: 1 };
  const newest = (id) => model.rows.filter((row) => row.feature === id).at(-1) ?? null;
  for (let step = 0; step < 400; step += 1) {
    const id = ids[random(ids.length)];
    const op = random(10);
    if (op < 7) {
      const verdict = [null, 'take', 'refine', 'later', 'drop'][random(5)];
      // Mostly a clear without a note; sometimes with one, which is refused.
      const note = random(3) === 0 || (verdict === null && random(4)) ? '' : `note ${step}`;
      const top = newest(id)?.id ?? 0;
      const shown = random(5) === 0 ? Math.max(0, top - 1 - random(3)) : top;
      let expected = 'ok';
      if (verdict === 'refine' && !note) expected = 'ValidationError';
      else if (verdict === null && note) expected = 'ValidationError';
      else if (shown < top) expected = 'ConflictError';
      else if (verdict === null && (newest(id) ? newest(id).verdict === null : fileVerdict.get(id) === null)) expected = 'ValidationError';
      let got = 'ok';
      try {
        const rowId = s.addVerdict(id, { verdict, note, shown, hasFileVerdict: fileVerdict.get(id) !== null });
        assert.equal(rowId, model.next, `step ${step}: row ids are sequential`);
      } catch (error) {
        got = error.name;
      }
      assert.equal(got, expected, `step ${step}: ${id} ${verdict} shown ${shown}/${top}`);
      if (got === 'ok') model.rows.push({ id: model.next++, feature: id, verdict, acked: false });
    } else {
      const withRows = ids.filter((entry) => newest(entry));
      if (!withRows.length) continue;
      const target = withRows[random(withRows.length)];
      // The row the agent read: mostly the newest, sometimes an older one.
      const rows = model.rows.filter((row) => row.feature === target);
      const upTo = rows[Math.max(0, rows.length - 1 - random(3))].id;
      s.ackVerdicts([{ feature: target, upTo }]);
      for (const row of rows) if (row.id <= upTo) row.acked = true;
    }
    const current = currentVerdicts(features, s.currentVerdictRows());
    for (const entry of ids) {
      const top = newest(entry);
      assert.equal(current.get(entry).verdict, top ? top.verdict : fileVerdict.get(entry), `step ${step}: current verdict of ${entry}`);
      assert.equal(current.get(entry).shown, top?.id ?? 0);
    }
    const unread = s.verdictsToRead().map((row) => row.id);
    const expectedUnread = ids.map(newest).filter((row) => row && !row.acked).map((row) => row.id).sort((a, b) => a - b);
    assert.deepEqual(unread, expectedUnread, `step ${step}: unread verdicts`);
  }
  s.close();
});

// ---- the CLI ----

test('dashboard verdicts and verdicts ack', async () => {
  const home = tempDir();
  const repo = stackRepo();
  const env = { SILO_DASHBOARD_REPO: repo };
  const s = new Store(home);
  s.addVerdict('engine', { verdict: 'take', shown: 0, name: 'Name engine' });
  s.addVerdict('fork', { verdict: 'refine', note: 'Fork from a checkpoint only\nnot from a running one', shown: 0, name: 'Name fork' });
  s.addVerdict('gone-entry', { verdict: 'drop', shown: 0, name: 'An entry since removed' });
  s.close();
  // The list renames fork after the owner judged it: the verdict keeps the name judged.
  writeDoc(repo, 'features.jsonl', FEATURES.map((entry) => (entry.id === 'fork' ? { ...entry, name: 'Fork, renamed' } : entry)).map(line).join('\n'));

  const listed = await runCli(['verdicts', '--json'], { home, env });
  assert.equal(listed.code, 0, listed.stderr);
  const rows = JSON.parse(listed.stdout);
  assert.deepEqual(rows.map((row) => [row.id, row.ack, row.feature_id, row.name, row.in_list, row.verdict, row.acked_at]), [
    [1, 'engine@1', 'engine', 'Name engine', true, 'take', null],
    [2, 'fork@2', 'fork', 'Name fork', true, 'refine', null],
    [3, 'gone-entry@3', 'gone-entry', 'An entry since removed', false, 'drop', null],
  ]);
  assert.match(rows[1].note, /checkpoint only\nnot from/);
  const text = await runCli(['verdicts'], { home, env });
  assert.match(text.stdout, /^engine@1 · take "Name engine" · \S+$/m);
  assert.match(text.stdout, /fork@2 · refine "Name fork" · \S+\n {2}Fork from a checkpoint only\n {2}not from a running one/);
  assert.match(text.stdout, /gone-entry@3 · drop "An entry since removed" \(no longer in the feature list\)/);

  assert.equal((await runCli(['verdicts', 'ack'], { home, env })).code, 2);
  assert.equal((await runCli(['verdicts', 'frob'], { home, env })).code, 2);
  assert.equal((await runCli(['verdicts', 'ack', 'Bad Id@1'], { home, env })).code, 2);
  assert.equal((await runCli(['verdicts', 'ack', '--all', 'engine@1'], { home, env })).code, 2);
  const bare = await runCli(['verdicts', 'ack', 'engine'], { home, env });
  assert.equal(bare.code, 2, 'a bare feature id is a usage error');
  assert.match(bare.stderr, /"engine" is not FEATURE_ID@ID: acknowledge each verdict as 'dashboard verdicts' lists it/);
  const missing = await runCli(['verdicts', 'ack', 'engine@1', 'never-decided@9'], { home, env });
  assert.equal(missing.code, 1);
  assert.match(missing.stderr, /no verdict 9 on never-decided/);
  const wrongRow = await runCli(['verdicts', 'ack', 'fork@1'], { home, env });
  assert.equal(wrongRow.code, 1, "row 1 is engine's, not fork's");
  assert.equal(JSON.parse((await runCli(['verdicts', '--json'], { home, env })).stdout).length, 3, 'all or nothing');

  const acked = await runCli(['verdicts', 'ack', 'engine@1', 'fork@2', '--json'], { home, env });
  assert.equal(acked.code, 0, acked.stderr);
  assert.deepEqual(JSON.parse(acked.stdout), { acknowledged: ['engine', 'fork'] });
  assert.deepEqual(JSON.parse((await runCli(['verdicts', '--json'], { home, env })).stdout).map((row) => row.ack), ['gone-entry@3']);
  // The owner changes fork after the agent listed it: acknowledging what was read leaves the change unread.
  const later = new Store(home);
  later.addVerdict('fork', { verdict: 'take', shown: 2, name: 'Fork, renamed' });
  later.close();
  assert.equal((await runCli(['verdicts', 'ack', 'fork@2', 'gone-entry@3'], { home, env })).stdout.trim(), 'acknowledged fork, gone-entry');
  assert.deepEqual(JSON.parse((await runCli(['verdicts', '--json'], { home, env })).stdout).map((row) => [row.ack, row.name, row.verdict]), [['fork@4', 'Fork, renamed', 'take']]);
  assert.equal((await runCli(['verdicts', 'ack', 'fork@4'], { home, env })).code, 0);
  assert.equal((await runCli(['verdicts'], { home, env })).stdout.trim(), 'No new verdicts.');
  const all = JSON.parse((await runCli(['verdicts', '--all', '--json'], { home, env })).stdout);
  assert.equal(all.length, 3);
  assert.ok(all.every((row) => row.acked_at));
  // A cleared verdict is listed as cleared.
  const again = new Store(home);
  again.addVerdict('engine', { verdict: null, shown: 1, name: 'Name engine' });
  again.close();
  assert.match((await runCli(['verdicts'], { home, env })).stdout, /^engine@5 · cleared "Name engine"/);
  assert.match((await runCli(['--help'], { home })).stdout, /verdicts ack FEATURE_ID@ID/);

});
