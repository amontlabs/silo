// The checkout the command works in: its root, its plan files, git.

import { execFileSync } from 'node:child_process';
import { existsSync, realpathSync, statSync } from 'node:fs';
import { dirname, join, resolve, sep } from 'node:path';
import { isHidden, loadDir, loadRef } from '../read/index.js';
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

/**
 * Whether a path of an `at` names a regular file of the repository: it resolves, links followed, to
 * a file inside the repository's top. A directory, or a link that leads outside, is not one. The
 * check refuses absolute paths and `..` before it asks.
 * @param {string} root @returns {(path: string) => boolean}
 */
export const existsIn = (root) => {
  const top = realpathSync(root);
  return (path) => {
    try {
      const real = realpathSync(join(top, path));
      return real.startsWith(top + sep) && statSync(real).isFile();
    } catch {
      return false;
    }
  };
};

/**
 * The paths under `plan/` that git sees changed, staged or untracked, as the reader would read
 * them. Untracked files are listed one by one and ignored files too, whatever the user's
 * settings; a rename counts by both its names. A path whose every part the reader reads is kept;
 * one the reader skips (a part of its name starts with a dot) is not. Throws Refused when git fails.
 * @param {string} root @returns {string[]}
 */
export function uncommitted(root) {
  const out = git(root, ['-c', 'core.fsmonitor=false', 'status', '--porcelain=v1', '-z', '--untracked-files=all', '--ignored=traditional', '--no-renames', '--ignore-submodules=none', '--', 'plan']);
  if (out === null) throw new Refused('git could not say whether plan/ has uncommitted changes; nothing changed');
  return out.split('\0').filter(Boolean).map((record) => record.slice(3)).filter((path) => !isHidden(path.slice('plan/'.length)));
}

/**
 * Whether the latest commit since the base that changed an entry's file is the owner's verdict on
 * it (`Owner: take <id>` or `Owner: drop <id>`), and the file is still as that commit left it.
 * Rule 11 does not apply to such an entry: the owner ruled again after the change.
 * @param {string} root @param {string} ref the base @returns {(id: string, file: string) => boolean} file: `plan/…`
 */
export const ruledByOwner = (root, ref) => (id, file) => {
  const subject = git(root, ['log', '-1', '--format=%s', `${ref}..HEAD`, '--', file]);
  if (subject === null || ![`Owner: take ${id}`, `Owner: drop ${id}`].includes(subject.trim())) return false;
  return git(root, ['diff', '--quiet', 'HEAD', '--', file]) !== null;
};

/**
 * The files of `plan/` in the merged plan (`origin/main`, else `HEAD`), or null outside git or
 * without either ref. A base that exists but does not read (a link under `plan/`) throws.
 * @param {string} root @returns {{ ref: string, files: Map<string, string> } | null}
 */
export function baseFiles(root) {
  for (const ref of ['origin/main', 'HEAD']) {
    if (git(root, ['rev-parse', '--verify', '--quiet', `${ref}^{commit}`]) === null) continue;
    return { ref, files: loadRef(root, ref) };
  }
  return null;
}
