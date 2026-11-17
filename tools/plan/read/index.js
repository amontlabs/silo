// Reads `plan/`, from a directory or from a git ref, into one plan object.
//
//   plan = { product, areas, stages: [{ id, title, line, features: [ids] }], features: { [id]: feature }, decisions }
//   feature = the fields of its file, plus id, stage, position (from 1, in build order) and notes (its .md, or null)

import { execFileSync } from 'node:child_process';
import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { PLAN_FILES } from '../model/names.js';

/** The plan could not be read: a file is missing or is not JSON. */
export class PlanError extends Error {
  /** @param {{ file: string, field: string, message: string, fix: string }[]} problems */
  constructor(problems) {
    super(problems.map((p) => `${p.file}: ${p.field}: ${p.message}. Fix: ${p.fix}`).join('\n'));
    this.problems = problems;
  }
}

/**
 * Every file under a plan directory, by its path relative to it (`features/fork.json`).
 * @param {string} dir @returns {Map<string, string>}
 */
export function loadDir(dir) {
  /** @type {Map<string, string>} */
  const files = new Map();
  const walk = (path, prefix) => {
    for (const name of readdirSync(path).sort()) {
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
  const git = (args, input) => execFileSync('git', ['-C', repo, ...args], { input, stdio: ['pipe', 'pipe', 'pipe'], maxBuffer: 256 * 1024 * 1024 });
  const commit = git(['rev-parse', '--verify', '--quiet', `${ref}^{commit}`]).toString().trim();
  const names = git(['ls-tree', '-r', '-z', '--name-only', commit, '--', `${prefix}/`]).toString().split('\0').filter(Boolean);
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

const FEATURE_FILE = /^features\/([^/]+)\.json$/;

/**
 * Builds the plan object from the files, as far as they parse. Shape errors are the check's;
 * this reports only missing files and invalid JSON, in `problems`.
 * @param {Map<string, string>} files
 */
export function parsePlan(files) {
  /** @type {{ file: string, field: string, message: string, fix: string }[]} */
  const problems = [];
  const areas = json(files, PLAN_FILES.areas, problems);
  const order = json(files, PLAN_FILES.order, problems);
  const decisions = json(files, PLAN_FILES.decisions, problems);
  if (!files.has(PLAN_FILES.product)) problems.push({ file: 'plan/PRODUCT.md', field: '-', message: 'missing', fix: 'create plan/PRODUCT.md' });

  const stages = Array.isArray(order?.stages) ? order.stages.filter((stage) => stage && typeof stage === 'object') : [];
  /** @type {Map<string, { stage: string, position: number }>} */
  const placed = new Map();
  let position = 0;
  for (const stage of stages) {
    for (const id of Array.isArray(stage.features) ? stage.features : []) {
      position += 1;
      if (typeof id === 'string' && !placed.has(id)) placed.set(id, { stage: stage.id, position });
    }
  }

  /** @type {Record<string, any>} */
  const features = Object.create(null);
  /** @type {Record<string, any>} */
  const raw = Object.create(null);
  for (const name of files.keys()) {
    const match = FEATURE_FILE.exec(name);
    if (!match) continue;
    const data = json(files, name, problems);
    if (data === undefined) continue;
    raw[match[1]] = data;
    const place = placed.get(match[1]);
    const fields = data && typeof data === 'object' && !Array.isArray(data) ? data : {};
    features[match[1]] = { ...fields, id: match[1], stage: place?.stage ?? null, position: place?.position ?? null, notes: files.get(`features/${match[1]}.md`) ?? null };
  }

  const plan = {
    product: files.get(PLAN_FILES.product) ?? '',
    areas: Array.isArray(areas) ? areas : [],
    stages: stages.map((stage) => ({ ...stage, features: Array.isArray(stage.features) ? stage.features : [] })),
    features,
    decisions: Array.isArray(decisions) ? decisions : [],
  };
  return { plan, problems, raw: { areas, order, decisions, features: raw } };
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
