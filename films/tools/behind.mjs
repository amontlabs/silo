#!/usr/bin/env node
// When a film is behind the plan: the one rule, used by the Workshop's build (website/src/workshop/data.ts) and by
// this command. Node only, no package: it reads the repository with git and plan/ from the disk.
//
// A film is behind on an entry it shows when what the Workshop prints of that entry is different now from what it was
// at the film's last commit that is not a merge. The two ends are compared, so a change made and undone since does not
// count. An entry is found by its id (its file's name) wherever its file is. A take alone changes nothing printed.
//
//   node tools/behind.mjs [<id>…]                run from films/: each film named, or every film
//   node tools/behind.mjs <id> --holds "<why>"   a film that is behind and still holds: the check goes in its checked.md
//
// It prints, for each film, each entry it is behind on and what changed in it, or that it is up to date.
// Exit status: 0 when every film named is up to date (or the check was written), 1 when one is behind, cannot be read,
// or is not behind for --holds, 2 on wrong use.
import { execFileSync } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { closeSync, constants, existsSync, lstatSync, openSync, readdirSync, readFileSync, renameSync, rmSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { wordId } from '../../tools/plan/model/index.js';

export const PLAN_DIR = 'plan';

/** @param {unknown} value @returns {string[]} */
const list = (value) => (Array.isArray(value) ? value.filter((item) => typeof item === 'string') : []);

/**
 * The order films/README.md gives the films in: the ids of its table "The films, in the order they are watched".
 * @param {string} readme
 * @returns {string[]}
 */
export function filmOrder(readme) {
  const at = readme.indexOf('## The films, in the order they are watched');
  if (at === -1) return [];
  const ids = [];
  for (const line of readme.slice(at).split('\n').slice(1)) {
    if (line.startsWith('## ')) break;
    const id = /^\|\s*`([a-z0-9-]+)`\s*\|/.exec(line)?.[1];
    if (id) ids.push(id);
  }
  return ids;
}

/**
 * The text of a statement, as an item of `sets` or of a list holds it: every field but `in`, in order. A take copies a
 * `sets` item into its list without `in`, so the same statement reads the same at both.
 * @param {Record<string, unknown>} item
 */
function statementText(item) {
  return JSON.stringify(Object.keys(item).filter((key) => key !== 'in').sort().map((key) => [key, item[key]]));
}

/**
 * The id of a statement item: a word by its word, as the lists name it (`wordId`); a principle or a property by its id.
 * @param {Record<string, unknown>} item
 * @returns {string | null}
 */
const statementId = (item) => (typeof item?.id === 'string' ? item.id : typeof item?.word === 'string' ? wordId(item.word) : null);

/** Whether a file of plan/ is one of its lists. @param {string} path */
const isListFile = (path) => /(?:^|\/)(?:words|principles|properties)\.json$/.test(path);

/**
 * The statements of some list files' texts, as texts by id.
 * @param {string[]} texts
 * @returns {Map<string, string>}
 */
function listTextsOf(texts) {
  const statements = new Map();
  for (const text of texts) {
    let items;
    try {
      items = JSON.parse(text);
    } catch {
      continue;
    }
    if (!Array.isArray(items)) continue;
    for (const item of items) {
      const id = statementId(item);
      if (id) statements.set(id, statementText(item));
    }
  }
  return statements;
}

/**
 * The statements an entry sets or added, each with its text: from its `sets` item, or for an id of `adds` from the
 * lists (null when they lack it).
 * @param {Record<string, unknown>} entry
 * @param {ReadonlyMap<string, string>} lists
 * @returns {Map<string, string | null>}
 */
function setOrAdded(entry, lists) {
  const statements = new Map();
  for (const id of list(entry.adds)) statements.set(id, lists.get(id) ?? null);
  if (Array.isArray(entry.sets)) {
    for (const item of entry.sets) {
      const id = statementId(item);
      if (id) statements.set(id, statementText(item));
    }
  }
  return statements;
}

/** A value as one line of the command's output. @param {unknown} value */
const shown = (value) => (value === null || value === undefined ? '(none)' : JSON.stringify(value));

/**
 * What changed, field by field, between two texts of one statement (`statementText`).
 * @param {string} id
 * @param {string} before
 * @param {string} after
 */
function statementChanges(id, before, after) {
  const was = Object.fromEntries(JSON.parse(before));
  const now = Object.fromEntries(JSON.parse(after));
  return [...new Set([...Object.keys(was), ...Object.keys(now)])].sort()
    .filter((key) => JSON.stringify(was[key]) !== JSON.stringify(now[key]))
    .map((key) => `statement ${id}, ${key}: was ${shown(was[key])}, now ${shown(now[key])}`);
}

/**
 * What the Workshop prints of an entry file that differs between two moments, one line each: its question or name, its
 * answer or what, the ids or the texts of the statements it sets or added, what it removes, what it keeps, its surfaces;
 * or that it was dropped, replaced or deleted. Empty when nothing printed differs. The texts of its statements are
 * compared wherever they live at each moment: in its `sets` before a take, in the plan's lists (`listsBefore`,
 * `listsAfter`) after it. A take alone changes none of these: it writes the ids of `sets` as `adds`, the items into the
 * lists, and deletes `sets` and `removes`. A file that did not exist before is new, not changed; one that no longer
 * exists has changed.
 * @param {string | null} before
 * @param {string | null} after
 * @param {ReadonlyMap<string, string>} [listsBefore]
 * @param {ReadonlyMap<string, string>} [listsAfter]
 * @returns {string[]}
 */
export function printedChanges(before, after, listsBefore = new Map(), listsAfter = new Map()) {
  if (before === null) return [];
  if (after === null) return ['its file was deleted'];
  let was;
  let now;
  try {
    was = JSON.parse(before);
    now = JSON.parse(after);
  } catch {
    return [];
  }
  const changes = [];
  if (now.verdict === 'drop' && was.verdict !== 'drop') changes.push('dropped');
  if (typeof now.replaced_by === 'string' && was.replaced_by !== now.replaced_by) changes.push(`replaced by ${now.replaced_by}`);
  const took = now.verdict === 'take' && was.verdict !== 'take';
  /** @type {[string, (entry: Record<string, unknown>) => unknown][]} */
  const fields = [
    ['name', (entry) => entry.name ?? null],
    ['asks', (entry) => entry.asks ?? null],
    ['what', (entry) => entry.what ?? null],
    ['answer', (entry) => entry.answer ?? null],
    ['removes', (entry) => [...list(entry.removes)].sort()],
    ['keeps', (entry) => [...list(entry.keeps)].sort()],
    ['surfaces', (entry) => entry.surfaces ?? null],
  ];
  for (const [key, of] of fields) {
    // A take deletes `removes`: what it removed is no longer printed, which is no change.
    if (key === 'removes' && took) continue;
    if (JSON.stringify(of(was)) !== JSON.stringify(of(now))) changes.push(`${key}: was ${shown(of(was))}, now ${shown(of(now))}`);
  }
  const a = setOrAdded(was, listsBefore);
  const b = setOrAdded(now, listsAfter);
  for (const id of a.keys()) if (!b.has(id)) changes.push(`statement ${id}: no longer set`);
  for (const id of b.keys()) if (!a.has(id)) changes.push(`statement ${id}: newly set`);
  for (const [id, text] of a) {
    const next = b.get(id);
    if (text !== null && next !== null && next !== undefined && next !== text) changes.push(...statementChanges(id, text, next));
  }
  return changes;
}

/**
 * Whether what the Workshop prints of an entry file differs between two moments (`printedChanges`).
 * @param {string | null} before
 * @param {string | null} after
 * @param {ReadonlyMap<string, string>} [listsBefore]
 * @param {ReadonlyMap<string, string>} [listsAfter]
 */
export const printedChanged = (before, after, listsBefore, listsAfter) => printedChanges(before, after, listsBefore, listsAfter).length > 0;

/** The id of the entry a file of plan/ holds, from its path (`<part>/decisions/<id>.json`, `decisions/<id>.json`, …). @param {string} path */
const entryIdOf = (path) => /(?:^|\/)(?:decisions|features)\/([^/]+)\.json$/.exec(path)?.[1] ?? null;

/** @typedef {{ entries: Map<string, string>, lists: ReadonlyMap<string, string> }} PlanAt The files of the entries asked for, by id, and the plan's lists, at one moment. */

/**
 * The files of the entries `ids` as they were at `commit`, each found by its id (its file's name) wherever it was, and
 * the plan's lists then; null when git cannot say.
 * @param {string} repository
 * @param {string} commit
 * @param {ReadonlySet<string>} ids
 * @returns {PlanAt | null}
 */
export function planAt(repository, commit, ids) {
  /** @param {string[]} args @param {string} [input] */
  const git = (args, input) => execFileSync('git', ['-C', repository, ...args], { input, maxBuffer: 512 * 1024 * 1024, timeout: 120_000, stdio: ['pipe', 'pipe', 'ignore'] });
  try {
    // By the file's name, which is the entry's id: two entries whose files hold the same bytes are still two.
    const oidOf = new Map();
    const listOids = [];
    for (const line of git(['ls-tree', '-r', '-z', commit, '--', PLAN_DIR]).toString('utf8').split('\0')) {
      const [meta, path] = line.split('\t');
      if (!path) continue;
      const oid = meta.split(' ')[2];
      const id = entryIdOf(path);
      if (id && ids.has(id)) oidOf.set(id, oid);
      else if (isListFile(path)) listOids.push(oid);
    }
    const oids = [...new Set([...oidOf.values(), ...listOids])];
    const blobs = new Map();
    if (oids.length) {
      const out = git(['cat-file', '--batch'], oids.map((oid) => `${oid}\n`).join(''));
      let at = 0;
      for (const oid of oids) {
        const end = out.indexOf(10, at);
        if (end === -1) break;
        const [, type, size] = out.subarray(at, end).toString().split(' ');
        if (type === 'missing') {
          at = end + 1;
          continue;
        }
        blobs.set(oid, out.subarray(end + 1, end + 1 + Number(size)).toString('utf8'));
        at = end + 1 + Number(size) + 1;
      }
    }
    const entries = new Map();
    for (const [id, oid] of oidOf) {
      const text = blobs.get(oid);
      if (text !== undefined) entries.set(id, text);
    }
    return { entries, lists: listTextsOf(listOids.flatMap((oid) => blobs.get(oid) ?? [])) };
  } catch {
    return null;
  }
}

/**
 * The files of the entries `ids` as plan/ holds them now, by id, wherever they are, and its lists.
 * @param {string} dir
 * @param {ReadonlySet<string>} ids
 * @returns {PlanAt}
 */
export function planNow(dir, ids) {
  const entries = new Map();
  const listFiles = [];
  /** @param {string} folder */
  const walk = (folder) => {
    for (const entry of readdirSync(folder, { withFileTypes: true })) {
      const path = join(folder, entry.name);
      if (entry.isDirectory()) walk(path);
      else {
        const id = entryIdOf(path);
        if (id && ids.has(id)) entries.set(id, readFileSync(path, 'utf8'));
        else if (isListFile(path)) listFiles.push(readFileSync(path, 'utf8'));
      }
    }
  };
  if (existsSync(dir)) walk(dir);
  return { entries, lists: listTextsOf(listFiles) };
}

/**
 * The last commit that is not a merge and touched a folder of the repository; null when git cannot say.
 * @param {string} repository
 * @param {string} folder
 * @returns {string | null}
 */
export function folderCommit(repository, folder) {
  try {
    return execFileSync('git', ['-C', repository, 'log', '-1', '--no-merges', '--format=%H', '--', folder], { encoding: 'utf8', timeout: 60_000, stdio: ['ignore', 'pipe', 'ignore'] }).trim() || null;
  } catch {
    return null;
  }
}

/**
 * @typedef {object} Sources Where the rule reads the two ends; each has the real repository's reading by default.
 * @property {(folder: string) => string | null} [lastCommit]
 * @property {(commit: string, ids: ReadonlySet<string>) => PlanAt | null} [atCommit]
 * @property {(ids: ReadonlySet<string>) => PlanAt} [now]
 */

/**
 * The entries a film shows that it is behind on, each with what changed in it, in the order of `shows`; null when it
 * cannot be told (a film git knows no commit of, or a commit git cannot read). An entry that did not exist at the
 * film's commit makes the film behind; an id that is an entry at neither end (a film also shows statements of the
 * lists) is not looked at.
 * @param {string} repository
 * @param {string} name the film's folder under films/
 * @param {string[]} shows
 * @param {Sources} [sources]
 * @returns {{ commit: string, behind: { id: string, changes: string[] }[] } | null}
 */
export function filmBehind(repository, name, shows, sources = {}) {
  const { lastCommit = (folder) => folderCommit(repository, folder), atCommit = (commit, ids) => planAt(repository, commit, ids), now = (ids) => planNow(join(repository, PLAN_DIR), ids) } = sources;
  const commit = lastCommit(`films/${name}`);
  if (!commit) return null;
  const ids = new Set(shows);
  const then = atCommit(commit, ids);
  if (!then) return null;
  const current = now(ids);
  const behind = [];
  for (const id of shows) {
    // A film shows statements too (a principle, a property, a word): an id that is an entry at neither end is none.
    if (!then.entries.has(id) && !current.entries.has(id)) continue;
    const changes = then.entries.has(id)
      ? printedChanges(then.entries.get(id) ?? null, current.entries.get(id) ?? null, then.lists, current.lists)
      : ['new since the film: it did not exist at the film\'s commit'];
    if (changes.length) behind.push({ id, changes });
  }
  return { commit, behind };
}

/** @typedef {{ id: string, title: string, line: string, shows: string[], behind: string[] }} FilmInfo */

/**
 * The real folders directly under films/ that hold a film.json, by name. A folder that cannot be searched (its
 * film.json cannot be looked for) is kept, so that reading its film.json fails and says why, never dropped silently.
 * @param {string} dir
 */
function filmFolders(dir) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir).sort().filter((name) => {
    if (name.startsWith('.') || !lstatSync(join(dir, name)).isDirectory()) return false;
    try {
      lstatSync(join(dir, name, 'film.json'));
      return true;
    } catch (error) {
      const code = /** @type {{ code?: string }} */ (error).code;
      return code !== 'ENOENT' && code !== 'ENOTDIR';
    }
  });
}

