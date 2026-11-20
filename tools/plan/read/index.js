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
import { readdirSync, readFileSync, statSync } from 'node:fs';
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

/**
 * Every file under a plan directory, by its path relative to it (`engine/features/fork.json`).
 * @param {string} dir @returns {Map<string, string>}
 */
export function loadDir(dir) {
  /** @type {Map<string, string>} */
  const files = new Map();
  const walk = (path, prefix) => {
    for (const name of readdirSync(path).sort()) {
      if (name.startsWith('.')) continue;
      const full = join(path, name);
      const rel = prefix ? `${prefix}/${name}` : name;
      if (statSync(full).isDirectory()) walk(full, rel);
      else files.set(rel, readFileSync(full, 'utf8'));
    }
  };
  walk(dir, '');
  return files;
}

/**
 * Every file under `plan/` at a git ref of a repository.
 * @param {string} repo a directory inside the repository @param {string} ref @param {string} [prefix] the plan's folder in the tree
 * @returns {Map<string, string>}
 */
export function loadRef(repo, ref, prefix = 'plan') {
  const inRepo = (dir, args, input) => execFileSync('git', ['-C', dir, ...args], { input, stdio: ['pipe', 'pipe', 'pipe'], maxBuffer: 256 * 1024 * 1024 });
  // Paths given to git are relative to the directory it runs in: run it at the top level, so any directory inside the repository works.
  const top = inRepo(repo, ['rev-parse', '--show-toplevel']).toString().trim();
  const git = (args, input) => inRepo(top, args, input);
  const commit = git(['rev-parse', '--verify', '--quiet', `${ref}^{commit}`]).toString().trim();
  const names = git(['ls-tree', '-r', '-z', '--name-only', commit, '--', `${prefix}/`]).toString().split('\0').filter((name) => name && !isHidden(name.slice(prefix.length + 1)));
  /** @type {Map<string, string>} */
  const files = new Map();
  if (!names.length) return files;
  const out = git(['cat-file', '--batch'], names.map((name) => `${commit}:${name}\n`).join(''));
  let at = 0;
  for (const name of names) {
    const end = out.indexOf(10, at);
    const header = out.subarray(at, end).toString();
    const size = Number(header.split(' ')[2]);
    files.set(name.slice(prefix.length + 1), out.subarray(end + 1, end + 1 + size).toString('utf8'));
    at = end + 1 + size + 1;
  }
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
    words: objects(words).map((word) => ({ ...word, id: wordId(word.word) })),
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
