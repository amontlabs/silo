// The owner's verdict from the Workshop: what `plan take <id>` or `plan drop <id>` and a push to main do, through
// GitHub's API with the owner's own token. Main's plan/ is read at one commit, the plan tool's pure `take` or `drop`
// runs on it, and the files it changed become one commit on main whose only parent is that commit, made and put on
// main in one call (GraphQL createCommitOnBranch) only while main's head is still that commit: a compare-and-swap, so
// a push or a reset of main meanwhile is never undone. When main moved, everything is done again from the new main, a
// bounded number of times. The call either moves main to the commit or changes nothing.
import { drop, entryFile, take, verdictSubject } from '../../tools/plan/write/index.js';
import { isHidden, parsePlan } from '../../tools/plan/read/index.js';
import { wordId } from '../../tools/plan/model/names.js';
import { GitHubError } from './github.mjs';
// The site's own checks and Silo's map, as deployed with this server: the layout is code, not a file of main.
import { checkBuild, PlanFileError } from '../src/workshop/build-check.ts';
import { SILO_MAP } from '../src/workshop/map.ts';

export const BRANCH = 'main';
/** How many times the write is done again when main moved between the read and the write. */
export const ATTEMPTS = 3;
/**
 * How many folders down one read of plan/ gives the files with their text: `plan/<part>/decisions/<id>.json` is in the
 * third. A folder deeper than that is read by a follow-up query, `LEVELS` more at a time, up to `ROUNDS` of them.
 */
export const LEVELS = 3;
const ROUNDS = 8;
const SYMLINK = 0o120000;
const UTF8 = new TextDecoder('utf-8', { fatal: true, ignoreBOM: true });

const FILE = 'oid text isTruncated';
const NAMES = 'entries { name type mode }';
/** The modes git gives a regular file: what the plan tool's `exists` counts as a file of the repository. */
const REGULAR = new Set([0o100644, 0o100755]);
/** One folder of the tree, `depth` folders more below it, with each file's text; the last level gives names only. */
const level = (depth) => (depth === 0
  ? 'entries { name type mode }'
  : `entries { name type mode object { ... on Blob { ${FILE} } ... on Tree { ${level(depth - 1)} } } }`);

/** One folder of a tree with the id of each file, `depth` folders more below it. */
const ids = (depth) => (depth === 0
  ? 'entries { name type oid }'
  : `entries { name type oid object { ... on Tree { ${ids(depth - 1)} } } }`);

/**
 * One GraphQL call: the names in main's root folder, every file of plan/ at main with its text, the commit the page
 * was built from, and the id of every file of plan/ at it.
 */
export const PLAN_QUERY = `query PlanAt($owner: String!, $name: String!, $commit: String!, $plan: String!, $built: String!, $builtPlan: String!) { repository(owner: $owner, name: $name) { commit: object(expression: $commit) { ... on Commit { tree { ${NAMES} } } } plan: object(expression: $plan) { ... on Tree { ${level(LEVELS)} } } built: object(expression: $built) { ... on Commit { oid } } builtPlan: object(expression: $builtPlan) { ... on Tree { ${ids(LEVELS)} } } } }`;

/** One GraphQL call: each folder of plan/ in `dirs` at `commit`, as `d<index>`, with its files' text `LEVELS` folders down. */
export const deeperQuery = (commit, dirs) => `query Deeper($owner: String!, $name: String!) { repository(owner: $owner, name: $name) { ${dirs.map((dir, index) => `d${index}: object(expression: ${JSON.stringify(`${commit}:plan/${dir}`)}) { ... on Tree { ${level(LEVELS)} } }`).join(' ')} } }`;

/** One GraphQL call: the file `path` of the repository at `commit`, with its text. */
export const entryQuery = (commit, path) => `query EntryAt($owner: String!, $name: String!) { repository(owner: $owner, name: $name) { file: object(expression: ${JSON.stringify(`${commit}:${path}`)}) { ... on Blob { ${FILE} } } } }`;