/**
 * The films of `repository`: each real folder directly under films/ that holds a film.json, in the order
 * films/README.md gives (one it does not name comes after, by id), each with the entries it is behind on
 * (`filmBehind`). A shallow history, or a film that cannot be told, says nothing is behind.
 * @param {string} repository
 * @param {boolean} shallow
 * @param {Sources['lastCommit']} [lastCommit]
 * @param {Sources['atCommit']} [atCommit]
 * @param {Sources['now']} [now]
 * @returns {FilmInfo[]}
 */
export function readFilms(repository, shallow, lastCommit, atCommit, now) {
  const dir = join(repository, 'films');
  const films = filmFolders(dir).map((name) => {
    const raw = JSON.parse(readFileSync(join(dir, name, 'film.json'), 'utf8'));
    const shows = list(raw.shows);
    const told = shallow ? null : filmBehind(repository, name, shows, { lastCommit, atCommit, now });
    return { id: name, title: typeof raw.title === 'string' ? raw.title : name, line: typeof raw.line === 'string' ? raw.line : '', shows, behind: told ? told.behind.map((item) => item.id) : [] };
  });
  const order = filmsInOrder(dir);
  return films.sort((a, b) => order.indexOf(a.id) - order.indexOf(b.id));
}

/**
 * The names of the films of films/ (`filmFolders`), in the order films/README.md gives; one it does not name comes
 * after, by id. Reads no film.json.
 * @param {string} dir films/
 * @returns {string[]}
 */
