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
 * Whether the entry's file holds what the owner ruled on. Among the commits since the base, on any
 * branch that was merged, the latest whose subject is exactly a verdict on the entry, the owner's
 * (`Owner: take <id>` or `Owner: drop <id>`) or the agents' (`Agents: take <id>` or `Agents: drop <id>`;
 * with a `verdict`, only that kind) is found; no commit
 * comes before a descendant of it. In it the entry's file is found by its id, among the files the
 * commit wrote (not those it deleted), wherever it was then. The answer is yes when that version and
 * the working file, committed or not, agree on the verdict and on the text keys (asks, answer,
 * name, what, replaces, sets, removes): so a merge, a move or an edit of other keys, such as the
 * `adds` a take of another entry rewrites, changes nothing, and neither does an edit that was put
 * back. The commit's own file says who wrote the verdict (`by`) as the subject does: `Agents:` with
 * `"by": "agents"`, `Owner:` with no `by`. No such commit, or a difference: no. Rule 11 does not apply to an entry for which it is yes.
 * @param {string} root @param {string} ref the base @returns {(id: string, file: string, verdict?: string) => boolean} file: `plan/…`
 */
export const ruledByOwner = (root, ref) => (id, file, verdict) => {
  const kinds = verdict ? [verdict] : ['take', 'drop'];
  const subjects = ['Owner', 'Agents'].flatMap((who) => kinds.map((kind) => `${who}: ${kind} ${id}`));
  const log = git(root, ['log', '-z', '--date-order', '--format=%H %s', `${ref}..HEAD`]);
  if (log === null) return false;
  const found = log.split('\0').filter(Boolean).find((record) => subjects.includes(record.slice(record.indexOf(' ') + 1)));
  if (found === undefined) return false;
  const commit = found.slice(0, found.indexOf(' '));
  const byAgents = found.slice(found.indexOf(' ') + 1).startsWith('Agents: ');
  const changed = git(root, ['diff-tree', '-r', '--root', '--no-renames', '--diff-filter=d', '--name-only', '-z', '--no-commit-id', commit]);
  // plan/decisions/<id>.json, plan/<part>/decisions/<id>.json or plan/<part>/features/<id>.json
  const isEntryFile = (name) => {
    const parts = name.split('/');
    return parts[0] === 'plan' && (parts.length === 3 || parts.length === 4) && parts.at(-1) === `${id}.json` && ['decisions', 'features'].includes(parts.at(-2));
  };
  const path = changed?.split('\0').find(isEntryFile);
  if (path === undefined) return false;
  let disk;
  try {
    disk = readFileSync(join(root, file), 'utf8');
  } catch {
    disk = null;
  }
  const blob = git(root, ['cat-file', 'blob', `${commit}:${path}`]);
  let by;
  try {
    by = JSON.parse(blob ?? '').by;
  } catch {
    by = undefined;
  }
  return (by === 'agents') === byAgents && !ruledKeysDiffer(blob, disk);
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
