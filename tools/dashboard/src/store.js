import { DatabaseSync } from 'node:sqlite';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { ConflictError, DashboardError, NotFoundError, StoreVersionError, ValidationError } from './errors.js';
import { currentReaction, itemState, seenValue } from './state.js';
import { ID_SHAPE } from './docs.js';

export const SCHEMA_VERSION = 2;
export const KINDS = ['look', 'decide', 'shipped', 'note'];
// Kinds an agent posts. A note is created by the owner, from the page.
export const POST_KINDS = ['look', 'decide', 'shipped'];
export const VERDICTS = ['agree', 'disagree'];
export const REACTIONS = ['essential', 'nice', 'unneeded'];
export const MAX_OPTIONS = 6;
const MAX_OPTION_LABEL = 80;
const MAX_OPTION_DETAIL = 300;

const REPOSITORY = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../..');
const MAX_TITLE = 200;
const MAX_TEXT = 200_000;

// Where an owner note points: a journey step, a journey, or a product section.
// Returns { kind, journey, step, section } or throws.
export function parseAnchor(anchor) {
  const text = typeof anchor === 'string' ? anchor : '';
  let match = /^journeys#([^/]+)\/([^/]+)$/.exec(text);
  if (match && ID_SHAPE.test(match[1]) && ID_SHAPE.test(match[2])) return { kind: 'step', journey: match[1], step: match[2] };
  match = /^journeys#([^/]+)$/.exec(text);
  if (match && ID_SHAPE.test(match[1])) return { kind: 'journey', journey: match[1] };
  match = /^product#(.+)$/.exec(text);
  if (match && ID_SHAPE.test(match[1])) return { kind: 'section', section: match[1] };
  throw new ValidationError('anchor must be journeys#<journey id>/<step key>, journeys#<journey id> or product#<section key>');
}

const ITEMS_COLUMNS = `  kind TEXT NOT NULL CHECK (kind IN ('look', 'decide', 'shipped', 'note')),
  journey TEXT,
  title TEXT NOT NULL,
  body TEXT NOT NULL DEFAULT '',
  default_text TEXT,
  source TEXT,
  status TEXT NOT NULL DEFAULT 'open' CHECK (status IN ('open', 'closed')),
  created_at TEXT NOT NULL,
  closed_at TEXT,
  close_note TEXT,
  options TEXT,
  anchor TEXT,
  anchor_text TEXT`;
const CHOICE_COLUMN = 'choice INTEGER';
const REACTION_COLUMN = "reaction TEXT CHECK (reaction IS NULL OR reaction IN ('essential', 'nice', 'unneeded'))";