function filmsInOrder(dir) {
  const readme = join(dir, 'README.md');
  const order = existsSync(readme) ? filmOrder(readFileSync(readme, 'utf8')) : [];
  /** @param {string} id */
  const rank = (id) => (order.includes(id) ? order.indexOf(id) : order.length);
  return filmFolders(dir).sort((a, b) => rank(a) - rank(b) || a.localeCompare(b));
}

const USAGE = 'usage: node tools/behind.mjs [<film id>…]\n       node tools/behind.mjs <film id> --holds "<why the film still holds>"\n(run from films/)';

/**
 * The entry a check appends to a film's checked.md: the date, the entries the film is behind on, and why it still holds.
 * @param {string} date `YYYY-MM-DD`
 * @param {string[]} ids
 * @param {string} why
 */
export function checkEntry(date, ids, why) {
  return `## ${date}\n\nBehind on: ${ids.map((id) => `\`${id}\``).join(', ')}.\n\n${why.trim()}\n`;
}

/** The head of a new checked.md. @param {string} name */
const checkedHead = (name) => `# Checks of ${name}\n\nEach time this film was behind the plan and was found to still hold: the date, the entries of the plan it\nwas behind on, and why what the film says is still right. Written by \`node tools/behind.mjs ${name} --holds\`.\n`;

