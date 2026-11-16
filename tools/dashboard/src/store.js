import { DatabaseSync } from 'node:sqlite';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { ConflictError, NotFoundError, StoreVersionError, ValidationError } from './errors.js';
import { itemState, seenValue } from './state.js';

export const SCHEMA_VERSION = 1;
export const KINDS = ['look', 'decide', 'shipped'];
export const VERDICTS = ['agree', 'disagree'];

const REPOSITORY = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const MAX_TITLE = 200;
const MAX_TEXT = 200_000;
const JOURNEY_ID = /^[a-z0-9][a-z0-9-]{0,79}$/;

const SCHEMA = `
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
`;

const MEDIA_TYPES = {
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.gif': 'image/gif',
  '.webp': 'image/webp',
  '.avif': 'image/avif',
  '.svg': 'image/svg+xml',
  '.mp4': 'video/mp4',
  '.m4v': 'video/mp4',
  '.mov': 'video/quicktime',
  '.webm': 'video/webm',
  '.mp3': 'audio/mpeg',
  '.m4a': 'audio/mp4',
  '.wav': 'audio/wav',
  '.ogg': 'audio/ogg',
  '.pdf': 'application/pdf',
  '.json': 'application/json',
  '.txt': 'text/plain',
  '.log': 'text/plain',
  '.md': 'text/markdown',
};

export function mediaTypeFor(fileName) {
  return MEDIA_TYPES[path.extname(fileName).toLowerCase()] || 'application/octet-stream';
}

export function homeDir(env = process.env) {
  const configured = env.SILO_DASHBOARD_HOME;
  return configured ? path.resolve(configured) : path.join(os.homedir(), '.local', 'state', 'silo-dashboard');
}

// A name that is one safe path segment: no separators, no leading dot.
export function sanitizeFileName(name) {
  let base = path.basename(String(name)).normalize('NFC');
  base = base.replace(/[^A-Za-z0-9._-]+/g, '_').replace(/^[._]+/, '');
  const ext = path.extname(base).slice(0, 12);
  let stem = base.slice(0, base.length - ext.length);
  stem = stem.slice(0, 80);
  return `${stem || 'file'}${ext}`;
}

function singleLine(value, label, max) {
  const text = String(value ?? '').trim();
  if (!text) throw new ValidationError(`${label} is required`);
  if (/[\r\n]/.test(text)) throw new ValidationError(`${label} must be a single line`);
  if (text.length > max) throw new ValidationError(`${label} is longer than ${max} characters`);
  return text;
}

function longText(value, label, { required = false } = {}) {
  const text = String(value ?? '').replace(/\r\n/g, '\n').trim();
  if (required && !text) throw new ValidationError(`${label} is required`);
  if (text.length > MAX_TEXT) throw new ValidationError(`${label} is longer than ${MAX_TEXT} characters`);
  return text;
}

export class Store {
  constructor(home = homeDir()) {
    this.home = path.resolve(home);
    const inside = path.relative(REPOSITORY, this.home);
    if (inside === '' || (!inside.startsWith('..') && !path.isAbsolute(inside))) {
      throw new ValidationError(`the dashboard home (${this.home}) is inside the repository; choose a folder outside it`);
    }
    fs.mkdirSync(this.home, { recursive: true, mode: 0o700 });
    this.attachmentsDir = path.join(this.home, 'attachments');
    this.db = new DatabaseSync(path.join(this.home, 'dashboard.db'));
    this.db.exec('PRAGMA busy_timeout = 10000');
    const found = this.db.prepare('PRAGMA user_version').get().user_version;
    if (found > SCHEMA_VERSION) {
      this.db.close();
      throw new StoreVersionError(found, SCHEMA_VERSION);
    }
    this.db.exec('PRAGMA journal_mode = WAL');
    this.db.exec('PRAGMA foreign_keys = ON');
    if (found < SCHEMA_VERSION) this.migrate(found);
  }

  migrate(from) {
    this.tx(() => {
      // Another process may have migrated while this one waited for the lock.
      const current = this.db.prepare('PRAGMA user_version').get().user_version;
      if (current >= SCHEMA_VERSION) return;
      if (from === 0 && current === 0) this.db.exec(SCHEMA);
      this.db.exec(`PRAGMA user_version = ${SCHEMA_VERSION}`);
    });
  }

  close() {
    if (this.db.isOpen) this.db.close();
  }