/** One GraphQL call: the names in each folder of `dirs` at `commit`, as `d<index>`; a folder that is not one has no entries. */
export const existsQuery = (commit, dirs) => `query Exists($owner: String!, $name: String!) { repository(owner: $owner, name: $name) { ${dirs.map((dir, index) => `d${index}: object(expression: ${JSON.stringify(`${commit}:${dir}`)}) { __typename ... on Tree { ${NAMES} } }`).join(' ')} } }`;

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
 * refuses, whatever its name, as the plan's reader does; a file whose name or folder starts with a dot is not read.
 * Files GitHub gave no text for (cut, or not text) are listed in `fetch`, and folders below what the answer gives with
 * their files in `cut`. A folder without its entries, or an entry without its object, anywhere, under a dot name too,
 * is an answer in part: the read fails whole, so a part of plan/ is never taken for all of it.
 */
function readTree(tree, prefix, depth, out) {
  if (!Array.isArray(tree?.entries)) throw partial();
  for (const entry of tree.entries) {
    if (typeof entry?.name !== 'string') throw partial();
    const path = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.mode === SYMLINK || entry.type === 'commit') throw new VerdictRefusal(409, 'refused', `plan/${path} is a symbolic link or a submodule on main`);
    // A folder is looked into whatever its name, for links, as tools/plan/read does; a file under a dot name is not read.
    if (entry.type === 'tree') {
      // The files of a folder at depth 1 are the last ones the answer gives with their text: it is read again on its own.
      if (depth <= 1) out.cut.push(path);
      else readTree(entry.object, path, depth - 1, out);
    } else if (entry.type === 'blob') {
      const blob = entry.object;
      if (typeof blob?.oid !== 'string' || typeof entry.mode !== 'number') throw partial();
      if (isHidden(path)) continue;
      out.modes.set(path, entry.mode.toString(8).padStart(6, '0'));
      out.oids.set(path, blob.oid);
      if (typeof blob.text === 'string' && !blob.isTruncated) out.files.set(path, blob.text);
      else out.fetch.push({ path, oid: blob.oid });
    }
  }
}

/** The id of each file of a tree from the GraphQL answer, by path; what lies deeper than the query reads is left out. */
function idsOf(tree, prefix, depth, out) {
  if (!Array.isArray(tree?.entries)) throw partial();
  for (const entry of tree.entries) {
    if (typeof entry?.name !== 'string') throw partial();
    const path = prefix ? `${prefix}/${entry.name}` : entry.name;
    if (entry.type === 'tree' && depth > 1) idsOf(entry.object, path, depth - 1, out);
    else if (entry.type === 'blob') {
      if (typeof entry.oid !== 'string') throw partial();
      out.set(path, entry.oid);
    }
  }
  return out;
}

const LIST_FILE = /^(?:words|principles|(?:[^/]+\/)?properties)\.json$/;
/** An entry's file, `decisions/<id>.json` or `features/<id>.json`, at the root or in a part: its id. */
const ENTRY_FILE = /^(?:[^/]+\/)?(?:decisions|features)\/([^/]+)\.json$/;
const strings = (value) => (Array.isArray(value) ? value.filter((item) => typeof item === 'string') : []);

/** The ids of the statements an entry's page shows under Now: those it sets, removes, adds or keeps. */
function statementsOf(entry) {
  const statements = new Set(['removes', 'adds', 'keeps'].flatMap((key) => strings(entry[key])));
  for (const item of Array.isArray(entry.sets) ? entry.sets : []) {
    if (typeof item?.id === 'string') statements.add(item.id);
    if (typeof item?.word === 'string') statements.add(wordId(item.word));
  }
  return statements;
}

/** Whether the list `list`, the file `file`, holds one of `statements`. */
function listShows(file, list, statements) {
  if (!LIST_FILE.test(file) || isHidden(file) || !Array.isArray(list)) return false;
  const idOf = (statement) => (file === 'words.json' ? (typeof statement?.word === 'string' ? wordId(statement.word) : null) : statement?.id);
  return list.some((statement) => statements.has(idOf(statement)));
}

/**
 * The files whose text the entry's page shows for it beside its own file and notes: the lists that hold a statement
 * it sets, removes, adds or keeps (the page shows each one's text as it is now), and the files of the decisions it
 * replaces (their answer). Paths relative to plan/.
 */
