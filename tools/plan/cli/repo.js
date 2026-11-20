// The checkout the command works in: its root, its plan files, git.

import { execFileSync } from 'node:child_process';
import { existsSync, lstatSync, readFileSync, realpathSync, statSync } from 'node:fs';
import { dirname, join, resolve, sep } from 'node:path';
import { ruledKeysDiffer } from '../check/index.js';
import { isHidden, loadDir, loadRef } from '../read/index.js';
import { Refused } from './args.js';

/**
 * The nearest directory, from cwd up, that holds `plan/order.json`. A `plan` that is a symbolic
 * link, dangling or not, stops the search there with the reader's error: it is never followed, and
 * a plan higher up is not the one meant.
 * @param {string} cwd @returns {string}
 */
export function findRoot(cwd) {
  let dir = resolve(cwd);
  for (;;) {
    const plan = join(dir, 'plan');
    if (isLink(plan)) {
      loadDir(plan); // throws the reader's PlanError for a link
      throw new Refused(`${plan} is a symbolic link`);
    }
    if (existsSync(join(plan, 'order.json'))) return dir;
    const up = dirname(dir);
    if (up === dir) throw new Refused(`no plan/ found in ${cwd} or above it`);
    dir = up;
  }
}

/** @param {string} path @returns {boolean} whether the path itself is a symbolic link. */
const isLink = (path) => {
  try {
    return lstatSync(path).isSymbolicLink();
  } catch {
    return false;
  }
};

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
 * The files under `plan/` that git is told not to look at, with the flag that says so
 * (`plan/x.md (assume-unchanged)`): `git status` hides an edit of such a file, so a verdict would
 * read a plan git does not see. Every file counts, dot-named ones too. Throws Refused when git fails.
 * @param {string} root @returns {string[]}
 */
export function unwatched(root) {
  // `ls-files -v` tags each file: a lowercase letter is assume-unchanged, `S` or `s` skip-worktree.
  const out = git(root, ['-c', 'core.fsmonitor=false', 'ls-files', '-v', '-z', '--', 'plan']);
  if (out === null) throw new Refused('git could not say whether a file of plan/ is hidden from it; nothing changed');
  return out.split('\0').filter(Boolean).flatMap((record) => {
    const [tag, path] = [record[0], record.slice(2)];
    const flags = [];
    if (tag !== tag.toUpperCase()) flags.push('assume-unchanged');
    if (tag.toUpperCase() === 'S') flags.push('skip-worktree');
    return flags.length ? [`${path} (${flags.join(', ')})`] : [];
  });
}

/**
 * Whether the latest commit since the base that changed an entry's verdict or its text keys
 * (asks, answer, name, what, sets, removes) is the owner's verdict on it (`Owner: take <id>` or
 * `Owner: drop <id>`), and none of those has changed since, committed or not. Rule 11 does not
 * apply to such an entry: the owner ruled again after the change. A commit that touched only other
 * keys of the file, such as the `adds` a take of another entry rewrites, does not count.
 * @param {string} root @param {string} ref the base @returns {(id: string, file: string) => boolean} file: `plan/…`
 */
export const ruledByOwner = (root, ref) => (id, file) => {
  const at = (rev) => git(root, ['cat-file', 'blob', `${rev}:${file}`]);
  // The working file against HEAD: an uncommitted change of those keys comes after any verdict.
  let disk;
  try {
    disk = readFileSync(join(root, file), 'utf8');
  } catch {
    disk = null;
  }
  if (ruledKeysDiffer(at('HEAD'), disk)) return false;
  const log = git(root, ['log', '-z', '--format=%H %s', `${ref}..HEAD`, '--', file]);
  if (log === null) return false;
  for (const record of log.split('\0').filter(Boolean)) {
    const space = record.indexOf(' ');
    const [commit, subject] = [record.slice(0, space), record.slice(space + 1)];
    if (!ruledKeysDiffer(at(`${commit}^`), at(commit))) continue;
    return [`Owner: take ${id}`, `Owner: drop ${id}`].includes(subject.trim());
  }
  return false;
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