  tx(fn) {
    this.db.exec('BEGIN IMMEDIATE');
    try {
      const result = fn();
      this.db.exec('COMMIT');
      return result;
    } catch (error) {
      if (this.db.isTransaction) this.db.exec('ROLLBACK');
      throw error;
    }
  }

  now() {
    return new Date().toISOString();
  }

  createItem({ kind, journey = null, title, body = '', defaultText = null, source = null }) {
    if (!KINDS.includes(kind)) throw new ValidationError(`kind must be one of ${KINDS.join(', ')}`);
    const cleanTitle = singleLine(title, 'title', MAX_TITLE);
    if (journey != null && !JOURNEY_ID.test(journey)) {
      throw new ValidationError('journey must be a journey id: lowercase letters, digits and hyphens');
    }
    const cleanDefault = defaultText == null ? null : longText(defaultText, 'default');
    if (kind === 'look' && !cleanDefault) throw new ValidationError('a look item needs a default (what stands if the owner says nothing)');
    const cleanSource = source == null ? null : singleLine(source, 'source', MAX_TITLE);
    const info = this.db
      .prepare('INSERT INTO items (kind, journey, title, body, default_text, source, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)')
      .run(kind, journey, cleanTitle, longText(body, 'body'), cleanDefault, cleanSource, this.now());
    return Number(info.lastInsertRowid);
  }

  // Copies a file into the item's folder and records it. Returns the attachment row.
  addAttachment(itemId, sourcePath, caption = null) {
    const item = this.requireItem(itemId);
    let stat;
    try {
      stat = fs.statSync(sourcePath);
    } catch {
      throw new NotFoundError(`file not found: ${sourcePath}`);
    }
    if (!stat.isFile()) throw new ValidationError(`not a regular file: ${sourcePath}`);
    const folder = path.join(this.attachmentsDir, String(item.id));
    fs.mkdirSync(folder, { recursive: true, mode: 0o700 });
    const clean = sanitizeFileName(sourcePath);
    const ext = path.extname(clean);
    const stem = clean.slice(0, clean.length - ext.length);
    let stored = clean;
    for (let n = 1; ; n += 1) {
      const taken = this.db.prepare('SELECT 1 FROM attachments WHERE item_id = ? AND file = ?').get(item.id, stored);
      if (!taken) {
        try {
          fs.copyFileSync(sourcePath, path.join(folder, stored), fs.constants.COPYFILE_EXCL);
          break;
        } catch (error) {
          if (error.code !== 'EEXIST') throw error;
        }
      }
      stored = `${stem}-${n + 1}${ext}`;
    }
    const cleanCaption = caption ? singleLine(caption, 'caption', MAX_TITLE) : null;
    const info = this.db
      .prepare('INSERT INTO attachments (item_id, file, media_type, caption, created_at) VALUES (?, ?, ?, ?, ?)')
      .run(item.id, stored, mediaTypeFor(stored), cleanCaption, this.now());
    return { id: Number(info.lastInsertRowid), item_id: item.id, file: stored, media_type: mediaTypeFor(stored), caption: cleanCaption };
  }

  attachmentPath(attachment) {
    const folder = path.resolve(this.attachmentsDir, String(attachment.item_id));
    const full = path.resolve(folder, attachment.file);
    if (path.dirname(full) !== folder) return null;
    return full;
  }

  getAttachment(id) {
    return this.db.prepare('SELECT * FROM attachments WHERE id = ?').get(id) || null;
  }

  addReply(itemId, { author, text = '', verdict = null }) {
    const item = this.requireItem(itemId);
    if (item.status === 'closed') throw new ConflictError(`item ${item.id} is closed`);
    if (!['owner', 'agent'].includes(author)) throw new ValidationError('author must be owner or agent');
    if (verdict != null && (author !== 'owner' || !VERDICTS.includes(verdict))) {
      throw new ValidationError('a verdict is agree or disagree, and only the owner gives one');
    }
    if (verdict === 'agree' && item.kind !== 'look') throw new ValidationError('only a look item can be agreed with');
    if (verdict === 'disagree' && item.kind !== 'look') throw new ValidationError('only a look item can be disagreed with');
    const clean = longText(text, 'text', { required: verdict !== 'agree' });
    const info = this.db
      .prepare('INSERT INTO replies (item_id, author, verdict, text, created_at) VALUES (?, ?, ?, ?, ?)')
      .run(item.id, author, verdict, clean, this.now());
    return Number(info.lastInsertRowid);
  }

