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

test('a caption that fails leaves no file behind and does not shift later names', () =>
  withStore((store) => {
    const id = store.createItem({ kind: 'shipped', title: 'Build' });
    const source = tempDir();
    fs.writeFileSync(path.join(source, 'clip.mp4'), 'mp4');
    assert.throws(() => store.addAttachment(id, path.join(source, 'clip.mp4'), 'two\nlines'), /single line/);
    const folder = path.join(store.home, 'attachments', String(id));
    assert.deepEqual(fs.existsSync(folder) ? fs.readdirSync(folder) : [], []);
    assert.equal(store.addAttachment(id, path.join(source, 'clip.mp4')).file, 'clip.mp4');
    // A failed insert removes its copy.
    store.db.exec('CREATE TRIGGER refuse BEFORE INSERT ON attachments BEGIN SELECT RAISE(ABORT, \'refused\'); END');
    assert.throws(() => store.addAttachment(id, path.join(source, 'clip.mp4')), /refused/);
    assert.deepEqual(fs.readdirSync(folder), ['clip.mp4']);
  }));

test('replies and Seen are refused when the item has a newer reply than the caller saw', () => {
  const home = tempDir();
  const page = new Store(home);
  const agent = new Store(home);
  try {
    const look = page.createItem({ kind: 'look', title: 'L', defaultText: 'd' });
    const shipped = page.createItem({ kind: 'shipped', title: 'S' });
    const newer = agent.addReply(look, { author: 'agent', text: 'update' });
    assert.throws(() => page.addReply(look, { author: 'owner', verdict: 'agree', after: 0 }), (error) => error.status === 409 && /newer reply/.test(error.message));
    assert.equal(page.getItem(look).replies.length, 1);
    page.addReply(look, { author: 'owner', verdict: 'agree', after: newer });
    const update = agent.addReply(shipped, { author: 'agent', text: 'more' });
    assert.throws(() => page.markShippedSeen(shipped, { after: 0 }), /newer reply/);
    page.markShippedSeen(shipped, { after: update });
    // Another handle closed the item: the reply is refused inside the same transaction.
    agent.closeItem(look);
    assert.throws(() => page.addReply(look, { author: 'owner', text: 'late', after: 99 }), /closed/);
  } finally {
    page.close();
    agent.close();
  }
});

test('the store is readable only by its owner, also when it existed with wider modes', () => {
  const home = tempDir();
  fs.chmodSync(home, 0o755);
  new Store(home).close();
  const folder = path.join(home, 'attachments', '1');
  fs.mkdirSync(folder, { recursive: true, mode: 0o755 });
  fs.writeFileSync(path.join(folder, 'old.png'), 'x', { mode: 0o644 });
  fs.chmodSync(path.join(home, 'dashboard.db'), 0o644);
  fs.chmodSync(path.join(home, 'attachments'), 0o755);
  const store = new Store(home);
  try {
    const mode = (file) => fs.statSync(file).mode & 0o777;
    assert.equal(mode(home), 0o700);
    assert.equal(mode(path.join(home, 'attachments')), 0o700);
    assert.equal(mode(folder), 0o700);
    assert.equal(mode(path.join(folder, 'old.png')), 0o600);
    for (const name of ['dashboard.db', 'dashboard.db-wal', 'dashboard.db-shm']) {
      if (fs.existsSync(path.join(home, name))) assert.equal(mode(path.join(home, name)), 0o600, name);
    }
    const id = store.createItem({ kind: 'shipped', title: 'S' });
    const source = path.join(tempDir(), 'new.png');
    fs.writeFileSync(source, 'y', { mode: 0o644 });
    const stored = store.addAttachment(id, source);
    assert.equal(mode(store.attachmentPath(stored)), 0o600);
    assert.equal(mode(path.join(home, 'dashboard.db-wal')), 0o600);
  } finally {
    store.close();
  }
});

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
      // model: id -> { kind, waiting, closed, last (newest reply id) }
      const model = new Map();
      for (let step = 0; step < 120; step += 1) {
        const ids = [...model.keys()];
        const op = ids.length === 0 ? 'post' : pick(['post', 'agent', 'agent', 'owner', 'owner', 'agree', 'disagree', 'seen', 'close', 'shown']);
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
          model.set(newId, { kind, waiting: true, closed: false, last: 0 });
        } else if (op === 'agent') {
          let reply;
          if (attempt(() => (reply = store.addReply(id, { author: 'agent', text: 'update' })), !entry.closed)) {
            entry.waiting = true;
            entry.last = reply;
          }
        } else if (op === 'owner') {
          let reply;
          if (attempt(() => (reply = store.addReply(id, { author: 'owner', text: 'my answer' })), !entry.closed)) {
            entry.waiting = false;
            entry.last = reply;
          }
        } else if (op === 'agree' || op === 'disagree') {
          const ok = !entry.closed && entry.kind === 'look';
          let reply;
          if (attempt(() => (reply = store.addReply(id, { author: 'owner', verdict: op, text: op === 'disagree' ? 'no' : '' })), ok)) {
            entry.waiting = false;
            entry.last = reply;
          }
        } else if (op === 'shown') {
          // An owner answer sent from a page that showed an older or the current reply.
          const after = entry.last > 0 && random() < 0.5 ? entry.last - 1 : entry.last;
          let reply;
          if (attempt(() => (reply = store.addReply(id, { author: 'owner', text: 'from the page', after })), !entry.closed && after >= entry.last)) {
            entry.waiting = false;
            entry.last = reply;
          }
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
