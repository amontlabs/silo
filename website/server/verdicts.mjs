// The owner's verdict from the Workshop: what `plan take <id>` or `plan drop <id>` and a push to main do, through
// GitHub's API with the owner's own token. Main's plan/ is read at one commit, the plan tool's pure `take` or `drop`
// runs on it, and the files it changed become one commit on main whose only parent is that commit. The ref moves
// only as a fast-forward: when main moved meanwhile, everything is done again from the new main. Nothing reaches
// main before the ref moves, and the objects made before a failure are reachable from nothing.
import { drop, entryFile, take, verdictSubject } from '../../tools/plan/write/index.js';
import { isHidden } from '../../tools/plan/read/index.js';
import { GitHubError } from './github.mjs';

export const BRANCH = 'main';
/** How many times the write is done again when main moved between the read and the ref update. */
export const ATTEMPTS = 3;
/** How deep plan/ may go: `plan/<part>/decisions/<id>.json` is three folders down from plan/'s own files. */
const LEVELS = 3;
const SYMLINK = 0o120000;
const UTF8 = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true });

const FILE = 'oid text isTruncated';
/** One folder of the tree, `depth` folders more below it, with each file's text; the last level gives names only. */
const level = (depth) => (depth === 0
  ? 'entries { name type mode }'
  : `entries { name type mode object { ... on Blob { ${FILE} } ... on Tree { ${level(depth - 1)} } } }`);

/**
 * One GraphQL call: main's commit and root tree, every file of plan/ at it with its text, and the entry's file at
 * the commit the page was built from.
 */
export const PLAN_QUERY = `query PlanAt($owner: String!, $name: String!, $commit: String!, $plan: String!, $seen: String!) { repository(owner: $owner, name: $name) { commit: object(expression: $commit) { ... on Commit { tree { oid } } } plan: object(expression: $plan) { ... on Tree { ${level(LEVELS)} } } seen: object(expression: $seen) { ... on Blob { ${FILE} } } } }`;

/** A verdict the route answers with a refusal: `status`, `code` and the message the page shows. */
export class VerdictRefusal extends Error {
  constructor(status, code, message) {
    super(message);
    this.status = status;
    this.code = code;
  }
}

export const CHANGED = 'This entry changed since the page was built: reload the page.';
export const BUSY = 'Main kept changing while the verdict was written. Try again.';
export const DENIED = 'GitHub refused the change to main. The Workshop needs the permission Contents: Read and write, and your account write access to the repository.';

/** A file's text from its bytes, refused when they are not UTF-8. */
function decode(bytes, path) {
  try {
    return UTF8.decode(bytes);
  } catch {
    throw new VerdictRefusal(409, 'refused', `plan/${path} is not valid UTF-8`);
  }
}

/**
 * The files of plan/ from the GraphQL answer, by path relative to plan/, and the mode of each. A link or a submodule
 * refuses, whatever its name, as the plan's reader does; a file whose name or folder starts with a dot is skipped.
 * Files GitHub gave no text for (cut, or not text) are listed in `fetch`.
 */
function readTree(tree, prefix, depth, out) {
  for (const entry of tree?.entries ?? []) {
    const path = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.mode === SYMLINK || entry.type === 'commit') throw new VerdictRefusal(409, 'refused', `plan/${path} is a symbolic link or a submodule on main`);
    if (isHidden(path)) continue;
    if (entry.type === 'tree') {
      // The files of a folder at depth 1 are the last ones the query gives with their text.
      if (depth <= 1 || !entry.object) throw new VerdictRefusal(409, 'refused', `plan/${path} is deeper than the Workshop reads`);
      readTree(entry.object, path, depth - 1, out);
    } else if (entry.type === 'blob') {
      out.modes.set(path, entry.mode.toString(8).padStart(6, '0'));
      const blob = entry.object ?? {};
      if (typeof blob.text === 'string' && !blob.isTruncated) out.files.set(path, blob.text);
      else out.fetch.push({ path, oid: blob.oid });
    }
  }
}