export function shownFiles(files, id) {
  const { plan, json } = parsePlan(files);
  const entry = Object.hasOwn(plan.entries, id) ? plan.entries[id] : null;
  if (!entry) return [];
  const statements = statementsOf(entry);
  const shown = new Set([...json].filter(([file, list]) => listShows(file, list, statements)).map(([file]) => file));
  for (const replaced of strings(entry.replaces)) if (Object.hasOwn(plan.entries, replaced)) shown.add(entryFile(plan.entries[replaced]));
  return [...shown];
}

/**
 * The files the page built from the build showed for the entry, as far as main does not show them: a list that held
 * a statement shown at the build and differs on main (the build's text is read for it), and the file of a decision it
 * replaces wherever it was at the build. `entry` is the entry as both have it (its file is the same at both), `seen`
 * the id of each file at the build, `oids` the same on main.
 */
async function shownAtBuild(entry, seen, oids, textOf) {
  const statements = statementsOf(entry);
  const replaces = new Set(strings(entry.replaces));
  const shown = [];
  for (const [file, oid] of seen) {
    if (isHidden(file)) continue;
    if (replaces.has(ENTRY_FILE.exec(file)?.[1])) shown.push(file);
    else if (LIST_FILE.test(file) && oids.get(file) !== oid) {
      let list;
      try {
        list = JSON.parse(await textOf(oid, file));
      } catch (error) {
        if (error instanceof VerdictRefusal) throw error;
        continue;
      }
      if (listShows(file, list, statements)) shown.push(file);
    }
  }
  return shown;
}

/** The verdict an entry's file holds as the owner's (no `by`), or null. @param {string | undefined} text */
export function ownersVerdict(text) {
  try {
    const data = JSON.parse(text ?? '');
    return data !== null && typeof data === 'object' && !Array.isArray(data) && !Object.hasOwn(data, 'by') && (data.verdict === 'take' || data.verdict === 'drop') ? data.verdict : null;
  } catch {
    return null;
  }
}

/** Whether `built` is a commit of the history of `head`, as a page built from main's history names. */
async function inHistory(call, repo, built, head) {
  if (built === head) return true;
  try {
    const { data } = await call('GET', `/repos/${repo}/compare/${built}...${head}?per_page=1`);
    return data?.status === 'ahead' || data?.status === 'identical';
  } catch (error) {
    if (error instanceof GitHubError && error.status === 404) return false;
    throw error;
  }
}

/** The names of a folder from a GraphQL answer, by name: `{ type, mode }`. An answer without them is in part. */
function namesOf(tree) {
  if (!Array.isArray(tree?.entries) || tree.entries.some((entry) => typeof entry?.name !== 'string')) throw partial();
  return new Map(tree.entries.map((entry) => [entry.name, entry]));
}

/**
 * The plan tool's `exists` (tools/plan/cli/repo.js `existsIn`) answered from main: whether each path an `at` names is a
 * regular file of the repository at `head`. Asked of the paths `paths` only, in one call; `root` is main's root folder.
 * A path through a link, or a link itself, is not followed: it counts as no file.
 */
async function filesOnMain({ call, owner, name, head, root, paths }) {
  const dirs = [...new Set([...paths].map((path) => path.slice(0, Math.max(0, path.lastIndexOf('/')))))].filter((dir) => dir !== '');
  /** @type {Map<string, Map<string, any> | null>} */
  const folders = new Map([['', namesOf(root)]]);
  if (dirs.length) {
    const { data: answer } = await call('POST', '/graphql', { body: { query: existsQuery(head, dirs), variables: { owner, name } } });
    const errors = Array.isArray(answer?.errors) ? answer.errors : [];
    if (errors.some((error) => error?.type === 'RATE_LIMITED')) throw new GitHubError(429, { as: 'user', rateLimited: true, retryAfter: 60 });
    const repository = answer?.data?.repository;
    if (errors.length || !repository) throw partial();
    dirs.forEach((dir, index) => {
      const object = repository[`d${index}`];
      if (object === undefined) throw partial();
      // null: no such path at main; another type than a folder: no folder there.
      folders.set(dir, object?.__typename === 'Tree' ? namesOf(object) : null);
    });
  }
  const files = new Set([...paths].filter((path) => {
    const slash = path.lastIndexOf('/');
    const entry = folders.get(slash === -1 ? '' : path.slice(0, slash))?.get(path.slice(slash + 1));
    return entry?.type === 'blob' && REGULAR.has(entry.mode);
  }));
  return (path) => files.has(path);
}