/**
 * What a film's checked.md holds, '' when there is none, or why it is refused. Only a plain file directly in the film's
 * plain folder is read: a folder that is a link, a checked.md that is a link of either kind (symbolic, or a hard link
 * shared with another name) or anything but a plain file is refused, so the command never reads or writes through one.
 * @param {string} dir films/
 * @param {string} name
 * @returns {string | { refused: string }}
 */
function readChecked(dir, name) {
  const folder = join(dir, name);
  const shown = `films/${name}/checked.md`;
  const of = lstatSync(folder);
  if (of.isSymbolicLink() || !of.isDirectory()) return { refused: `films/${name}: is not a plain folder, so ${shown} is not written. Fix: make the film a real folder directly under films/.` };
  const file = join(folder, 'checked.md');
  // Its kind is read without opening it: opening a named pipe, say, would block.
  let kind;
  try {
    kind = lstatSync(file);
  } catch (error) {
    const code = /** @type {{ code?: string }} */ (error).code;
    if (code === 'ENOENT') return '';
    return { refused: `${shown}: cannot be read (${code ?? String(error)}), so it is not written.` };
  }
  if (kind.isSymbolicLink()) return { refused: `${shown}: is a link, so it is not written. Fix: replace it with a plain file, or remove it.` };
  if (!kind.isFile()) return { refused: `${shown}: is not a plain file, so it is not written. Fix: remove it.` };
  if (kind.nlink > 1) return { refused: `${shown}: is a hard link, shared with another file, so it is not written. Fix: replace it with a copy of its own.` };
  let fd;
  try {
    fd = openSync(file, constants.O_RDONLY | constants.O_NOFOLLOW);
  } catch (error) {
    const code = /** @type {{ code?: string }} */ (error).code;
    return { refused: `${shown}: cannot be read (${code ?? String(error)}), so it is not written.` };
  }
  try {
    return readFileSync(fd, 'utf8');
  } finally {
    closeSync(fd);
  }
}