/**
 * Writes the owner's verdict on `entry` ({ id, kind, part }, from the plan the server shipped with) on main.
 * `call(method, path, options)` calls GitHub with the owner's token. `built` is the commit the page was built from:
 * the entry's file there must be the one on main, so the verdict is for the text the owner saw. Answers
 * `{ status: 'done' | 'already', commit, message }`; throws a VerdictRefusal, or the GitHubError of a failed call.
 */
export async function writeVerdict({ call, repo, entry, verdict, built, attempts = ATTEMPTS }) {
  const [owner, name] = repo.split('/');
  const path = entryFile(entry);
  for (let attempt = 0; attempt < attempts; attempt += 1) {
    const { data: ref } = await call('GET', `/repos/${repo}/git/ref/heads/${BRANCH}`);
    const head = ref?.object?.sha;
    if (typeof head !== 'string') throw new GitHubError(502, { as: 'user' });

    const { data: answer } = await call('POST', '/graphql', { body: { query: PLAN_QUERY, variables: { owner, name, commit: head, plan: `${head}:plan`, seen: `${built}:plan/${path}` } } });
    const repository = answer?.data?.repository;
    if (!repository) {
      const limited = Array.isArray(answer?.errors) && answer.errors.some((error) => error?.type === 'RATE_LIMITED');
      throw new GitHubError(limited ? 429 : 502, { as: 'user', rateLimited: limited, retryAfter: limited ? 60 : 0 });
    }
    const root = repository.commit?.tree?.oid;
    if (typeof root !== 'string' || !repository.plan) throw new VerdictRefusal(409, 'refused', 'main has no plan/');
    const read = { files: new Map(), modes: new Map(), fetch: [] };
    readTree(repository.plan, '', LEVELS, read);
    const blobText = async (oid, where) => {
      const { data } = await call('GET', `/repos/${repo}/git/blobs/${oid}`);
      return decode(Buffer.from(String(data?.content ?? ''), 'base64'), where);
    };
    for (const file of read.fetch) read.files.set(file.path, await blobText(file.oid, file.path));

    const seen = repository.seen;
    const seenText = !seen ? null : typeof seen.text === 'string' && !seen.isTruncated ? seen.text : await blobText(seen.oid, path);
    if (seenText === null || read.files.get(path) !== seenText) throw new VerdictRefusal(409, 'changed', CHANGED);

    const outcome = (verdict === 'take' ? take : drop)(read.files, entry.id);
    if (outcome.status === 'refused') throw new VerdictRefusal(409, 'refused', outcome.message);
    if (outcome.status === 'already') return { status: 'already', commit: null, message: outcome.message };

    const tree = [...outcome.changed].map(([file, text]) => ({ path: `plan/${file}`, mode: read.modes.get(file) ?? '100644', type: 'blob', content: text }));
    const { data: made } = await call('POST', `/repos/${repo}/git/trees`, { body: { base_tree: root, tree } });
    const { data: commit } = await call('POST', `/repos/${repo}/git/commits`, { body: { message: verdictSubject(verdict, entry.id), tree: made.sha, parents: [head] } });
    try {
      await call('PATCH', `/repos/${repo}/git/refs/heads/${BRANCH}`, { body: { sha: commit.sha, force: false } });
    } catch (error) {
      // GitHub answers 422 when the update is not a fast-forward: main moved since it was read.
      if (!(error instanceof GitHubError && error.status === 422)) throw error;
      const { data: now } = await call('GET', `/repos/${repo}/git/ref/heads/${BRANCH}`);
      if (now?.object?.sha === head) throw error;
      continue;
    }
    return { status: 'done', commit: commit.sha, message: outcome.message };
  }
  throw new VerdictRefusal(409, 'busy', BUSY);
}
