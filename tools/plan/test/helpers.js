// Fixtures: small plans written to a temporary directory. Tests never read the real plan/.

import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import {
  canonicalDecision, canonicalFeature, canonicalOrder, canonicalPart, canonicalPrinciple, canonicalProperty, canonicalText, canonicalWord,
} from '../model/canonical.js';
import { loadDir } from '../read/index.js';

export const temp = (prefix = 'plan-test-') => mkdtempSync(join(process.env.ATELIER_TMP ?? tmpdir(), prefix));
export const cleanup = (dir) => rmSync(dir, { recursive: true, force: true });

/** The sample tree of the model, completed: two parts, every list, six decisions and one feature. */
export const SAMPLE_DIR = join(import.meta.dirname, 'fixtures', 'sample');

/** The sample plan, by path relative to `plan/`: JSON files parsed, the rest as text. */
export function basePlan() {
  /** @type {Record<string, unknown>} */
  const files = {};
  for (const [name, text] of loadDir(SAMPLE_DIR)) files[name] = name.endsWith('.json') ? JSON.parse(text) : text;
  return files;
}

const each = (fn) => (list) => (Array.isArray(list) ? list.map(fn) : list);

/** @param {string} name a path relative to `plan/` @returns {(value: any) => any} the canonical form of that file. */
export function canonicalFile(name) {
  if (name === 'parts.json') return each(canonicalPart);
  if (name === 'order.json') return canonicalOrder;
  if (name === 'words.json') return each(canonicalWord);
  if (name === 'principles.json') return each(canonicalPrinciple);
  if (/^(?:[^/]+\/)?properties\.json$/.test(name)) return each(canonicalProperty);
  if (/^(?:[^/]+\/)?decisions\/[^/]+\.json$/.test(name)) return canonicalDecision;
  if (/^[^/]+\/features\/[^/]+\.json$/.test(name)) return canonicalFeature;
  return (value) => value;
}

/** A fixture value as text: strings as they are, the rest in canonical form. */
function text(name, value) {
  return typeof value === 'string' ? value : canonicalText(canonicalFile(name)(value));
}

/**
 * Writes a plan into `<root>/plan`. Values that are strings are written as they are; others in canonical form.
 * @param {Record<string, unknown>} files @param {string} [root]
 */
export function writePlan(files, root = temp()) {
  const dir = join(root, 'plan');
  rmSync(dir, { recursive: true, force: true });
  mkdirSync(dir, { recursive: true });
  for (const [name, value] of Object.entries(files)) {
    if (value === undefined) continue;
    mkdirSync(join(dir, name, '..'), { recursive: true });
    writeFileSync(join(dir, name), text(name, value));
  }
  return root;
}

/** As a Map of texts, the way the reader gives files to the check. */
export function filesOf(files) {
  return new Map(Object.entries(files).filter(([, v]) => v !== undefined).map(([name, value]) => [name, text(name, value)]));
}

export function git(root, ...args) {
  return gitWith({}, root, ...args);
}

export function gitWith(env, root, ...args) {
  return execFileSync('git', ['-C', root, ...args], {
    env: { ...process.env, ...env, GIT_AUTHOR_NAME: 'Test', GIT_AUTHOR_EMAIL: 'test@example.invalid', GIT_COMMITTER_NAME: 'Test', GIT_COMMITTER_EMAIL: 'test@example.invalid', GIT_CONFIG_GLOBAL: '/dev/null', GIT_CONFIG_SYSTEM: '/dev/null' },
    stdio: ['ignore', 'pipe', 'pipe'],
  }).toString();
}

export function initRepo(root) {
  git(root, 'init', '-q', '-b', 'main');
  git(root, 'add', '-A');
  git(root, 'commit', '-q', '-m', 'Plan');
}

/** Runs the command in `cwd`; returns { code, out, err }. The command is loaded only when a test runs it. */
export async function run(cwd, argv, context = {}) {
  const { main } = await import('../cli/main.js');
  const out = [];
  const err = [];
  const code = await main(argv, { cwd, now: new Date('2026-11-20T12:00:00Z'), out: (t) => out.push(t), err: (t) => err.push(t), ...context });
  return { code, out: out.join('\n'), err: err.join('\n') };
}
