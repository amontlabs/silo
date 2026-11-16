import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { fileURLToPath } from 'node:url';
import { StoreVersionError } from '../src/errors.js';
import { SCHEMA_VERSION, Store, homeDir, sanitizeFileName } from '../src/store.js';
import { renderPage } from '../src/views.js';
import { tempDir } from './helpers.js';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');

function withStore(fn) {
  const store = new Store(tempDir());
  try {
    return fn(store);
  } finally {
    store.close();
  }
}

test('a new store has the current schema version and WAL', () =>
  withStore((store) => {
    assert.equal(store.db.prepare('PRAGMA user_version').get().user_version, SCHEMA_VERSION);
    assert.equal(store.db.prepare('PRAGMA journal_mode').get().journal_mode, 'wal');
    const tables = store.db.prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name").all().map((row) => row.name);
    for (const name of ['attachments', 'items', 'replies', 'seen']) assert.ok(tables.includes(name), name);
  }));

test('a store with a newer schema is refused and left as it was', () => {
  const home = tempDir();
  const raw = new DatabaseSync(path.join(home, 'dashboard.db'));
  raw.exec(`CREATE TABLE future (x); PRAGMA user_version = ${SCHEMA_VERSION + 1}`);
  raw.close();
  assert.throws(() => new Store(home), (error) => error instanceof StoreVersionError && /schema version 2.*only knows version 1/.test(error.message));
  const again = new DatabaseSync(path.join(home, 'dashboard.db'));
  assert.equal(again.prepare('PRAGMA user_version').get().user_version, SCHEMA_VERSION + 1);
  assert.equal(again.prepare("SELECT count(*) AS n FROM sqlite_master WHERE name = 'items'").get().n, 0);
  again.close();
});

test('reopening a store keeps its data; two handles share one file', () => {
  const home = tempDir();
  const a = new Store(home);
  const b = new Store(home);
  const id = a.createItem({ kind: 'decide', title: 'Pick a name' });
  b.addReply(id, { author: 'owner', text: 'Silo' });
  assert.equal(a.getItem(id).replies.length, 1);
  a.close();
  b.close();
  const c = new Store(home);
  assert.equal(c.getItem(id).title, 'Pick a name');
  c.close();
});

test('the home folder defaults outside the repository and may not be inside it', () => {
  assert.match(homeDir({}), /\.local\/state\/silo-dashboard$/);
  assert.equal(homeDir({ SILO_DASHBOARD_HOME: '/x/y' }), '/x/y');
  assert.throws(() => new Store(path.join(REPO, 'tools', 'dashboard', 'data')), /inside the repository/);
  assert.throws(() => new Store(REPO), /inside the repository/);
  assert.equal(fs.existsSync(path.join(REPO, 'tools', 'dashboard', 'data')), false);
});

test('item validation', () =>
  withStore((store) => {
    assert.throws(() => store.createItem({ kind: 'look', title: 'No default' }), /needs a default/);
    assert.throws(() => store.createItem({ kind: 'later', title: 'x' }), /kind must be/);
    assert.throws(() => store.createItem({ kind: 'decide', title: '' }), /title is required/);
    assert.throws(() => store.createItem({ kind: 'decide', title: 'a\nb' }), /single line/);
    assert.throws(() => store.createItem({ kind: 'decide', title: 'x', journey: '../etc' }), /journey must be/);
    const id = store.createItem({ kind: 'look', title: 'Ok', defaultText: 'Keep it', journey: 'unknown-journey' });
    assert.equal(store.getItem(id).journey, 'unknown-journey');
  }));

test('replies: verdicts are for look items, from the owner; closed items take none', () =>
  withStore((store) => {
    const look = store.createItem({ kind: 'look', title: 'L', defaultText: 'd' });
    const decide = store.createItem({ kind: 'decide', title: 'D' });
    assert.throws(() => store.addReply(decide, { author: 'owner', verdict: 'agree' }), /only a look item/);
    assert.throws(() => store.addReply(look, { author: 'agent', verdict: 'agree' }), /only the owner/);
    assert.throws(() => store.addReply(look, { author: 'owner', verdict: 'disagree', text: '' }), /text is required/);
    assert.throws(() => store.addReply(decide, { author: 'owner', text: '  ' }), /text is required/);
    store.addReply(look, { author: 'owner', verdict: 'agree' });
    store.closeItem(decide, 'done');
    assert.throws(() => store.addReply(decide, { author: 'agent', text: 'late' }), /closed/);
    assert.throws(() => store.closeItem(decide), /already closed/);
    assert.throws(() => store.addReply(999, { author: 'agent', text: 'x' }), /no item 999/);
  }));

test('attachments: sanitised names, numeric suffix on collision, missing file refused', () =>
  withStore((store) => {
    const id = store.createItem({ kind: 'shipped', title: 'Build' });
    const source = tempDir();
    const file = path.join(source, 'shot one.png');
    fs.writeFileSync(file, 'png');
    const first = store.addAttachment(id, file, 'First');
    const second = store.addAttachment(id, file);
    assert.equal(first.file, 'shot_one.png');
    assert.equal(second.file, 'shot_one-2.png');
    assert.equal(first.media_type, 'image/png');
    assert.equal(fs.readFileSync(path.join(store.home, 'attachments', String(id), 'shot_one-2.png'), 'utf8'), 'png');
    assert.throws(() => store.addAttachment(id, path.join(source, 'missing.png')), /file not found/);
    assert.throws(() => store.addAttachment(id, source), /not a regular file/);
    assert.equal(store.attachmentPath({ item_id: id, file: '../../dashboard.db' }), null);
    assert.equal(sanitizeFileName('../../.evil name.sh'), 'evil_name.sh');
    assert.equal(sanitizeFileName('...'), 'file');
  }));