/**
 * Writes a film's checked.md: a new file made beside it, then renamed into its place, so the old name's target is
 * never written, whatever it is.
 * @param {string} dir films/
 * @param {string} name
 * @param {string} text
 * @param {string} [tempName] the name of the new file, random by default
 */
export function writeChecked(dir, name, text, tempName = `.checked.md.${randomBytes(12).toString('hex')}.tmp`) {
  // A link put in place of checked.md, of the film's folder or of films/ while this runs is not guarded against: the
  // command runs in the person's own checkout, and whoever can change it during the run can already write anywhere in it.
  const folder = join(dir, name);
  const temp = join(folder, tempName);
  // 'wx' fails on a file already there; only a file this run made is ever removed.
  writeFileSync(temp, text, { flag: 'wx' });
  try {
    renameSync(temp, join(folder, 'checked.md'));
  } catch (error) {
    rmSync(temp, { force: true });
    throw error;
  }
}

/** Whether a path is a symbolic link, one that leads nowhere included. @param {string} path */
function isLink(path) {
  try {
    return lstatSync(path).isSymbolicLink();
  } catch {
    return false;
  }
}

/** Today, `YYYY-MM-DD`, in local time. */
function today() {
  const now = new Date();
  return `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, '0')}-${String(now.getDate()).padStart(2, '0')}`;
}

