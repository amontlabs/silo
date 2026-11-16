import { test } from 'node:test';
import assert from 'node:assert/strict';
import path from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { SCHEMA_VERSION, Store } from '../src/store.js';
import { tempDir } from './helpers.js';

// The version 1 schema exactly as it shipped.
const V1 = `
CREATE TABLE items (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  kind TEXT NOT NULL CHECK (kind IN ('look', 'decide', 'shipped')),
  journey TEXT,
  title TEXT NOT NULL,
  body TEXT NOT NULL DEFAULT '',
  default_text TEXT,
  source TEXT,
  status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'closed')),
  created_at TEXT NOT NULL,
  closed_at TEXT,
  close_note TEXT
);
CREATE TABLE replies (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  item_id INTEGER NOT NULL REFERENCES items (id) ON DELETE CASCADE,
  author TEXT NOT NULL CHECK (author IN ('owner', 'agent')),
  verdict TEXT CHECK (verdict IS NULL OR verdict IN ('agree', 'disagree')),
  text TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  acked_at TEXT
);
CREATE INDEX replies_by_item ON replies (item_id);
CREATE TABLE attachments (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  item_id INTEGER NOT NULL REFERENCES items (id) ON DELETE CASCADE,
  file TEXT NOT NULL,
  media_type TEXT NOT NULL,
  caption TEXT,
  created_at TEXT NOT NULL
);
CREATE INDEX attachments_by_item ON attachments (item_id);
CREATE TABLE seen (
  key TEXT PRIMARY KEY,
  value TEXT NOT NULL
);
PRAGMA user_version = 1;
`;

function versionOneStore() {
  const home = tempDir();
  const db = new DatabaseSync(path.join(home, 'dashboard.db'));
  db.exec('PRAGMA journal_mode = WAL');
  db.exec(V1);
  const at = '2026-11-15T10:00:00.000Z';
  const item = db.prepare('INSERT INTO items (kind, journey, title, body, default_text, source, status, created_at, closed_at, close_note) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)');
  item.run('look', 'live', 'Sorted by use', 'Body **one**', 'Keep it', 'feat/x', 'open', at, null, null);
  item.run('decide', null, 'Pick a name', '', null, null, 'closed', at, at, 'Done');
  item.run('shipped', 'task', 'Build 3', 'Shipped', null, null, 'open', at, null, null);
  item.run('decide', null, 'Deleted later', '', null, null, 'open', at, null, null);
  db.prepare('DELETE FROM items WHERE id = 4').run();
  const reply = db.prepare('INSERT INTO replies (item_id, author, verdict, text, created_at, acked_at) VALUES (?, ?, ?, ?, ?, ?)');
  reply.run(1, 'agent', null, 'Updated', at, null);
  reply.run(1, 'owner', 'disagree', 'By name', at, at);
  reply.run(2, 'owner', null, 'Silo', at, null);
  db.prepare("INSERT INTO attachments (item_id, file, media_type, caption, created_at) VALUES (3, 'clip.mp4', 'video/mp4', 'Demo', ?)").run(at);
  db.prepare("INSERT INTO seen (key, value) VALUES ('docs_commit', 'abc'), ('item:3', '{\"at\":\"x\",\"after\":0}')").run();
  db.close();
  return home;
}

test('a version 1 store with data migrates in place: ids, replies, attachments and seen rows kept', () => {
  const home = versionOneStore();
  const store = new Store(home);
  try {
    assert.equal(store.db.prepare('PRAGMA user_version').get().user_version, SCHEMA_VERSION);
    assert.deepEqual(store.db.prepare('PRAGMA foreign_key_check').all(), []);
    const items = store.itemsWithState();
    assert.deepEqual(
      items.map((item) => [item.id, item.kind, item.journey, item.title, item.status, item.default_text, item.source, item.close_note, item.options, item.anchor]),
      [
        [1, 'look', 'live', 'Sorted by use', 'open', 'Keep it', 'feat/x', null, null, null],
        [2, 'decide', null, 'Pick a name', 'closed', null, null, 'Done', null, null],
        [3, 'shipped', 'task', 'Build 3', 'open', null, null, null, null, null],
      ],
    );
    assert.deepEqual(items[0].replies.map((reply) => [reply.id, reply.author, reply.verdict, reply.text, reply.acked_at !== null, reply.choice, reply.reaction]), [
      [1, 'agent', null, 'Updated', false, null, null],
      [2, 'owner', 'disagree', 'By name', true, null, null],
    ]);
    assert.deepEqual(items.map((item) => item.state), ['answered', 'closed', 'answered']);
    assert.deepEqual(items[2].attachments.map((file) => [file.file, file.caption]), [['clip.mp4', 'Demo']]);
    assert.equal(store.getSeen('docs_commit'), 'abc');
    // The deleted id 4 is not reused, and the new kinds and columns work.
    const next = store.createItem({ kind: 'decide', title: 'New', options: [{ label: 'A' }, { label: 'B' }] });
    assert.equal(next, 5);
    assert.equal(store.addReply(next, { author: 'owner', choice: 1 }) > 3, true);
    const note = store.addNote({ anchor: 'journeys#live/2', anchorText: 'Step', reaction: 'essential' });
    assert.equal(store.getItem(note.item).kind, 'note');
    // Cascades still hold after the rebuild.
    store.db.prepare('DELETE FROM items WHERE id = 1').run();
    assert.equal(store.db.prepare('SELECT count(*) AS n FROM replies WHERE item_id = 1').get().n, 0);
  } finally {
    store.close();
  }
  // Opening again changes nothing.
  const again = new Store(home);
  assert.equal(again.allItems().length, 4);
  again.close();
});