test('post is all or nothing, files included', () =>
  withStore((store) => {
    const source = tempDir();
    fs.writeFileSync(path.join(source, 'a.txt'), 'a');
    assert.throws(() => store.post({ kind: 'decide', title: 'X' }, [{ path: path.join(source, 'a.txt') }, { path: path.join(source, 'gone.txt') }]), /file not found/);
    assert.equal(store.allItems().length, 0);
    const folder = path.join(store.home, 'attachments', '1');
    assert.deepEqual(fs.existsSync(folder) ? fs.readdirSync(folder) : [], []);
  }));

test('ack is all or nothing and only takes owner replies', () =>
  withStore((store) => {
    const id = store.createItem({ kind: 'decide', title: 'D' });
    const agent = store.addReply(id, { author: 'agent', text: 'more' });
    const owner = store.addReply(id, { author: 'owner', text: 'answer' });
    assert.throws(() => store.ack([owner, 999]), /no reply 999/);
    assert.throws(() => store.ack([owner, agent]), /agent reply/);
    assert.equal(store.ownerReplies().length, 1);
    store.ack([owner]);
    assert.equal(store.ownerReplies().length, 0);
    assert.equal(store.ownerReplies({ all: true }).length, 1);
  }));

// ---- "For you" membership, checked against a reference model ----

function rng(seed) {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const KIND_RANK = { decide: 0, look: 1, shipped: 2 };

function forYouIds(html) {
  const region = /<section id="for-you"[\s\S]*?<\/section>/.exec(html)[0];
  return [...region.matchAll(/<article class="item [^"]*" id="item-(\d+)"/g)].map((match) => Number(match[1]));
}

test('model: an item is in "For you" until the owner answers, marks it seen or it closes; an agent reply brings it back', () => {
  for (const seed of [1, 2, 3, 4, 5, 6, 7, 8]) {
    withStore((store) => {
      const random = rng(seed);
      const pick = (list) => list[Math.floor(random() * list.length)];
      // model: id -> { kind, waiting, closed }
      const model = new Map();
      for (let step = 0; step < 120; step += 1) {
        const ids = [...model.keys()];
        const op = ids.length === 0 ? 'post' : pick(['post', 'agent', 'agent', 'owner', 'owner', 'agree', 'disagree', 'seen', 'close']);
        const id = ids.length ? pick(ids) : null;
        const entry = id === null ? null : model.get(id);
        const attempt = (fn, ok) => {
          if (ok) {
            fn();
            return true;
          }
          assert.throws(fn, `seed ${seed} step ${step} ${op} on ${id} should fail`);
          return false;
        };
        if (op === 'post') {
          const kind = pick(['look', 'decide', 'shipped']);
          const newId = store.createItem({ kind, title: `item ${step}`, defaultText: kind === 'look' ? 'default' : null });
          model.set(newId, { kind, waiting: true, closed: false });
        } else if (op === 'agent') {
          if (attempt(() => store.addReply(id, { author: 'agent', text: 'update' }), !entry.closed)) entry.waiting = true;
        } else if (op === 'owner') {
          if (attempt(() => store.addReply(id, { author: 'owner', text: 'my answer' }), !entry.closed)) entry.waiting = false;
        } else if (op === 'agree' || op === 'disagree') {
          const ok = !entry.closed && entry.kind === 'look';
          if (attempt(() => store.addReply(id, { author: 'owner', verdict: op, text: op === 'disagree' ? 'no' : '' }), ok)) entry.waiting = false;
        } else if (op === 'seen') {
          if (attempt(() => store.markShippedSeen(id), !entry.closed && entry.kind === 'shipped')) entry.waiting = false;
        } else if (op === 'close') {
          if (attempt(() => store.closeItem(id), !entry.closed)) {
            entry.closed = true;
            entry.waiting = false;
          }
        }

        const expected = [...model.entries()]
          .filter(([, value]) => value.waiting)
          .sort(([a, x], [b, y]) => KIND_RANK[x.kind] - KIND_RANK[y.kind] || b - a)
          .map(([key]) => key);
        const states = store.itemsWithState();
        const actual = states.filter((item) => item.state === 'waiting').map((item) => item.id);
        assert.deepEqual([...actual].sort((a, b) => a - b), [...expected].sort((a, b) => a - b), `seed ${seed} step ${step} (${op})`);
        for (const item of states) assert.equal(item.state === 'closed', model.get(item.id).closed);
        const html = renderPage({ items: states, docs: { product: null, journeys: null, decisions: null }, changes: { state: 'nogit', changes: [] } }).toString();
        assert.deepEqual(forYouIds(html), expected, `page order, seed ${seed} step ${step}`);
      }
    });
  }
});
