// Reads `plan/`, from a directory or from a git ref, into one plan object.
//
//   plan = {
//     parts: [{ id, title, line }],
//     words: [{ word, means, not?, id }],
//     principles: [{ id, says, asks, known_as?, why }],
//     properties: [{ id, says, kind, from?, check, at?, part }],   part: an id, or null for the root
//     stages: [{ id, title, line, entries: [ids] }],
//     entries: { [id]: { id, kind, part, stage, position, notes?, ...the keys of its file } },
//   }
//
// position counts from 1 across the flattened order; notes is the text of <id>.md, absent without one.

import { execFileSync } from 'node:child_process';
import { lstatSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { wordId } from '../model/names.js';

/** The plan could not be read: a file is missing or is not JSON. */
export class PlanError extends Error {
  /** @param {{ file: string, field: string, message: string, fix: string }[]} problems */
  constructor(problems) {
    super(problems.map((p) => `${p.file}: ${p.field}: ${p.message}. Fix: ${p.fix}`).join('\n'));
    this.problems = problems;
  }
}

/** A path with a part whose name starts with a dot (`.DS_Store`, `features/.x.json`): not a file of the plan, ignored. @param {string} rel */
export const isHidden = (rel) => rel.split('/').some((part) => part.startsWith('.'));

/** A symbolic link under `plan/`: an error, never followed (it could lead outside `plan/`). @param {string} rel */
const linkProblem = (rel) => ({ file: `plan/${rel}`, field: '-', message: 'a symbolic link', fix: 'replace it with the file or folder itself' });

/** Bytes that are not valid UTF-8 are an error, never a replacement character; a BOM is kept as it is. */
const UTF8 = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true });

/** A file whose bytes are not valid UTF-8. @param {string} rel */
const encodingProblem = (rel) => ({ file: `plan/${rel}`, field: '-', message: 'not valid UTF-8', fix: 'save it as UTF-8' });

/**
 * The text of a file of the plan, decoded strictly: invalid bytes push a problem and give undefined.
 * @param {Uint8Array} bytes @param {string} rel @param {any[]} problems
 */
function decode(bytes, rel, problems) {
  try {
    return UTF8.decode(bytes);
  } catch {
    problems.push(encodingProblem(rel));
    return undefined;
  }
}

/**
 * Every file under a plan directory, by its path relative to it (`engine/features/fork.json`).
 * A symbolic link, the directory itself or anything under it, whatever its name, is never followed:
 * it throws a PlanError. A path with a dot-named part is looked at for links, never read. A file
 * that is not valid UTF-8 throws a PlanError too.
 * @param {string} dir @returns {Map<string, string>}
 */
export function loadDir(dir) {
  /** @type {Map<string, string>} */
  const files = new Map();
  if (lstatSync(dir).isSymbolicLink()) throw new PlanError([linkProblem('')]);
  /** @type {{ file: string, field: string, message: string, fix: string }[]} */
  const problems = [];
  const walk = (path, prefix) => {
    for (const name of readdirSync(path).sort()) {
      const full = join(path, name);
      const rel = prefix ? `${prefix}/${name}` : name;
      // Looked at before the name: a link is an error whatever it is called.
      const stat = lstatSync(full);
      if (stat.isSymbolicLink()) problems.push(linkProblem(rel));
      else if (stat.isDirectory()) walk(full, rel);
      else if (!isHidden(rel)) {
        const text = decode(readFileSync(full), rel, problems);
        if (text !== undefined) files.set(rel, text);
      }
    }
  };
  walk(dir, '');
  if (problems.length) throw new PlanError(problems);
  return files;
}

/**
 * Every file under `plan/` at a git ref of a repository. Names are read NUL-separated and the
 * blobs by their object id, so a path holding a newline is read like any other. A symbolic link
 * or a submodule under `plan/`, whatever its name, throws a PlanError, and so does a plan folder
 * that is itself a link or not a folder, and a file that is not valid UTF-8.
 * @param {string} repo a directory inside the repository @param {string} ref @param {string} [prefix] the plan's folder in the tree
 * @returns {Map<string, string>}
 */
