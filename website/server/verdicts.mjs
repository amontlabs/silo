// The owner's verdict from the Workshop: what `plan take <id>` or `plan drop <id>` and a push to main do, through
// GitHub's API with the owner's own token. Main's plan/ is read at one commit, the plan tool's pure `take` or `drop`
// runs on it, and the files it changed become one commit on main whose only parent is that commit, made and put on
// main in one call (GraphQL createCommitOnBranch) only while main's head is still that commit: a compare-and-swap, so
// a push or a reset of main meanwhile is never undone. When main moved, everything is done again from the new main, a
// bounded number of times. The call either moves main to the commit or changes nothing.
import { drop, entryFile, take, verdictSubject } from '../../tools/plan/write/index.js';
import { isHidden } from '../../tools/plan/read/index.js';
import { GitHubError } from './github.mjs';

export const BRANCH = 'main';
/** How many times the write is done again when main moved between the read and the write. */
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
export const PARTIAL = 'GitHub answered in part, so nothing was written. Try again.';
export const DENIED = 'GitHub refused the change to main. The Workshop needs the permission Contents: Read and write, and your account write access to the repository.';

/**
 * The commit and the move of main, together: GitHub makes the commit on `expectedHeadOid` with the token's user as its
 * author and moves the branch to it only if its head is still that commit.
 */
export const COMMIT_MUTATION = 'mutation Verdict($input: CreateCommitOnBranchInput!) { createCommitOnBranch(input: $input) { commit { oid } } }';
/** The GraphQL error types of an answer that refuses the write itself: the token may not write the repository's contents. */
const DENIED_TYPES = new Set(['FORBIDDEN', 'NOT_FOUND']);
const MODE_FILE = '100644';

/** A file's text from its bytes, refused when they are not UTF-8. */
function decode(bytes, path) {
  try {
    return UTF8.decode(bytes);
  } catch {
    throw new VerdictRefusal(409, 'refused', `plan/${path} is not valid UTF-8`);
  }
}

/** GitHub's answer lacks part of what was asked: nothing is written on it. */
const partial = () => new VerdictRefusal(502, 'partial', PARTIAL);

/**
 * The files of plan/ from the GraphQL answer, by path relative to plan/, and the mode of each. A link or a submodule
 * refuses, whatever its name, as the plan's reader does; a file whose name or folder starts with a dot is skipped.
 * Files GitHub gave no text for (cut, or not text) are listed in `fetch`. A folder without its entries, or an entry
 * without its object, is an answer in part: the read fails whole, so a part of plan/ is never taken for all of it.
 */
function readTree(tree, prefix, depth, out) {
  if (!Array.isArray(tree?.entries)) throw partial();
  for (const entry of tree.entries) {
    if (typeof entry?.name !== 'string') throw partial();
    const path = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.mode === SYMLINK || entry.type === 'commit') throw new VerdictRefusal(409, 'refused', `plan/${path} is a symbolic link or a submodule on main`);
    if (isHidden(path)) continue;
    if (entry.type === 'tree') {
      // The files of a folder at depth 1 are the last ones the query gives with their text.
      if (depth <= 1) throw new VerdictRefusal(409, 'refused', `plan/${path} is deeper than the Workshop reads`);
      readTree(entry.object, path, depth - 1, out);
    } else if (entry.type === 'blob') {
      const blob = entry.object;
      if (typeof blob?.oid !== 'string' || typeof entry.mode !== 'number') throw partial();
      out.modes.set(path, entry.mode.toString(8).padStart(6, '0'));
      if (typeof blob.text === 'string' && !blob.isTruncated) out.files.set(path, blob.text);
      else out.fetch.push({ path, oid: blob.oid });
    }
  }
}

