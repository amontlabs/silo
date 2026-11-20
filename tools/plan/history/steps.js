// Reads the steps of the plan's history from a git repository: the first-parent chain of HEAD, one
// step per commit that changed an entry's file or notes.

import { PATHSPECS, entryPath } from './paths.js';

// At most three git processes start, whatever the number of commits: is-shallow, log, cat-file.
const BUFFER = 1024 * 1024 * 1024;

/**
 * The steps of the repository at `root`, oldest first, and whether it is a shallow clone. Each step is
 * `{ commit, date, files }`: the full hash, the committer date in UTC (`2026-11-20T12:00:00Z`), and for
 * each changed path relative to `plan/` the text before and after, null when the file did not exist.
 * A merge is one step, compared with its first parent. Makes no network call and never throws: with no
 * repository, no commit or no `plan/`, there are no steps.
 * @param {string} root
 * @returns {{ steps: { commit: string, date: string, files: Record<string, { before: string | null, after: string | null }> }[], shallow: boolean }}
 */
export function readSteps(root) {
  // Looked up here and not imported, so that nothing that imports this folder has a `node:` import.
  const { execFileSync } = process.getBuiltinModule('node:child_process');
  const git = (args, input) => execFileSync('git', ['-C', root, ...args], { input, stdio: ['pipe', 'pipe', 'ignore'], maxBuffer: BUFFER, env: { ...process.env, GIT_NO_LAZY_FETCH: '1' } });
  try {
    const shallow = git(['rev-parse', '--is-shallow-repository']).toString().trim() === 'true';
    // `-z` keeps a path with a newline or a quote whole; `--no-renames` turns a move into a delete and an add.
    const log = git(['log', '--first-parent', '-m', '--raw', '-z', '--no-abbrev', '--no-renames', '--no-relative', '--root', '--format=%x01%H %ct', '--', ...PATHSPECS]).toString('utf8');
    /** @type {{ commit: string, date: string, changes: { path: string, status: string, old: string, now: string }[] }[]} */
    const commits = [];
    const tokens = log.split('\0');
    for (let i = 0; i < tokens.length; i += 1) {
      // The line the format ends with comes before the first record of a commit.
      const token = tokens[i].replace(/^\n/, '');
      const at = token.indexOf('\x01');
      if (at !== -1) {
        const [commit, time] = token.slice(at + 1).split(' ');
        commits.push({ commit, date: new Date(Number(time) * 1000).toISOString().replace('.000Z', 'Z'), changes: [] });
      } else if (token.startsWith(':') && commits.length) {
        const [oldMode, newMode, old, now, status] = token.slice(1).split(' ');
        const path = tokens[(i += 1)];
        // A symbolic link or a submodule is no file of the plan.
        if (typeof path === 'string' && path.startsWith('plan/') && entryPath(path.slice(5)) && !/^1[26]0000$/.test(oldMode) === !/^1[26]0000$/.test(newMode)) {
          commits.at(-1).changes.push({ path: path.slice(5), status, old, now });
        }
      }
    }
    const isNone = (oid) => /^0+$/.test(oid);
    const wanted = [...new Set(commits.flatMap((c) => c.changes.flatMap(({ old, now }) => [old, now])).filter((oid) => !isNone(oid)))];
    /** @type {Map<string, string>} */
    const blobs = new Map();
    if (wanted.length) {
      const out = git(['cat-file', '--batch'], wanted.map((oid) => `${oid}\n`).join(''));
      const decoder = new TextDecoder('utf-8');
      let at = 0;
      for (const oid of wanted) {
        const end = out.indexOf(10, at);
        if (end === -1) break;
        const [, type, size] = out.subarray(at, end).toString().split(' ');
        if (type === 'missing') {
          // An object the clone does not hold reads as an empty file, which is not JSON.
          blobs.set(oid, '');
          at = end + 1;
        } else {
          blobs.set(oid, decoder.decode(out.subarray(end + 1, end + 1 + Number(size))));
          at = end + 1 + Number(size) + 1;
        }
      }
    }
    const text = (oid) => (isNone(oid) ? null : (blobs.get(oid) ?? ''));
    const steps = commits.reverse().filter((c) => c.changes.length).map(({ commit, date, changes }) => ({
      commit,
      date,
      files: Object.fromEntries(changes.map(({ path, old, now }) => [path, { before: text(old), after: text(now) }])),
    }));
    return { steps, shallow };
  } catch {
    return { steps: [], shallow: false };
  }
}
