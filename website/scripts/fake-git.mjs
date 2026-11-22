// The git data of the fake GitHub: blobs, trees and commits hashed as git hashes them (a tree made here has the id
// git gives the same files), the branch refs, and the reads the Workshop's verdict makes (server/verdicts.mjs).
import { createHash } from 'node:crypto';

const UTF8 = new TextDecoder('utf-8', { fatal: true });
const TREE = '40000';
const SYMLINK = '120000';

export function createFakeGit() {
  /** sha -> { type: 'blob', content: Buffer } | { type: 'tree', entries } | { type: 'commit', tree, parents, message, author, madeAs } */
  const objects = new Map();
  /** 'heads/main' -> sha */
  const refs = new Map();
  const commits = [];

  const hash = (type, body) => createHash('sha1').update(Buffer.concat([Buffer.from(`${type} ${body.length}\0`), body])).digest('hex');

  function blob(content) {
    const body = Buffer.isBuffer(content) ? content : Buffer.from(content, 'utf8');
    const sha = hash('blob', body);
    objects.set(sha, { type: 'blob', content: body });
    return sha;
  }

  /** A tree from `{ name, mode, sha }` entries, `mode` as git writes it ('100644', '40000'), sorted as git sorts them. */
  function tree(entries) {
    const key = (entry) => Buffer.from(entry.mode === TREE ? `${entry.name}/` : entry.name);
    const sorted = [...entries].sort((a, b) => Buffer.compare(key(a), key(b)));
    const body = Buffer.concat(sorted.map((entry) => Buffer.concat([Buffer.from(`${entry.mode} ${entry.name}\0`), Buffer.from(entry.sha, 'hex')])));
    const sha = hash('tree', body);
    objects.set(sha, { type: 'tree', entries: sorted });
    return sha;
  }

  /** The tree of files given by path (`plan/order.json`), each `{ sha, mode }`. */
  function treeOf(files) {
    const dirs = new Map();
    for (const [path, file] of files) {
      const slash = path.indexOf('/');
      if (slash === -1) {
        dirs.set(path, file);
        continue;
      }
      const [dir, rest] = [path.slice(0, slash), path.slice(slash + 1)];
      if (!(dirs.get(dir) instanceof Map)) dirs.set(dir, new Map());
      dirs.get(dir).set(rest, file);
    }
    return tree([...dirs].map(([name, value]) => (value instanceof Map ? { name, mode: TREE, sha: treeOf(value) } : { name, mode: value.mode, sha: value.sha })));
  }

  /** Every file of a tree, by path, `{ sha, mode }`. */
  function flatten(sha, prefix = '', out = new Map()) {
    for (const entry of objects.get(sha).entries) {
      const path = prefix + entry.name;
      if (entry.mode === TREE) flatten(entry.sha, `${path}/`, out);
      else out.set(path, { sha: entry.sha, mode: entry.mode });
    }
    return out;
  }

  /** The text of every file of a tree, by path. */
  function files(sha) {
    return new Map([...flatten(sha)].map(([path, file]) => [path, objects.get(file.sha).content.toString('utf8')]));
  }

  /** A commit; `sha` names it as an existing commit of a real repository is named. */
  function commit({ tree: treeSha, parents = [], message, author = 'fake', madeAs = null, sha }) {
    const id = sha ?? hash('commit', Buffer.from(`tree ${treeSha}\n${parents.map((p) => `parent ${p}\n`).join('')}author ${author}\n\n${message}`));
    const object = { type: 'commit', sha: id, tree: treeSha, parents, message, author, madeAs };
    objects.set(id, object);
    commits.push(object);
    return id;
  }

  /** main as one commit holding `texts` (path -> text); `sha` names the commit. */
  function seed(texts, { sha, branch = 'main', message = 'Seed' } = {}) {
    const entries = new Map([...texts].map(([path, text]) => [path, { sha: blob(text), mode: '100644' }]));
    const id = commit({ tree: treeOf(entries), message, sha });
    refs.set(`heads/${branch}`, id);
    return id;
  }

  /** A commit on `branch` changing `texts` (path -> text, null removes), as a push would make it. */
  function push(texts, { branch = 'main', message = 'A push' } = {}) {
    const head = refs.get(`heads/${branch}`);
    const all = flatten(objects.get(head).tree);
    for (const [path, text] of texts) {
      if (text === null) all.delete(path);
      else all.set(path, { sha: blob(text), mode: all.get(path)?.mode ?? '100644' });
    }
    const id = commit({ tree: treeOf(all), parents: [head], message });
    refs.set(`heads/${branch}`, id);
    return id;
  }

  function isAncestor(ancestor, of) {
    const seen = new Set();
    const queue = [of];
    while (queue.length) {
      const sha = queue.pop();
      if (sha === ancestor) return true;
      if (seen.has(sha)) continue;
      seen.add(sha);
      queue.push(...(objects.get(sha)?.parents ?? []));
    }
    return false;
  }

  /** What `object(expression: "<commit>:<path>")` names, or null. */
  function resolve(expression) {
    const colon = expression.indexOf(':');
    const commitSha = colon === -1 ? expression : expression.slice(0, colon);
    const start = objects.get(commitSha);
    if (start?.type !== 'commit') return null;
    if (colon === -1) return { sha: commitSha, object: start };
    let sha = start.tree;
    for (const name of expression.slice(colon + 1).split('/').filter(Boolean)) {
      const entry = objects.get(sha)?.entries?.find((one) => one.name === name);
      if (!entry) return null;
      sha = entry.sha;
    }
    return { sha, object: objects.get(sha) };
  }

  /**
   * An object as GraphQL gives it for the Workshop's query: a commit's tree, a blob's text (null when it is not
   * UTF-8, cut past `truncateOver` bytes), a tree's entries with `depth` levels of objects under them.
   */
  function graph(found, depth, { truncateOver = Infinity } = {}) {
    if (!found) return null;
    const { sha, object } = found;
    if (object.type === 'commit') return { tree: { oid: object.tree } };
    if (object.type === 'blob') {
      let text = null;
      try {
        text = UTF8.decode(object.content);
      } catch {
        text = null;
      }
      const cut = object.content.length > truncateOver;
      return { oid: sha, text: cut && text !== null ? text.slice(0, truncateOver) : text, isTruncated: cut };
    }
    return {
      entries: object.entries.map((entry) => ({
        name: entry.name,
        type: entry.mode === TREE ? 'tree' : entry.mode === '160000' ? 'commit' : 'blob',
        mode: parseInt(entry.mode, 8),
        ...(depth > 0 ? { object: graph({ sha: entry.sha, object: objects.get(entry.sha) ?? { type: 'commit' } }, depth - 1, { truncateOver }) } : {}),
      })),
    };
  }

  /**
   * The tree of a commit createCommitOnBranch makes: `base_tree` with each entry of `tree` set (`content` or `sha`) or
   * removed (`sha: null`). Answers null when the request names what does not exist.
   */
  function createTree({ base_tree: base, tree: entries }) {
    if (base !== undefined && objects.get(base)?.type !== 'tree') return null;
    const all = base === undefined ? new Map() : flatten(base);
    for (const entry of entries ?? []) {
      if (typeof entry.path !== 'string' || entry.type !== 'blob') return null;
      if (entry.sha === null) {
        all.delete(entry.path);
        continue;
      }
      const sha = typeof entry.content === 'string' ? blob(entry.content) : entry.sha;
      if (objects.get(sha)?.type !== 'blob') return null;
      all.set(entry.path, { sha, mode: entry.mode === '100755' || entry.mode === SYMLINK ? entry.mode : '100644' });
    }
    return treeOf(all);
  }

  return { objects, refs, commits, blob, tree, treeOf, flatten, files, commit, seed, push, isAncestor, resolve, graph, createTree };
}