/** Whether a GraphQL error of createCommitOnBranch says the branch's head was not `expectedHeadOid`: main moved. */
const headMismatch = (error) => error?.type === 'STALE_DATA' || /expected branch to point to/i.test(String(error?.message ?? ''));

/** Whether main's head is no longer `head`. */
async function moved(call, repo, head) {
  const { data } = await call('GET', `/repos/${repo}/git/ref/heads/${BRANCH}`);
  return data?.object?.sha !== head;
}

/**
 * Writes the owner's verdict on `entry` ({ id, kind, part }, from the plan the server shipped with) on main.
 * `call(method, path, options)` calls GitHub with the owner's token. `built` is the commit the page was built from:
 * what the page showed for the entry there must be on main, so the verdict is for the text the owner saw. `map` is the
 * map's layout the plan the verdict leaves is checked against, as the site's build checks it. Answers
 * `{ status: 'done' | 'already', commit, message }`; throws a VerdictRefusal, or the GitHubError of a failed call.
 *
 * After a write whose outcome is unknown (its answer lost, none in time, or a 5xx), main's entry is read again before
 * any failure is answered: the commit may be on main, and then the answer is "already".
 */
export async function writeVerdict(options) {
  const doubt = { write: false };
  try {
    return await attempts(options, doubt);
  } catch (error) {
    if (!doubt.write || (error instanceof GitHubError && error.rateLimited)) throw error;
    let done = false;
    try {
      done = await verdictOnMain(options);
    } catch {
      // The reread failed too: the first failure is the answer.
    }
    if (done) return already(options.entry.id, options.verdict);
    throw error;
  }
}

const already = (id, verdict) => ({ status: 'already', commit: null, message: `${id} is already ${verdict === 'take' ? 'taken' : 'dropped'}` });

/** A file's text from its blob `oid` (REST), for one GraphQL gave without its text. */
async function blobText(call, repo, oid, where) {
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
}

/** The data of a GraphQL answer; any error beside it, or none, is an answer in part. */
function graphData(answer) {
  const errors = Array.isArray(answer?.errors) ? answer.errors : [];
  if (errors.some((error) => error?.type === 'RATE_LIMITED')) throw new GitHubError(429, { as: 'user', rateLimited: true, retryAfter: 60 });
  const repository = answer?.data?.repository;
  if (errors.length || !repository) throw partial();
  return repository;
}

/** Whether main's file of the entry holds the owner's `verdict` now. */
async function verdictOnMain({ call, repo, entry, verdict }) {
  const [owner, name] = repo.split('/');
  const { data: answer } = await call('POST', '/graphql', { body: { query: entryQuery(BRANCH, `plan/${entryFile(entry)}`), variables: { owner, name } } });
  const file = graphData(answer).file;
  if (typeof file?.oid !== 'string') return false;
  const text = typeof file.text === 'string' && !file.isTruncated ? file.text : await blobText(call, repo, file.oid, entryFile(entry));
  return ownersVerdict(text) === verdict;
}