export function loadRef(repo, ref, prefix = 'plan') {
  const inRepo = (dir, args, input) => execFileSync('git', ['-C', dir, ...args], { input, stdio: ['pipe', 'pipe', 'pipe'], maxBuffer: 256 * 1024 * 1024 });
  // Paths given to git are relative to the directory it runs in: run it at the top level, so any directory inside the repository works.
  const top = inRepo(repo, ['rev-parse', '--show-toplevel']).toString().trim();
  const git = (args, input) => inRepo(top, args, input);
  const commit = git(['rev-parse', '--verify', '--quiet', `${ref}^{commit}`]).toString().trim();
  // The plan folder and each folder above it must be a folder: through a link, `ls-tree -r` sees nothing and the plan would read empty.
  const parts = prefix.split('/');
  for (let depth = 1; depth <= parts.length; depth += 1) {
    const path = parts.slice(0, depth).join('/');
    const [record] = git(['ls-tree', '-z', commit, '--', path]).toString('utf8').split('\0').filter(Boolean);
    if (!record) break;
    const [mode, type] = record.slice(0, record.indexOf('\t')).split(' ');
    if (mode === '120000') throw new PlanError([{ file: `${path}/`, field: '-', message: 'a symbolic link', fix: 'replace it with the folder itself' }]);
    if (type !== 'tree') throw new PlanError([{ file: `${path}/`, field: '-', message: `a ${type}, not a folder`, fix: 'make it the folder of the plan' }]);
  }
  // Each record: `<mode> <type> <oid>\t<path>`.
  const all = git(['ls-tree', '-r', '-z', commit, '--', `${prefix}/`]).toString('utf8').split('\0').filter(Boolean).map((record) => {
    const tab = record.indexOf('\t');
    const [mode, type, oid] = record.slice(0, tab).split(' ');
    return { mode, type, oid, name: record.slice(tab + 1).slice(prefix.length + 1) };
  });
  // Looked at before the name: a link or a submodule is an error whatever it is called.
  const links = all.filter((record) => record.type !== 'blob' || record.mode === '120000');
  if (links.length) throw new PlanError(links.map((record) => (record.mode === '120000' ? linkProblem(record.name) : { file: `plan/${record.name}`, field: '-', message: `a ${record.type}, not a file`, fix: 'replace it with the files themselves' })));
  const records = all.filter((record) => !isHidden(record.name));
  /** @type {Map<string, string>} */
  const files = new Map();
  if (!records.length) return files;
  const out = git(['cat-file', '--batch'], records.map((record) => `${record.oid}\n`).join(''));
  /** @type {{ file: string, field: string, message: string, fix: string }[]} */
  const problems = [];
  let at = 0;
  for (const { name, oid } of records) {
    const end = out.indexOf(10, at);
    const [got, , sizeText] = out.subarray(at, end).toString().split(' ');
    const size = Number(sizeText);
    if (got !== oid || !Number.isInteger(size)) throw new Error(`git cat-file gave an unexpected answer for plan/${name}`);
    const text = decode(out.subarray(end + 1, end + 1 + size), name, problems);
    if (text !== undefined) files.set(name, text);
    at = end + 1 + size + 1;
  }
  if (problems.length) throw new PlanError(problems);
  return files;
}

/** @param {Map<string, string>} files @param {string} name @param {any[]} problems */
function json(files, name, problems) {
  const text = files.get(name);
  if (text === undefined) {
    problems.push({ file: `plan/${name}`, field: '-', message: 'missing', fix: `create plan/${name}` });
    return undefined;
  }
  try {
    return JSON.parse(text);
  } catch (error) {
    problems.push({ file: `plan/${name}`, field: '-', message: `not JSON (${/** @type {Error} */ (error).message})`, fix: 'write valid JSON' });
    return undefined;
  }
}

/** The lists that must exist, at the root of `plan/`. */
const REQUIRED_FILES = ['parts.json', 'order.json', 'words.json', 'principles.json'];

