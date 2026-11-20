// The checkout the command works in: its root, its plan files, git.

import { execFileSync } from 'node:child_process';
import { existsSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { loadDir, loadRef } from '../read/index.js';
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
    return execFileSync('git', ['-C', root, ...args], { stdio: ['ignore', 'pipe', 'pipe'], maxBuffer: 64 * 1024 * 1024 }).toString();
  } catch {
    return null;
  }
}

/** @param {string} root @returns {Map<string, string>} the files of `plan/`, by path relative to it. */
export const planFiles = (root) => loadDir(join(root, 'plan'));

/** @param {string} root @returns {(path: string) => boolean} whether a path of the repository exists. */
export const existsIn = (root) => (path) => existsSync(join(root, path));

/**
 * The files of `plan/` in the merged plan (`origin/main`, else `HEAD`), or null outside git or
 * without either ref.
 * @param {string} root @returns {{ ref: string, files: Map<string, string> } | null}
 */
export function baseFiles(root) {
  for (const ref of ['origin/main', 'HEAD']) {
    try {
      return { ref, files: loadRef(root, ref) };
    } catch {
      // Not a git repository, or no such ref: try the next.
    }
  }
  return null;
}