test('a failing migration leaves the version 1 store as it was', () => {
  const home = versionOneStore();
  const db = new DatabaseSync(path.join(home, 'dashboard.db'));
  // A column already named like a new one makes the ALTER fail mid-migration.
  db.exec('ALTER TABLE replies ADD COLUMN reaction TEXT');
  db.close();
  assert.throws(() => new Store(home), /duplicate column/);
  const check = new DatabaseSync(path.join(home, 'dashboard.db'));
  assert.equal(check.prepare('PRAGMA user_version').get().user_version, 1);
  assert.equal(check.prepare('SELECT count(*) AS n FROM items').get().n, 3);
  assert.equal(check.prepare('SELECT count(*) AS n FROM replies').get().n, 3);
  assert.equal(check.prepare("SELECT count(*) AS n FROM sqlite_master WHERE name = 'items_v2'").get().n, 0);
  assert.match(check.prepare("SELECT sql FROM sqlite_master WHERE name = 'items'").get().sql, /'shipped'\)\)/);
  check.close();
});

const hasVerdicts = (db) => db.prepare("SELECT count(*) AS n FROM sqlite_master WHERE type = 'table' AND name = 'verdicts'").get().n === 1;

test('a version 1 store goes straight to version 3: items kept, verdicts table added and usable', () => {
  const home = versionOneStore();
  const store = new Store(home);
  try {
    assert.equal(store.db.prepare('PRAGMA user_version').get().user_version, 3);
    assert.ok(hasVerdicts(store.db));
    assert.equal(store.allItems().length, 3);
    assert.equal(store.addVerdict('shell-a', { verdict: 'take', shown: 0 }), 1);
    assert.deepEqual(store.currentVerdictRows().map((row) => [row.feature_id, row.verdict]), [['shell-a', 'take']]);
  } finally {
    store.close();
  }
});

// A version 2 store is a version 3 store without the verdicts table.
function versionTwoStore() {
  const home = tempDir();
  const store = new Store(home);
  const item = store.createItem({ kind: 'decide', title: 'Pick', options: [{ label: 'A' }, { label: 'B' }] });
  store.addReply(item, { author: 'owner', choice: 1 });
  store.addNote({ anchor: 'journeys#live/start', anchorText: 'Start', reaction: 'nice' });
  store.setSeen('docs_commit', 'abc');
  store.db.exec('DROP TABLE verdicts; PRAGMA user_version = 2;');
  store.close();
  return home;
}

test('a version 2 store with data migrates to version 3 in place, and opening again changes nothing', () => {
  const home = versionTwoStore();
  const before = new DatabaseSync(path.join(home, 'dashboard.db'));
  assert.equal(hasVerdicts(before), false);
  const dump = (db) => JSON.stringify([db.prepare('SELECT * FROM items ORDER BY id').all(), db.prepare('SELECT * FROM replies ORDER BY id').all(), db.prepare('SELECT * FROM seen ORDER BY key').all()]);
  const was = dump(before);
  before.close();
  const store = new Store(home);
  try {
    assert.equal(store.db.prepare('PRAGMA user_version').get().user_version, 3);
    assert.ok(hasVerdicts(store.db));
    assert.equal(dump(store.db), was);
    store.addVerdict('shell-a', { verdict: 'later', shown: 0 });
  } finally {
    store.close();
  }
  const again = new Store(home);
  assert.equal(again.currentVerdictRows().length, 1);
  assert.equal(again.allItems().length, 2);
  again.close();
});

test('a newer store (version 4) is refused and left as it was', () => {
  const home = tempDir();
  const db = new DatabaseSync(path.join(home, 'dashboard.db'));
  db.exec('PRAGMA user_version = 4');
  db.close();
  assert.throws(() => new Store(home), /schema version 4/);
});