async function attempts({ call, repo, entry, verdict, built, attempts: tries = ATTEMPTS, map = SILO_MAP }, doubt) {
  const [owner, name] = repo.split('/');
  const path = entryFile(entry);
  for (let attempt = 0; attempt < tries; attempt += 1) {
    const { data: ref } = await call('GET', `/repos/${repo}/git/ref/heads/${BRANCH}`);
    const head = ref?.object?.sha;
    if (typeof head !== 'string') throw new GitHubError(502, { as: 'user' });

    const { data: answer } = await call('POST', '/graphql', { body: { query: PLAN_QUERY, variables: { owner, name, commit: head, plan: `${head}:plan`, built, builtPlan: `${built}:plan` } } });
    // Any error, even beside data, means the answer may lack part of plan/.
    const repository = graphData(answer);
    const root = namesOf(repository.commit?.tree);
    if (!repository.plan) {
      // plan/ in main's root without its object is an answer in part, not a main without plan/.
      if (root.get('plan')?.type === 'tree') throw partial();
      throw new VerdictRefusal(409, 'refused', 'main has no plan/');
    }
    const read = { files: new Map(), modes: new Map(), oids: new Map(), fetch: [], cut: [] };
    readTree(repository.plan, '', LEVELS, read);
    // The folders the answer cut, read on their own to plan/'s full depth.
    for (let round = 0; read.cut.length; round += 1) {
      if (round === ROUNDS) throw new VerdictRefusal(409, 'refused', `plan/${read.cut[0]} is deeper than the Workshop reads`);
      const dirs = read.cut;
      read.cut = [];
      const { data: deeper } = await call('POST', '/graphql', { body: { query: deeperQuery(head, dirs), variables: { owner, name } } });
      const folders = graphData(deeper);
      dirs.forEach((dir, index) => readTree(folders[`d${index}`], dir, LEVELS, read));
    }
    const textOf = (oid, where) => blobText(call, repo, oid, where);
    for (const file of read.fetch) read.files.set(file.path, await textOf(file.oid, file.path));

    // The owner's verdict already on main: done, whatever the page saw. The page asks again after an answer it lost,
    // and the verdict it sent may be on main by then.
    if (ownersVerdict(read.files.get(path)) === verdict) return already(entry.id, verdict);

    // The options `plan take` passes: `exists`, whether each `at` names a file of the repository, here answered from
    // main. A first run with every path a file names the paths the plan tool asks about.
    const write = verdict === 'take' ? take : drop;
    const asked = new Set();
    write(read.files, entry.id, { exists: (file) => { asked.add(file); return true; } });
    const exists = await filesOnMain({ call, owner, name, head, root: repository.commit?.tree, paths: asked });
    const outcome = write(read.files, entry.id, { exists });

    // A verdict is for the text the owner saw: the page's build is a commit of main's history, and every file the
    // page showed for the entry, at the build or as main shows it now, and every file the verdict writes, is at main
    // as it was at that commit.
    const seen = repository.built?.oid === built && repository.builtPlan ? idsOf(repository.builtPlan, '', LEVELS, new Map()) : null;
    const same = (file) => read.oids.get(file) === seen.get(file);
    const compared = new Set([path, path.replace(/\.json$/, '.md'), ...shownFiles(read.files, entry.id), ...(outcome.status === 'done' ? outcome.changed.keys() : [])]);
    if (!seen || ![...compared].every(same) || !(await shownAtBuild(parsePlan(read.files).plan.entries[entry.id] ?? {}, seen, read.oids, textOf)).every(same) || !await inHistory(call, repo, built, head)) {
      throw new VerdictRefusal(409, 'changed', CHANGED);
    }
    if (outcome.status === 'refused') throw new VerdictRefusal(409, 'refused', outcome.message);
    if (outcome.status === 'already') return { status: 'already', commit: null, message: outcome.message };

    // The plan the verdict leaves on main must pass every check of the site's build, or the deploy of main would
    // fail: the plan tool's check, the addresses the Workshop keeps, and the map's layout as this server was built with.
    const after = new Map(read.files);
    for (const [file, text] of outcome.changed) after.set(file, text);
    try {
      checkBuild(after, map, exists);
    } catch (error) {
      if (error instanceof PlanFileError) throw new VerdictRefusal(409, 'refused', error.message);
      throw error;
    }

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
      // 409: main is not where the call expected it.
      if (error.status === 409) continue;
      // Any other failure (no answer, none in time, a 5xx) may hide a commit made: main is read again.
      doubt.write = true;
      if (await moved(call, repo, head)) continue;
      throw error;
    }
    const oid = answered?.data?.createCommitOnBranch?.commit?.oid;
    if (typeof oid === 'string' && !answered.errors?.length) return { status: 'done', commit: oid, message: outcome.message };
    const failures = Array.isArray(answered?.errors) ? answered.errors : [];
    if (failures.some((error) => error?.type === 'RATE_LIMITED')) throw new GitHubError(429, { as: 'user', rateLimited: true, retryAfter: 60 });
    if (failures.some((error) => DENIED_TYPES.has(error?.type))) throw new VerdictRefusal(403, 'forbidden', DENIED);
    // The head was not the one expected: main moved, forward or back, even if it is back where it was read now.
    if (failures.length && failures.every(headMismatch)) continue;
    // The answer says nothing sure: read main again.
    doubt.write = true;
    if (await moved(call, repo, head)) continue;
    throw new GitHubError(502, { as: 'user' });
  }
  throw new VerdictRefusal(409, 'busy', BUSY);
}