/** Whether main's head is no longer `head`. */
async function moved(call, repo, head) {
  const { data } = await call('GET', `/repos/${repo}/git/ref/heads/${BRANCH}`);
  return data?.object?.sha !== head;
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
    // Any error, even beside data, means the answer may lack part of plan/.
    const errors = Array.isArray(answer?.errors) ? answer.errors : [];
    if (errors.some((error) => error?.type === 'RATE_LIMITED')) throw new GitHubError(429, { as: 'user', rateLimited: true, retryAfter: 60 });
    const repository = answer?.data?.repository;
    if (errors.length || !repository) throw partial();
    if (!repository.plan) throw new VerdictRefusal(409, 'refused', 'main has no plan/');
    const read = { files: new Map(), modes: new Map(), fetch: [] };
    readTree(repository.plan, '', LEVELS, read);
    const blobText = async (oid, where) => {
      let data;
      try {
        ({ data } = await call('GET', `/repos/${repo}/git/blobs/${oid}`));
      } catch (error) {
        // A blob the tree just named and GitHub does not give: the read is in part, not a matter of permission.
        if (error instanceof GitHubError && !error.rateLimited && !error.network) throw partial();
        throw error;
      }
      if (typeof data?.content !== 'string' || data.encoding !== 'base64') throw partial();
      return decode(Buffer.from(data.content, 'base64'), where);
    };
    for (const file of read.fetch) read.files.set(file.path, await blobText(file.oid, file.path));

    const seen = repository.seen;
    const seenText = !seen ? null : typeof seen.text === 'string' && !seen.isTruncated ? seen.text : await blobText(seen.oid, path);
    if (seenText === null || read.files.get(path) !== seenText) throw new VerdictRefusal(409, 'changed', CHANGED);

    const outcome = (verdict === 'take' ? take : drop)(read.files, entry.id);
    if (outcome.status === 'refused') throw new VerdictRefusal(409, 'refused', outcome.message);
    if (outcome.status === 'already') return { status: 'already', commit: null, message: outcome.message };

    // createCommitOnBranch writes plain files only: a file of another mode (executable) would lose it.
    for (const file of outcome.changed.keys()) {
      const mode = read.modes.get(file);
      if (mode !== undefined && mode !== MODE_FILE) throw new VerdictRefusal(409, 'refused', `plan/${file} has the mode ${mode} on main, and the Workshop writes only plain files (${MODE_FILE}): use plan ${verdict} ${entry.id}`);
    }
    const input = {
      branch: { repositoryNameWithOwner: repo, branchName: BRANCH },
      // The subject alone, as `plan take` writes it: `git commit -m <subject>` gives no body.
      message: { headline: verdictSubject(verdict, entry.id) },
      fileChanges: { additions: [...outcome.changed].map(([file, text]) => ({ path: `plan/${file}`, contents: Buffer.from(text, 'utf8').toString('base64') })) },
      expectedHeadOid: head,
    };
    let answered;
    try {
      ({ data: answered } = await call('POST', '/graphql', { body: { query: COMMIT_MUTATION, variables: { input } } }));
    } catch (error) {
      if (!(error instanceof GitHubError) || error.rateLimited) throw error;
      if (error.status === 403 || error.status === 404) throw new VerdictRefusal(403, 'forbidden', DENIED);
      // 409: main is not where the call expected it. Any other failure may hide a commit made: main is read again.
      if (error.status === 409 || await moved(call, repo, head)) continue;
      throw error;
    }
    const oid = answered?.data?.createCommitOnBranch?.commit?.oid;
    if (typeof oid === 'string' && !answered.errors?.length) return { status: 'done', commit: oid, message: outcome.message };
    const failures = Array.isArray(answered?.errors) ? answered.errors : [];
    if (failures.some((error) => error?.type === 'RATE_LIMITED')) throw new GitHubError(429, { as: 'user', rateLimited: true, retryAfter: 60 });
    if (failures.some((error) => DENIED_TYPES.has(error?.type))) throw new VerdictRefusal(403, 'forbidden', DENIED);
    // The head was not the one expected (main moved, forward or back), or the answer says nothing sure: read main again.
    if (await moved(call, repo, head)) continue;
    throw new GitHubError(502, { as: 'user' });
  }
  throw new VerdictRefusal(409, 'busy', BUSY);
}