/**
 * The command. Prints, for each film named (or every film), the entries it is behind on and what changed in each.
 * With `--holds "<why>"` and one film that is behind, appends a check to that film's checked.md (it commits nothing):
 * the commit that adds it touches the film's folder, so the film is up to date from that commit on.
 * @param {string[]} args
 * @param {string} [repository]
 * @param {string} [date]
 * @returns {number}
 */
export function main(args, repository = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..'), date = today()) {
  const at = args.indexOf('--holds');
  const holds = at !== -1;
  const why = holds ? args[at + 1] : undefined;
  const names = holds ? args.filter((_, i) => i !== at && i !== at + 1) : args;
  if (names.some((arg) => arg.startsWith('-')) || (holds && names.length !== 1)) {
    console.error(USAGE);
    return 2;
  }
  if (holds && (typeof why !== 'string' || why.startsWith('--') || why.trim() === '')) {
    console.error(`${names[0]}: --holds needs the reason the film still holds, quoting what the film says.\n${USAGE}`);
    return 2;
  }
  // No control character (CR, LF, tab, NEL…), no line or paragraph separator (U+2028, U+2029), no code fence.
  if (holds && (/[\p{Cc}\p{Zl}\p{Zp}]/u.test(why ?? '') || (why ?? '').includes('```'))) {
    console.error(`${names[0]}: the reason is one line of plain text: no line break or other control character, and no three backticks in a row. Write it as one or two sentences, and quote what the film says between double quotes.`);
    return 2;
  }
  const dir = join(repository, 'films');
  const known = filmFolders(dir);
  for (const name of names) {
    if (!known.includes(name)) {
      if (isLink(join(dir, name))) {
        console.error(`films/${name}: is a link, not a film, so nothing is read or written. Fix: make the film a real folder directly under films/.`);
        return 2;
      }
      console.error(`${name}: not a film: a film is a real folder directly under films/ that holds a film.json (${known.join(', ')}).`);
      return 2;
    }
  }
  let shallow = false;
  try {
    shallow = execFileSync('git', ['-C', repository, 'rev-parse', '--is-shallow-repository'], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim() === 'true';
  } catch {
    console.error('git cannot read this repository.');
    return 1;
  }
  if (shallow) {
    console.error('This clone is shallow: a film\'s commit may be missing. Fetch the whole history (git fetch --unshallow) first.');
    return 1;
  }
  const films = names.length ? names : filmsInOrder(dir);
  let status = 0;
  for (const name of films) {
    let shows;
    try {
      shows = list(JSON.parse(readFileSync(join(dir, name, 'film.json'), 'utf8')).shows);
    } catch (error) {
      console.log(`${name}: cannot be read: films/${name}/film.json: ${error instanceof Error ? error.message : String(error)}`);
      status = 1;
      continue;
    }
    const told = filmBehind(repository, name, shows);
    if (!told) {
      console.log(`${name}: cannot tell: no commit touches films/${name} yet, or git cannot read it.`);
      status = 1;
      continue;
    }
    const since = told.commit.slice(0, 10);
    if (holds) {
      if (!told.behind.length) {
        console.error(`${name}: up to date since ${since}: there is nothing to check, so nothing was written.`);
        return 1;
      }
      const before = readChecked(dir, name);
      if (typeof before !== 'string') {
        console.error(before.refused);
        return 1;
      }
      writeChecked(dir, name, `${(before || checkedHead(name)).replace(/\n*$/, '\n')}\n${checkEntry(date, told.behind.map((item) => item.id), why ?? '')}`);
      console.log(`${name}: the check is in films/${name}/checked.md. Commit it: the film is up to date from that commit on.`);
      return 0;
    }
    if (!told.behind.length) {
      console.log(`${name}: up to date: none of the ${shows.length} entries and statements it shows changed in what the Workshop prints since ${since}.`);
      continue;
    }
    status = 1;
    console.log(`${name}: behind the plan since ${since}, on ${told.behind.length} of the ${shows.length} it shows:`);
    for (const { id, changes } of told.behind) {
      console.log(`  ${id}`);
      for (const change of changes) console.log(`    ${change}`);
    }
  }
  return status;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.exitCode = main(process.argv.slice(2));
