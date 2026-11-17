// The checkout the command works in: its root, its plan files, git.

import { execFileSync } from 'node:child_process';
import { existsSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { canonicalText } from '../model/canonical.js';
import { loadDir, loadRef, parsePlan } from '../read/index.js';
import { Refused } from './args.js';

/** @param {string} cwd @returns {string} the nearest directory, from cwd up, that holds `plan/order.json`. */
export function findRoot(cwd) {
  let dir = resolve(cwd);
  for (;;) {
    if (existsSync(join(dir, 'plan', 'order.json'))) return dir;
    const up = dirname(dir);
    if (up === dir) throw new Refused(`no plan/ found in ${cwd} or above it`);
    dir = up;
  }
}

/** @param {string} root @param {string[]} args @returns {string | null} git's output, or null when git fails. */
export function git(root, args) {
  try {
    return execFileSync('git', ['-C', root, ...args], { stdio: ['ignore', 'pipe', 'ignore'], maxBuffer: 64 * 1024 * 1024 }).toString();
  } catch {
    return null;
  }
}

/** @param {string} root */
export function loadPlan(root) {
  const files = loadDir(join(root, 'plan'));
  return { files, ...parsePlan(files) };
}

/** @param {string} root @param {string} rel a path inside `plan/` @param {unknown} value */
export function writeJson(root, rel, value) {
  writeFileSync(join(root, 'plan', rel), canonicalText(value));
}

/**
 * The area ids of the merged plan (`origin/main`, else `HEAD`), to tell a new area; null when neither has one.
 * @param {string} root @returns {string[] | null}
 */
export function knownAreas(root) {
  for (const ref of ['origin/main', 'HEAD']) {
    try {
      const text = loadRef(root, ref).get('areas.json');
      if (text === undefined) continue;
      const areas = JSON.parse(text);
      if (Array.isArray(areas)) return areas.map((area) => area?.id).filter((id) => typeof id === 'string');
    } catch {
      // Not a git repository, no such ref, or not JSON: try the next.
    }
  }
  return null;
}

/**
 * The owner's latest verdicts: commits touching plan/features/ whose subject starts with `Owner: `, newest first.
 * @param {string} root @param {number} [count] @returns {{ commit: string, date: string, subject: string }[] | null} null outside git.
 */
export function latestVerdicts(root, count = 5) {
  const out = git(root, ['log', '--format=%H%x1f%cI%x1f%s%x1e', '--', 'plan/features/']);
  if (out === null) return null;
  return out
    .split('\x1e')
    .map((record) => record.trim())
    .filter(Boolean)
    .map((record) => {
      const [commit, date, subject] = record.split('\x1f');
      return { commit, date, subject };
    })
    .filter((entry) => entry.subject.startsWith('Owner: '))
    .slice(0, count);
}