const SCHEMA = `
CREATE TABLE items (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
${ITEMS_COLUMNS}
);
CREATE INDEX items_by_anchor ON items (anchor);
CREATE TABLE replies (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  item_id INTEGER NOT NULL REFERENCES items (id) ON DELETE CASCADE,
  author TEXT NOT NULL CHECK (author IN ('owner', 'agent')),
  verdict TEXT CHECK (verdict IS NULL OR verdict IN ('agree', 'disagree')),
  text TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  acked_at TEXT,
  ${CHOICE_COLUMN},
  ${REACTION_COLUMN}
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

// [{ label, detail, recommended }] for a decide item, or null.
export function cleanOptionList(options, kind) {
  if (options == null || (Array.isArray(options) && options.length === 0)) return null;
  if (kind !== 'decide') throw new ValidationError('only a decide item takes options');
  if (!Array.isArray(options)) throw new ValidationError('options must be a list');
  if (options.length < 2) throw new ValidationError('a decide item with options needs at least 2');
  if (options.length > MAX_OPTIONS) throw new ValidationError(`a decide item takes at most ${MAX_OPTIONS} options`);
  const clean = options.map((option, index) => {
    const label = singleLine(option?.label, `option ${index + 1} label`, MAX_OPTION_LABEL);
    const detail = option?.detail == null ? '' : String(option.detail).replace(/\r\n/g, '\n').trim();
    if (detail.length > MAX_OPTION_DETAIL) throw new ValidationError(`option ${index + 1} detail is longer than ${MAX_OPTION_DETAIL} characters`);
    return { label, detail: detail || null, recommended: option?.recommended === true };
  });
  if (clean.filter((option) => option.recommended).length > 1) throw new ValidationError('at most one option is recommended');
  return clean;
}

export function parseOptions(value) {
  if (!value) return null;
  try {
    const list = JSON.parse(value);
    return Array.isArray(list) ? list : null;
  } catch {
    return null;
  }
}

function choiceLabel(options, choice) {
  return choice == null ? null : (options?.[choice]?.label ?? null);
}

function decorate(item) {
  const options = parseOptions(item.options);
  return {
    ...item,
    options,
    replies: item.replies.map((reply) => ({ ...reply, choice_label: choiceLabel(options, reply.choice) })),
  };
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
    this.restrictAccess();
    this.db.exec('PRAGMA busy_timeout = 10000');
    const found = this.db.prepare('PRAGMA user_version').get().user_version;
    if (found > SCHEMA_VERSION) {
      this.db.close();
      throw new StoreVersionError(found, SCHEMA_VERSION);
    }
    this.db.exec('PRAGMA journal_mode = WAL');
    // Foreign keys are off while migrating: rebuilding the items table drops
    // the old one, which must not cascade to replies and attachments. node:sqlite
    // turns them on by default, so this is explicit.
    if (found < SCHEMA_VERSION) {
      this.db.exec('PRAGMA foreign_keys = OFF');
      this.migrate();
    }
    this.db.exec('PRAGMA foreign_keys = ON');
    this.restrictAccess();
  }

  // Only the owner's account reads the store: the home folder and attachment
  // folders 0700, the database files and attachments 0600, including what an
  // earlier version or another umask left behind. SQLite gives its -wal and
  // -shm files the database file's mode.
  restrictAccess() {
    const chmod = (file, mode) => {
      try {
        fs.chmodSync(file, mode);
      } catch (error) {
        if (error.code !== 'ENOENT') throw error;
      }
    };
    chmod(this.home, 0o700);
    for (const name of ['dashboard.db', 'dashboard.db-wal', 'dashboard.db-shm']) chmod(path.join(this.home, name), 0o600);
    let folders = [];
    try {
      chmod(this.attachmentsDir, 0o700);
      folders = fs.readdirSync(this.attachmentsDir, { withFileTypes: true });
    } catch (error) {
      if (error.code !== 'ENOENT') throw error;
    }
    for (const folder of folders) {
      const dir = path.join(this.attachmentsDir, folder.name);
      if (!folder.isDirectory()) {
        if (folder.isFile()) chmod(dir, 0o600);
        continue;
      }
      chmod(dir, 0o700);
      for (const file of fs.readdirSync(dir, { withFileTypes: true })) if (file.isFile()) chmod(path.join(dir, file.name), 0o600);
    }
  }

  // One transaction from whatever version the file has to SCHEMA_VERSION.
  migrate() {
    this.tx(() => {
      // Another process may have migrated while this one waited for the lock.
      const current = this.db.prepare('PRAGMA user_version').get().user_version;
      if (current >= SCHEMA_VERSION) return;
      if (current === 0) this.db.exec(SCHEMA);
      if (current === 1) this.migrateFrom1();
      this.db.exec(`PRAGMA user_version = ${SCHEMA_VERSION}`);
    });
  }

  // Version 1 to 2: items gain options, anchor and anchor_text, and 'note'
  // joins the kinds. SQLite cannot change a CHECK, so the items table is
  // rebuilt with the same ids; replies gain choice and reaction.
  migrateFrom1() {
    const sequence = this.db.prepare("SELECT seq FROM sqlite_sequence WHERE name = 'items'").get()?.seq ?? null;
    this.db.exec(`
      CREATE TABLE items_v2 (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
      ${ITEMS_COLUMNS}
      );
      INSERT INTO items_v2 (id, kind, journey, title, body, default_text, source, status, created_at, closed_at, close_note)
        SELECT id, kind, journey, title, body, default_text, source, status, created_at, closed_at, close_note FROM items;
      DROP TABLE items;
      ALTER TABLE items_v2 RENAME TO items;
      CREATE INDEX items_by_anchor ON items (anchor);
      ALTER TABLE replies ADD COLUMN ${CHOICE_COLUMN};
      ALTER TABLE replies ADD COLUMN ${REACTION_COLUMN};
    `);
    if (sequence !== null) {
      this.db.prepare("UPDATE sqlite_sequence SET seq = max(seq, ?) WHERE name = 'items'").run(sequence);
      if (!this.db.prepare("SELECT 1 FROM sqlite_sequence WHERE name = 'items'").get()) {
        this.db.prepare("INSERT INTO sqlite_sequence (name, seq) VALUES ('items', ?)").run(sequence);
      }
    }
    const broken = this.db.prepare('PRAGMA foreign_key_check').all();
    if (broken.length) throw new DashboardError(`migration left ${broken.length} dangling reference(s); the database was not changed`);
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

  createItem({ kind, journey = null, title, body = '', defaultText = null, source = null, options = null }) {
    if (!POST_KINDS.includes(kind)) throw new ValidationError(`kind must be one of ${POST_KINDS.join(', ')}`);
    const cleanTitle = singleLine(title, 'title', MAX_TITLE);
    const cleanOptions = cleanOptionList(options, kind);
    if (journey != null && !ID_SHAPE.test(journey)) {
      throw new ValidationError('journey must be a journey id: lowercase letters, digits and hyphens');
    }
    const cleanDefault = defaultText == null ? null : longText(defaultText, 'default');
    if (kind === 'look' && !cleanDefault) throw new ValidationError('a look item needs a default (what stands if the owner says nothing)');
    const cleanSource = source == null ? null : singleLine(source, 'source', MAX_TITLE);
    const info = this.db
      .prepare('INSERT INTO items (kind, journey, title, body, default_text, source, options, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)')
      .run(kind, journey, cleanTitle, longText(body, 'body'), cleanDefault, cleanSource, cleanOptions && JSON.stringify(cleanOptions), this.now());
    return Number(info.lastInsertRowid);
  }

  // Copies a file into the item's folder and records it. Returns the attachment
  // row. Everything is checked before the copy; a failed insert removes it.
  addAttachment(itemId, sourcePath, caption = null) {
    const item = this.requireItem(itemId);
    const cleanCaption = caption ? singleLine(caption, 'caption', MAX_TITLE) : null;
    let stat;
    try {
      stat = fs.statSync(sourcePath);
    } catch {
      throw new NotFoundError(`file not found: ${sourcePath}`);
    }
    if (!stat.isFile()) throw new ValidationError(`not a regular file: ${sourcePath}`);
    const folder = path.join(this.attachmentsDir, String(item.id));
    fs.mkdirSync(this.attachmentsDir, { recursive: true, mode: 0o700 });
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
    const copy = path.join(folder, stored);
    try {
      fs.chmodSync(copy, 0o600);
      const info = this.db
        .prepare('INSERT INTO attachments (item_id, file, media_type, caption, created_at) VALUES (?, ?, ?, ?, ?)')
        .run(item.id, stored, mediaTypeFor(stored), cleanCaption, this.now());
      return { id: Number(info.lastInsertRowid), item_id: item.id, file: stored, media_type: mediaTypeFor(stored), caption: cleanCaption };
    } catch (error) {
      fs.rmSync(copy, { force: true });
      throw error;
    }
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

  // `after`, when given, is the newest reply id the caller has seen: a newer
  // reply on the item refuses the action (ConflictError).
  // `choice` picks one of a decide item's options (0-based); the reply's text
  // is then the option's label.
  addReply(itemId, { author, text = '', verdict = null, choice = null, after = null }) {
    if (!['owner', 'agent'].includes(author)) throw new ValidationError('author must be owner or agent');
    return this.tx(() => {
      const item = this.requireItem(itemId);
      if (item.status === 'closed') throw new ConflictError(`item ${item.id} is closed`);
      this.requireUnchanged(item.id, after);
      if (verdict != null && (author !== 'owner' || !VERDICTS.includes(verdict))) {
        throw new ValidationError('a verdict is agree or disagree, and only the owner gives one');
      }
      if (verdict != null && item.kind !== 'look') throw new ValidationError(`only a look item can be ${verdict === 'agree' ? 'agreed' : 'disagreed'} with`);
      let clean;
      if (choice != null) {
        if (author !== 'owner') throw new ValidationError('only the owner picks an option');
        if (verdict != null) throw new ValidationError('a reply picks an option or gives a verdict, not both');
        const options = parseOptions(item.options);
        if (item.kind !== 'decide' || !options) throw new ValidationError('only a decide item with options takes a choice');
        if (!Number.isSafeInteger(choice) || choice < 0 || choice >= options.length) {
          throw new ValidationError(`choice must be an option number from 0 to ${options.length - 1}`);
        }
        clean = options[choice].label;
      } else {
        clean = longText(text, 'text', { required: verdict !== 'agree' });
      }
      const info = this.db
        .prepare('INSERT INTO replies (item_id, author, verdict, text, choice, created_at) VALUES (?, ?, ?, ?, ?, ?)')
        .run(item.id, author, verdict, clean, choice, this.now());
      return Number(info.lastInsertRowid);
    });
  }

  // An owner note on a journey step, a journey or a product section. One open
  // note item per anchor: a later note or reaction adds a reply to it.
  // `reaction` undefined leaves the reaction as it is; null clears it.
  // `after` is the newest reply id of the anchor's open note the page showed
  // (0 when it showed none). Returns { item, reply }.
  addNote({ anchor, anchorText, journey = null, text = '', reaction, after = null }) {
    const where = parseAnchor(anchor);
    const cleanText = longText(text, 'text');
    const cleanAnchorText = singleLine(anchorText, 'anchorText', MAX_TITLE);
    if (where.journey && journey != null && journey !== where.journey) throw new ValidationError('journey does not match the anchor');
    const noteJourney = where.journey ?? null;
    if (journey != null && !where.journey) throw new ValidationError('a product note has no journey');
    if (reaction !== undefined && reaction !== null && !REACTIONS.includes(reaction)) {
      throw new ValidationError(`reaction must be one of ${REACTIONS.join(', ')}, or null to clear it`);
    }
    if (reaction !== undefined && where.kind !== 'step') throw new ValidationError('only a journey step takes a reaction');
    if (reaction === undefined && !cleanText) throw new ValidationError('send text or a reaction');
    if (reaction === null && cleanText) throw new ValidationError('clearing a reaction carries no text');
    return this.tx(() => {
      let item = this.db.prepare("SELECT * FROM items WHERE kind = 'note' AND anchor = ? AND status = 'open' ORDER BY id DESC LIMIT 1").get(anchor);
      if (after != null) {
        const newest = item ? (this.db.prepare('SELECT max(id) AS id FROM replies WHERE item_id = ?').get(item.id).id ?? 0) : 0;
        if (newest > after) throw new ConflictError(`the note on ${anchor} has a newer reply than the one shown; read it again`);
      }
      if (reaction === null) {
        const replies = this.db
          .prepare("SELECT replies.* FROM replies JOIN items ON items.id = replies.item_id WHERE items.kind = 'note' AND items.anchor = ? ORDER BY replies.id")
          .all(anchor);
        if (!currentReaction(replies)) throw new ValidationError('there is no reaction to clear');
      }
      if (!item) {
        const info = this.db
          .prepare("INSERT INTO items (kind, journey, title, anchor, anchor_text, created_at) VALUES ('note', ?, ?, ?, ?, ?)")
          .run(noteJourney, cleanAnchorText, anchor, cleanAnchorText, this.now());
        item = this.requireItem(Number(info.lastInsertRowid));
      }
      const info = this.db
        .prepare("INSERT INTO replies (item_id, author, text, reaction, created_at) VALUES (?, 'owner', ?, ?, ?)")
        .run(item.id, cleanText, reaction ?? null, this.now());
      return { item: item.id, reply: Number(info.lastInsertRowid) };
    });
  }

  requireUnchanged(itemId, after) {
    if (after == null) return;
    const newest = this.db.prepare('SELECT max(id) AS id FROM replies WHERE item_id = ?').get(itemId).id ?? 0;
    if (newest > after) throw new ConflictError(`item ${itemId} has a newer reply than the one shown; read it again`);
  }

  markShippedSeen(itemId, { after = null } = {}) {
    return this.tx(() => {
      const item = this.getItem(itemId);
      if (!item) throw new NotFoundError(`no item ${itemId}`);
      if (item.kind !== 'shipped' && item.kind !== 'note') throw new ValidationError('only a shipped item or a note is marked seen');
      if (item.status === 'closed') throw new ConflictError(`item ${item.id} is closed`);
      this.requireUnchanged(item.id, after);
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
    return decorate({
      ...row,
      replies: this.db.prepare('SELECT * FROM replies WHERE item_id = ? ORDER BY id').all(row.id),
      attachments: this.db.prepare('SELECT * FROM attachments WHERE item_id = ? ORDER BY id').all(row.id),
    });
  }

  // Every item with its replies and attachments, oldest first. `options` is
  // parsed; a reply that picked an option carries its label as choice_label.
  allItems() {
    const items = this.db.prepare('SELECT * FROM items ORDER BY id').all();
    const byId = new Map(items.map((item) => [item.id, { ...item, replies: [], attachments: [] }]));
    for (const reply of this.db.prepare('SELECT * FROM replies ORDER BY id').all()) byId.get(reply.item_id)?.replies.push(reply);
    for (const file of this.db.prepare('SELECT * FROM attachments ORDER BY id').all()) byId.get(file.item_id)?.attachments.push(file);
    return [...byId.values()].map(decorate);
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
                replies.choice, items.options, items.anchor, items.anchor_text, replies.reaction,
                replies.text, replies.created_at, replies.acked_at
         FROM replies JOIN items ON items.id = replies.item_id
         WHERE replies.author = 'owner' ${all ? '' : 'AND replies.acked_at IS NULL'}
         ORDER BY replies.id`,
      )
      .all();
    return rows.map(({ options, ...row }) => ({ ...row, choice_label: choiceLabel(parseOptions(options), row.choice) }));
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