/** `decisions/<id>.json` at the root, `<part>/decisions/<id>.json` or `<part>/features/<id>.json` in a part. */
const ENTRY_FILE = /^(?:([^/]+)\/)?(decisions|features)\/([^/]+)\.json$/;
/** `properties.json` at the root or `<part>/properties.json`. */
const PROPERTIES_FILE = /^(?:([^/]+)\/)?properties\.json$/;

const isObject = (value) => value !== null && typeof value === 'object' && !Array.isArray(value);
const objects = (value) => (Array.isArray(value) ? value.filter(isObject) : []);

/**
 * Builds the plan object from the files, as far as they parse. Shape errors are the check's;
 * this reports only missing lists and invalid JSON, in `problems`. `json` holds every file it
 * read (the lists, the properties and the entries) that parsed, by its path relative to `plan/`,
 * as it is in the file.
 * @param {Map<string, string>} files
 */
export function parsePlan(files) {
  /** @type {{ file: string, field: string, message: string, fix: string }[]} */
  const problems = [];
  /** @type {Map<string, any>} */
  const parsed = new Map();
  const read = (name) => {
    const value = json(files, name, problems);
    if (value !== undefined) parsed.set(name, value);
    return value;
  };
  const [parts, order, words, principles] = REQUIRED_FILES.map(read);

  const stages = objects(order?.stages).map((stage) => ({ ...stage, entries: Array.isArray(stage.entries) ? stage.entries : [] }));
  /** @type {Map<string, { stage: string, position: number }>} */
  const placed = new Map();
  let position = 0;
  for (const stage of stages) {
    for (const id of stage.entries) {
      position += 1;
      if (typeof id === 'string' && !placed.has(id)) placed.set(id, { stage: stage.id, position });
    }
  }

  const properties = [];
  /** @type {Record<string, any>} */
  const entries = Object.create(null);
  // Sorted paths: the root's properties.json comes before every part's, and parts follow each other by name.
  const names = [...files.keys()].filter((name) => !isHidden(name)).sort((a, b) => (a.includes('/') ? 1 : 0) - (b.includes('/') ? 1 : 0) || (a < b ? -1 : a > b ? 1 : 0));
  for (const name of names) {
    const props = PROPERTIES_FILE.exec(name);
    if (props) {
      const list = read(name);
      for (const property of objects(list)) properties.push({ ...property, part: props[1] ?? null });
      continue;
    }
    const match = ENTRY_FILE.exec(name);
    if (!match || (match[1] === undefined && match[2] === 'features')) continue;
    const [, part = null, folder, id] = match;
    const data = read(name);
    if (data === undefined || Object.hasOwn(entries, id)) continue;
    const place = placed.get(id);
    /** @type {Record<string, any>} */
    const entry = { id, kind: folder === 'decisions' ? 'decision' : 'feature', part, stage: place?.stage ?? null, position: place?.position ?? null };
    for (const [key, value] of Object.entries(isObject(data) ? data : {})) if (!Object.hasOwn(entry, key)) entry[key] = value;
    const notes = files.get(name.replace(/\.json$/, '.md'));
    if (notes !== undefined) entry.notes = notes;
    entries[id] = entry;
  }

  const plan = {
    parts: objects(parts),
    words: objects(words).map((word) => ({ ...word, id: typeof word.word === 'string' ? wordId(word.word) : null })),
    principles: objects(principles),
    properties,
    stages,
    entries,
  };
  return { plan, problems, json: parsed };
}

/** @param {Map<string, string>} files */
function strict(files) {
  const { plan, problems } = parsePlan(files);
  if (problems.length) throw new PlanError(problems);
  return plan;
}

/** @param {string} dir the `plan/` directory */
export const readPlan = (dir) => strict(loadDir(dir));

/** @param {string} repo @param {string} ref @param {string} [prefix] */
export const readPlanAt = (repo, ref, prefix = 'plan') => strict(loadRef(repo, ref, prefix));