  markShippedSeen(itemId) {
    return this.tx(() => {
      const item = this.getItem(itemId);
      if (!item) throw new NotFoundError(`no item ${itemId}`);
      if (item.kind !== 'shipped') throw new ValidationError('only a shipped item is marked seen');
      if (item.status === 'closed') throw new ConflictError(`item ${item.id} is closed`);
      this.setSeen(`item:${item.id}`, seenValue(item, this.now()));
    });
  }

  closeItem(id, note = null) {
    const item = this.requireItem(id);
    if (item.status === 'closed') throw new ConflictError(`item ${item.id} is already closed`);
    const cleanNote = note ? longText(note, 'note') : null;
    this.db.prepare("UPDATE items SET status = 'closed', closed_at = ?, close_note = ? WHERE id = ?").run(this.now(), cleanNote, item.id);
  }

  requireItem(id) {
    const row = this.db.prepare('SELECT * FROM items WHERE id = ?').get(Number(id));
    if (!row) throw new NotFoundError(`no item ${id}`);
    return row;
  }

  getItem(id) {
    const row = this.db.prepare('SELECT * FROM items WHERE id = ?').get(Number(id));
    if (!row) return null;
    return {
      ...row,
      replies: this.db.prepare('SELECT * FROM replies WHERE item_id = ? ORDER BY id').all(row.id),
      attachments: this.db.prepare('SELECT * FROM attachments WHERE item_id = ? ORDER BY id').all(row.id),
    };
  }

  // Every item with its replies and attachments, oldest first.
  allItems() {
    const items = this.db.prepare('SELECT * FROM items ORDER BY id').all();
    const byId = new Map(items.map((item) => [item.id, { ...item, replies: [], attachments: [] }]));
    for (const reply of this.db.prepare('SELECT * FROM replies ORDER BY id').all()) byId.get(reply.item_id)?.replies.push(reply);
    for (const file of this.db.prepare('SELECT * FROM attachments ORDER BY id').all()) byId.get(file.item_id)?.attachments.push(file);
    return [...byId.values()];
  }

  // Items with their state: 'waiting' (in "For you"), 'answered' or 'closed'.
  itemsWithState() {
    const marks = new Map(this.db.prepare("SELECT key, value FROM seen WHERE key LIKE 'item:%'").all().map((row) => [row.key, row.value]));
    return this.allItems().map((item) => ({ ...item, state: itemState(item, marks.get(`item:${item.id}`)) }));
  }

  ownerReplies({ all = false } = {}) {
    const rows = this.db
      .prepare(
        `SELECT replies.id AS reply_id, replies.item_id, items.title, items.journey, items.kind, replies.verdict,
                replies.text, replies.created_at, replies.acked_at
         FROM replies JOIN items ON items.id = replies.item_id
         WHERE replies.author = 'owner' ${all ? '' : 'AND replies.acked_at IS NULL'}
         ORDER BY replies.id`,
      )
      .all();
    return rows;
  }

  // Marks owner replies acknowledged. All or nothing: an unknown id or an agent reply changes nothing.
  ack(ids) {
    return this.tx(() => {
      const when = this.now();
      for (const id of ids) {
        const row = this.db.prepare('SELECT author FROM replies WHERE id = ?').get(id);
        if (!row) throw new NotFoundError(`no reply ${id}`);
        if (row.author !== 'owner') throw new ConflictError(`reply ${id} is an agent reply; only owner replies are acknowledged`);
      }
      for (const id of ids) this.db.prepare('UPDATE replies SET acked_at = ? WHERE id = ? AND acked_at IS NULL').run(when, id);
      return ids;
    });
  }

  getSeen(key) {
    return this.db.prepare('SELECT value FROM seen WHERE key = ?').get(key)?.value ?? null;
  }

  setSeen(key, value) {
    this.db.prepare('INSERT INTO seen (key, value) VALUES (?, ?) ON CONFLICT (key) DO UPDATE SET value = excluded.value').run(key, value);
  }

  // Creates an item with its attachments, all or nothing (files included).
  post(fields, attachments = []) {
    const created = [];
    let id;
    try {
      this.tx(() => {
        id = this.createItem(fields);
        for (const attachment of attachments) created.push(this.addAttachment(id, attachment.path, attachment.caption));
      });
    } catch (error) {
      for (const file of created) fs.rmSync(this.attachmentPath(file), { force: true });
      throw error;
    }
    return id;
  }
}
