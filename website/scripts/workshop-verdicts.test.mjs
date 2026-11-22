// The owner's verdict from the Workshop (`POST verdict`, server/verdicts.mjs) against the fake GitHub's git data:
// the commit it puts on main, who makes it, and every refusal, with nothing half written.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { mkdir, mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { drop, take } from '../../tools/plan/write/index.js';
import { loadDir } from '../../tools/plan/read/index.js';
import { ATTEMPTS, BUSY, CHANGED, DENIED, PARTIAL } from '../server/verdicts.mjs';
import { BUDGET } from '../server/workshop.mjs';
import { planEntries } from './plan-entries.mjs';
import { ORIGIN, setup, withBudget } from './workshop-test-support.mjs';

const repository = fileURLToPath(new URL('../..', import.meta.url));
const SAMPLE = join(repository, 'tools', 'plan', 'test', 'fixtures', 'sample');
const PLAN_BIN = join(repository, 'tools', 'plan', 'bin', 'plan');

/** The sample plan's files, by path relative to plan/; `change` replaces some. */
function sampleFiles(change = {}) {
  const files = loadDir(SAMPLE);
  for (const [name, text] of Object.entries(change)) files.set(name, text);
  return files;
}

/** The workshop on the fake GitHub, its main one commit holding the sample plan (`change` applied), and owner-fake signed in. */
async function started({ change = {}, env } = {}) {
  const files = sampleFiles(change);
  const t = await setup({ entries: planEntries(SAMPLE), ...(env ? { env } : {}) });
  const seed = t.fake.git.seed(new Map([...files].map(([name, text]) => [`plan/${name}`, text])));
  const owner = t.client();
  await owner.signIn('owner-fake');
  const verdict = (entry, kind, built = seed, who = owner, extra = {}) => who.call('POST', 'verdict', { body: { entry, verdict: kind, built, ...extra } });
  const main = () => t.fake.git.refs.get('heads/main');
  const gitWrites = () => t.fake.state.requests.filter(isWrite);
  return { ...t, files, seed, owner, verdict, main, gitWrites };
}

/** The one call that writes: the commit and main's move together (GraphQL createCommitOnBranch). */
const isWrite = (record) => record.graphql === 'createCommitOnBranch';

/** The files of a commit's plan/, by path relative to plan/. */
const planAt = (git, commit) => new Map([...git.files(git.objects.get(commit).tree)].filter(([path]) => path.startsWith('plan/')).map(([path, text]) => [path.slice('plan/'.length), text]));

/** What a commit changed in plan/ against its parent: path -> new text. */
function changedBy(git, commit) {
  const after = planAt(git, commit);
  const before = planAt(git, git.objects.get(commit).parents[0]);
  return new Map([...after].filter(([path, text]) => before.get(path) !== text));
}

test('a take is one commit on main, named as plan take names it, made with the owner\'s token, changing what take() changes', async () => {
  const t = await started();
  try {
    const id = 'nothing-without-a-reason';
    const response = await t.verdict(id, 'take');
    assert.equal(response.status, 200, response.body);
    assert.deepEqual(response.json, { ok: true, entry: id, verdict: 'take', commit: t.main(), message: `took ${id}` });
    const commit = t.fake.git.objects.get(t.main());
    assert.equal(commit.message, `Owner: take ${id}`);
    assert.deepEqual(commit.parents, [t.seed], 'its only parent is the old main');
    assert.equal(commit.madeAs, 'user');
    assert.equal(commit.author, 'owner-fake');
    const expected = take(t.files, id).changed;
    assert.deepEqual([...changedBy(t.fake.git, t.main())].sort(), [...expected].sort());
    assert.deepEqual([...expected.keys()].sort(), [`decisions/${id}.json`, 'principles.json']);
    // One write, the person's; the installation token never writes contents. The subject alone, no body, as
    // `git commit -m` gives it, made on the main that was read and only while it is still main's head.
    assert.ok(t.gitWrites().length === 1 && t.gitWrites().every((r) => r.kind === 'user' && r.login === 'owner-fake'), JSON.stringify(t.gitWrites()));
    assert.deepEqual(t.fake.state.commitInputs.map((input) => [input.message, input.expectedHeadOid]), [[{ headline: `Owner: take ${id}` }, t.seed]]);
    // Reading plan/ is one GraphQL call and one ref read, not a call per file.
    const reads = t.fake.state.requests.filter((r) => r.path.includes('/git/') && r.method === 'GET');
    assert.deepEqual(reads.map((r) => r.path.split('/git/')[1]), ['ref/heads/main']);
    // Taken again: already, and nothing is written.
    const again = await t.verdict(id, 'take', t.seed);
    assert.equal(again.status, 409, 'the page built before the take saw another file');
    assert.equal((await t.verdict('engine-language', 'take')).json.already, true);
  } finally { await t.close(); }
});

test('a drop, and a take or drop on what the agents decided: the plain verdict confirms it, without by', async () => {
  const agents = (verdict) => JSON.stringify({ asks: 'Which tool checks the model of access?', answer: 'Alloy.', verdict, by: 'agents' }, null, 2) + '\n';
  for (const [before, kind] of [['take', 'take'], ['drop', 'drop'], ['take', 'drop'], ['drop', 'take']]) {
    const t = await started({ change: { 'engine/decisions/model-tool.json': agents(before) } });
    try {
      const response = await t.verdict('model-tool', kind);
      assert.equal(response.status, 200, response.body);
      const changed = changedBy(t.fake.git, t.main());
      assert.deepEqual([...changed.keys()], ['engine/decisions/model-tool.json']);
      const file = JSON.parse(changed.get('engine/decisions/model-tool.json'));
      assert.equal(file.verdict, kind);
      assert.equal('by' in file, false, `${before} by agents, then the owner's ${kind}`);
      assert.equal(t.fake.git.objects.get(t.main()).message, `Owner: ${kind} model-tool`);
    } finally { await t.close(); }
  }
  const t = await started();
  try {
    assert.equal((await t.verdict('model-tool', 'drop')).status, 200);
    assert.deepEqual([...changedBy(t.fake.git, t.main())], [...drop(t.files, 'model-tool').changed]);
  } finally { await t.close(); }
});

const GIT_ENV = {
  ...process.env, GIT_CONFIG_GLOBAL: '/dev/null', GIT_CONFIG_NOSYSTEM: '1',
  GIT_AUTHOR_NAME: 'Owner', GIT_AUTHOR_EMAIL: 'owner@example.invalid', GIT_COMMITTER_NAME: 'Owner', GIT_COMMITTER_EMAIL: 'owner@example.invalid',
};
const git = (dir, ...args) => execFileSync('git', ['-C', dir, ...args], { env: GIT_ENV, encoding: 'utf8' }).trim();

/** Writes `files` (path -> text) as the whole working tree of `dir` and commits them with `message`. */
async function commitFiles(dir, files, message) {
  git(dir, 'rm', '-rq', '--ignore-unmatch', '.');
  for (const [path, text] of files) {
    await mkdir(dirname(join(dir, path)), { recursive: true });
    await writeFile(join(dir, path), text);
  }
  git(dir, 'add', '-A');
  git(dir, 'commit', '-qm', message);
}

test('the commit cannot be told apart from one plan take made: the same tree, and the real plan check passes on it', async () => {
  const t = await started();
  const dir = await mkdtemp(join(tmpdir(), 'workshop-verdict-'));
  try {
    const id = 'losses-show';
    assert.equal((await t.verdict(id, 'take')).status, 200);
    const { git: fake } = t.fake;
    const made = fake.objects.get(t.main());
    git(dir, 'init', '-q', '-b', 'main');
    await commitFiles(dir, fake.files(fake.objects.get(t.seed).tree), 'Seed');
    assert.equal(git(dir, 'rev-parse', 'HEAD^{tree}'), fake.objects.get(t.seed).tree, 'the fake hashes trees as git does');
    const seed = git(dir, 'rev-parse', 'HEAD');
    // The real command on the same main.
    git(dir, 'update-ref', 'refs/remotes/origin/main', seed);
    git(dir, 'checkout', '-qb', 'by-the-command');
    execFileSync(process.execPath, [PLAN_BIN, 'take', id], { cwd: dir, env: GIT_ENV, encoding: 'utf8' });
    assert.equal(git(dir, 'log', '-1', '--format=%B'), made.message, 'the same message, body included');
    assert.equal(git(dir, 'rev-parse', 'HEAD^{tree}'), made.tree, 'the same files, byte for byte');
    // The fake's commit, replayed on main, passes the real plan check against the old main.
    git(dir, 'checkout', '-q', 'main');
    await commitFiles(dir, fake.files(made.tree), made.message);
    assert.equal(git(dir, 'rev-parse', 'HEAD^{tree}'), made.tree);
    const out = execFileSync(process.execPath, [PLAN_BIN, 'check'], { cwd: dir, env: GIT_ENV, encoding: 'utf8' });
    assert.match(out, /plan check: ok/);
  } finally {
    await t.close();
    await rm(dir, { recursive: true, force: true });
  }
});

test('only the owner: a visitor sees no owner in me and is refused, and without WORKSHOP_OWNER_ID nobody is the owner', async () => {
  const t = await started();
  try {
    assert.equal((await t.owner.call('GET', 'me')).json.owner, true);
    const visitor = t.client();
    await visitor.signIn('visitor-fake');
    assert.equal('owner' in (await visitor.call('GET', 'me')).json, false);
    const refused = await t.verdict('linux-vm-engine', 'take', t.seed, visitor);
    assert.deepEqual([refused.status, refused.json.error], [403, 'not_owner']);
    const anonymous = await t.verdict('linux-vm-engine', 'take', t.seed, t.client());
    assert.equal(anonymous.status, 401);
    assert.equal(t.main(), t.seed);
    assert.equal(t.gitWrites().length, 0);
  } finally { await t.close(); }
  const none = await started({ env: { WORKSHOP_OWNER_ID: undefined } });
  try {
    assert.equal('owner' in (await none.owner.call('GET', 'me')).json, false);
    const refused = await none.verdict('linux-vm-engine', 'take');
    assert.deepEqual([refused.status, refused.json.error], [403, 'not_owner']);
    assert.equal(none.fake.state.requests.filter((r) => r.path.includes('/git/') || r.path === '/graphql').length, 0, 'nothing asked of GitHub');
  } finally { await none.close(); }
});

test('the page sends only the id, the verdict and its build; anything else, a bad value or another site is refused', async () => {
  const t = await started();
  try {
    const cases = [
      [{ entry: 'linux-vm-engine', verdict: 'take', built: t.seed, path: 'plan/x.json' }, {}, 400, 'bad_input'],
      [{ entry: 'linux-vm-engine', verdict: 'take', built: t.seed, message: 'Owner: take x' }, {}, 400, 'bad_input'],
      [{ entry: 'linux-vm-engine', verdict: 'later', built: t.seed }, {}, 400, 'bad_input'],
      [{ entry: 'linux-vm-engine', verdict: 'take', built: 'main' }, {}, 400, 'bad_input'],
      [{ entry: 'linux-vm-engine', verdict: 'take' }, {}, 400, 'bad_input'],
      [{ entry: ['linux-vm-engine'], verdict: 'take', built: t.seed }, {}, 400, 'bad_input'],
      [{ entry: 'no-such-entry', verdict: 'take', built: t.seed }, {}, 404, 'not_found'],
      [{ entry: 'linux-vm-engine', verdict: 'take', built: t.seed }, { origin: 'https://evil.test' }, 403, 'bad_origin'],
      [{ entry: 'linux-vm-engine', verdict: 'take', built: t.seed }, { 'x-workshop': undefined }, 403, 'bad_request_header'],
    ];
    for (const [body, headers, status, code] of cases) {
      const response = await t.owner.call('POST', 'verdict', { body, headers });
      assert.deepEqual([response.status, response.json.error], [status, code], JSON.stringify(body));
    }
    assert.equal(t.main(), t.seed);
    assert.equal(t.gitWrites().length, 0);
    await withBudget({ verdictsPerMinute: 1 }, async () => {
      assert.equal((await t.verdict('model-tool', 'drop')).status, 200);
      const limited = await t.verdict('no-detour', 'drop');
      assert.deepEqual([limited.status, limited.json.error], [429, 'rate_limited']);
    });
    assert.equal(BUDGET.verdictsPerMinute, 6);
  } finally { await t.close(); }
});

test('an entry changed on main since the page was built is refused; a change to another file is not', async () => {
  const t = await started();
  try {
    const path = 'plan/engine/decisions/linux-vm-engine.json';
    const other = t.fake.git.push(new Map([['plan/engine/decisions/model-tool.json', '{\n  "asks": "Which tool checks the model of access, now?"\n}\n']]));
    const fine = await t.verdict('no-detour', 'drop');
    assert.equal(fine.status, 200, fine.body);
    assert.deepEqual(t.fake.git.objects.get(t.main()).parents, [other], 'built on main as it is now');

    const t2 = await started();
    try {
      const moved = t2.fake.git.push(new Map([[path, '{\n  "asks": "Which VM engine runs Linux computers?",\n  "answer": "libkrun."\n}\n']]));
      const refused = await t2.verdict('linux-vm-engine', 'take');
      assert.deepEqual([refused.status, refused.json.error, refused.json.message], [409, 'changed', CHANGED]);
      assert.equal(t2.main(), moved);
      // A build from a commit GitHub does not hold vouches for nothing.
      const unknown = await t2.verdict('linux-vm-engine', 'take', 'f'.repeat(40));
      assert.deepEqual([unknown.status, unknown.json.error], [409, 'changed']);
      assert.equal(t2.main(), moved);
      assert.equal(t2.gitWrites().length, 0);
    } finally { await t2.close(); }
  } finally { await t.close(); }
});

test('main moving during the write: done again on the new main, then 409 after a bounded number of tries, never forced', async () => {
  const t = await started();
  try {
    let pushes = 0;
    const agentPush = (times) => (record) => {
      if (isWrite(record) && pushes < times) {
        pushes += 1;
        t.fake.git.push(new Map([[`plan/engine/README.md`, `An agent's push ${pushes}.\n`]]));
      }
      return null;
    };
    t.fake.state.hook = agentPush(1);
    const response = await t.verdict('linux-vm-engine', 'take');
    assert.equal(response.status, 200, response.body);
    const commit = t.fake.git.objects.get(t.main());
    assert.equal(commit.message, 'Owner: take linux-vm-engine');
    assert.equal(t.fake.git.objects.get(commit.parents[0]).message, 'A push', 'redone on the new main');
    assert.equal(planAt(t.fake.git, t.main()).get('engine/README.md'), 'An agent\'s push 1.\n', 'the agent\'s push is kept');
    assert.equal(t.gitWrites().length, 2);

    pushes = 0;
    t.fake.state.hook = agentPush(Infinity);
    const before = t.fake.state.requests.length;
    const busy = await t.verdict('no-detour', 'drop');
    assert.deepEqual([busy.status, busy.json.error, busy.json.message], [409, 'busy', BUSY]);
    assert.equal(t.fake.state.requests.slice(before).filter(isWrite).length, ATTEMPTS);
    assert.equal(t.fake.git.objects.get(t.main()).message, 'A push', 'main is the agents\' pushes alone');
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('main reset to an older commit while the verdict is written: the commits removed stay removed, and the verdict is redone on the main as it is', async () => {
  const t = await started();
  try {
    const removed = t.fake.git.push(new Map([['plan/engine/README.md', 'A push that is then undone.\n']]));
    let reset = false;
    t.fake.state.hook = (record) => {
      if (isWrite(record) && !reset) {
        reset = true;
        t.fake.git.refs.set('heads/main', t.seed);
      }
      return null;
    };
    const response = await t.verdict('linux-vm-engine', 'take');
    assert.equal(response.status, 200, response.body);
    const commit = t.fake.git.objects.get(t.main());
    assert.equal(commit.message, 'Owner: take linux-vm-engine');
    assert.deepEqual(commit.parents, [t.seed], 'made on main as the reset left it');
    assert.equal(t.fake.git.isAncestor(removed, t.main()), false, 'the commit the reset removed is not put back');
    assert.equal(t.gitWrites().length, 2);
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('GitHub answering 409 to the write is main moving: redone, then 409 after a bounded number of tries', async () => {
  const t = await started();
  try {
    let conflicts = 1;
    t.fake.state.hook = (record) => {
      if (!isWrite(record) || conflicts === 0) return null;
      conflicts -= 1;
      return { status: 409, body: { message: 'Conflict' } };
    };
    const response = await t.verdict('linux-vm-engine', 'take');
    assert.equal(response.status, 200, response.body);
    assert.equal(t.fake.git.objects.get(t.main()).message, 'Owner: take linux-vm-engine');
    assert.equal(t.gitWrites().length, 2);

    conflicts = Infinity;
    const before = t.fake.state.requests.length;
    const busy = await t.verdict('no-detour', 'drop', t.main());
    assert.deepEqual([busy.status, busy.json.error, busy.json.message], [409, 'busy', BUSY]);
    assert.equal(t.fake.state.requests.slice(before).filter(isWrite).length, ATTEMPTS);
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('a refusal of the plan tool reaches the page as the tool gives it, and nothing is written', async () => {
  const t = await started();
  try {
    const refused = await t.verdict('model-tool', 'take');
    assert.deepEqual([refused.status, refused.json.error], [409, 'refused']);
    assert.equal(refused.json.message, take(t.files, 'model-tool').message);
    assert.equal(refused.json.message, 'model-tool has no answer and sets nothing: write its answer first');
    assert.equal(t.main(), t.seed);
    assert.equal(t.gitWrites().length, 0);
  } finally { await t.close(); }
});

test('GitHub refusing the write (the App without Contents: write, FORBIDDEN or NOT_FOUND, or a 403 to the call) leaves main as it was', async () => {
  for (const status of ['FORBIDDEN', 'NOT_FOUND']) {
    const t = await started();
    try {
      t.fake.state.contents = 'read';
      t.fake.state.contentsDenied = status;
      const response = await t.verdict('linux-vm-engine', 'take');
      assert.deepEqual([response.status, response.json.error, response.json.message], [403, 'forbidden', DENIED], `GitHub ${status}`);
      assert.equal(t.main(), t.seed);
    } finally { await t.close(); }
  }
  const t = await started();
  try {
    t.fake.state.hook = (record) => (isWrite(record) ? { status: 403, body: { message: 'Resource not accessible by integration' } } : null);
    const response = await t.verdict('linux-vm-engine', 'take');
    assert.deepEqual([response.status, response.json.error], [403, 'forbidden']);
    assert.equal(t.main(), t.seed);
    t.fake.state.hook = (record) => (isWrite(record) ? { status: 500 } : null);
    assert.equal((await t.verdict('linux-vm-engine', 'take')).status, 502);
    assert.equal(t.main(), t.seed);
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('files GitHub cuts in the GraphQL answer are read whole from their blobs', async () => {
  const t = await started();
  try {
    t.fake.state.truncateOver = 200;
    const response = await t.verdict('nothing-without-a-reason', 'take');
    assert.equal(response.status, 200, response.body);
    assert.deepEqual([...changedBy(t.fake.git, t.main())].sort(), [...take(t.files, 'nothing-without-a-reason').changed].sort());
    assert.ok(t.fake.state.requests.some((r) => r.path.includes('/git/blobs/')));
  } finally { await t.close(); }
});

test('the session must be the owner\'s on every write: a cross-site POST never reaches GitHub', async () => {
  const t = await started();
  try {
    const before = t.fake.state.requests.length;
    const response = await t.owner.call('POST', 'verdict', { body: { entry: 'linux-vm-engine', verdict: 'take', built: t.seed }, headers: { origin: `${ORIGIN}.evil.test` } });
    assert.equal(response.status, 403);
    assert.equal(t.fake.state.requests.length, before);
  } finally { await t.close(); }
});

// Model: random verdicts by the owner and a visitor, with the page's build any earlier main, agents pushing to main
// (between requests and during the write) and GitHub refusing the write. After every step main is checked
// against a reference: it moves only by a fast-forward, by the agents' pushes or by one commit per verdict the server
// answered as done, named `Owner: <verdict> <id>`, whose only parent is the main before it and whose changes are
// exactly what take() or drop() gives on that parent; every refusal leaves main as the agents left it.
test('model: main moves only by fast-forwards, and each verdict is one commit equal to take() or drop() on its parent', async () => {
  const t = await started();
  try {
    const visitor = t.client();
    await visitor.signIn('visitor-fake');
    const ids = planEntries(SAMPLE).map((entry) => entry.id);
    const pathOf = new Map(planEntries(SAMPLE).map((entry) => [entry.id, `${entry.part ? `${entry.part}/` : ''}${entry.kind === 'decision' ? 'decisions' : 'features'}/${entry.id}.json`]));
    let seed = 2026;
    const rand = (k) => { seed = (seed * 1103515245 + 12345) % 2147483648; return Math.floor((seed / 2147483648) * k); };
    const mains = [t.seed];
    const ours = new Set();
    let pushes = 0;
    const agentPush = (path) => {
      pushes += 1;
      return t.fake.git.push(new Map([[path, `Push ${pushes}.\n`]]));
    };
    const reached = { done: 0, already: 0, refused: 0, changed: 0, forbidden: 0, notOwner: 0, raced: 0 };
    for (let step = 0; step < 200; step += 1) {
      // A minute between steps, so the verdicts' own ceiling never answers.
      t.clock.t += 61_000;
      const before = t.main();
      const op = rand(10);
      if (op === 0) {
        // An agent pushes notes, or edits an entry's text: a page built before it no longer vouches for that entry.
        const id = ids[rand(ids.length)];
        const path = rand(2) ? `plan/${pathOf.get(id).replace(/\.json$/, '.md')}` : 'plan/engine/README.md';
        agentPush(path);
        mains.push(t.main());
        continue;
      }
      if (op === 1) {
        const id = ids[rand(ids.length)];
        const response = await t.verdict(id, rand(2) ? 'take' : 'drop', mains[rand(mains.length)], visitor);
        assert.deepEqual([response.status, response.json.error], [403, 'not_owner']);
        reached.notOwner += 1;
        assert.equal(t.main(), before);
        continue;
      }
      const id = ids[rand(ids.length)];
      const kind = rand(2) ? 'take' : 'drop';
      const built = mains[rand(mains.length)];
      // Sometimes an agent pushes while the ref is updated, or GitHub refuses the update.
      const trouble = rand(4);
      let racedTo = null;
      t.fake.state.hook = (record) => {
        if (!isWrite(record)) return null;
        t.fake.state.hook = null;
        if (trouble === 0) return { status: 403, body: { message: 'Resource not accessible by integration' } };
        if (trouble === 1) {
          racedTo = agentPush('plan/engine/README.md');
          reached.raced += 1;
        }
        return null;
      };
      const response = await t.verdict(id, kind, built);
      t.fake.state.hook = null;
      const base = racedTo ?? before;
      const files = planAt(t.fake.git, base);
      const path = pathOf.get(id);
      const outcome = (kind === 'take' ? take : drop)(files, id);
      const label = `${kind} ${id} at step ${step}`;
      if (files.get(path) !== planAt(t.fake.git, built).get(path)) {
        assert.deepEqual([response.status, response.json.error], [409, 'changed'], label);
        reached.changed += 1;
      } else if (outcome.status === 'refused') {
        assert.deepEqual([response.status, response.json.error, response.json.message], [409, 'refused', outcome.message], label);
        reached.refused += 1;
      } else if (outcome.status === 'already') {
        assert.deepEqual([response.status, response.json.already, response.json.commit], [200, true, null], label);
        reached.already += 1;
      } else if (trouble === 0) {
        assert.deepEqual([response.status, response.json.error], [403, 'forbidden'], label);
        reached.forbidden += 1;
      } else {
        assert.equal(response.status, 200, `${label}: ${response.body}`);
        const commit = t.fake.git.objects.get(t.main());
        assert.equal(response.json.commit, t.main());
        assert.equal(commit.message, `Owner: ${kind} ${id}`, label);
        assert.deepEqual(commit.parents, [base], label);
        assert.deepEqual([...changedBy(t.fake.git, t.main())].sort(), [...outcome.changed].sort(), label);
        ours.add(t.main());
        reached.done += 1;
      }
      if (response.status !== 200 || response.json.already) assert.equal(t.main(), base, `${label}: nothing reached main`);
      mains.push(t.main());
      // main's history is one line back to the seed, made of the agents' pushes and the server's verdicts.
      for (let sha = t.main(); sha !== t.seed;) {
        const commit = t.fake.git.objects.get(sha);
        assert.equal(commit.parents.length, 1);
        assert.ok(commit.message === 'A push' || ours.has(sha), `an unexpected commit at step ${step}`);
        sha = commit.parents[0];
      }
    }
    // Every case was reached, so no rule was checked on nothing.
    for (const [name, times] of Object.entries(reached)) assert.ok(times > 0, `${name} was never reached: ${JSON.stringify(reached)}`);
  } finally { t.fake.state.hook = null; await t.close(); }
});

test('an answer GitHub gives in part fails the whole read, nothing written, and says so: errors beside data, a folder without its entries, a file without its object', async () => {
  /** The folder `engine/decisions` of plan/ in a PlanAt answer. */
  const decisions = (answer) => answer.data.repository.plan.entries.find((e) => e.name === 'engine').object.entries.find((e) => e.name === 'decisions');
  const cases = [
    ['errors beside data', (answer) => {
      decisions(answer).object = null;
      return { ...answer, errors: [{ type: 'SERVICE_UNAVAILABLE', path: ['repository', 'plan'], message: 'Something went wrong while executing your query.' }] };
    }],
    ['a folder whose entries are missing', (answer) => {
      delete decisions(answer).object.entries;
      return answer;
    }],
    ['a file whose object is null', (answer) => {
      decisions(answer).object.entries.find((e) => e.name === 'model-tool.json').object = null;
      return answer;
    }],
  ];
  for (const [name, change] of cases) {
    const t = await started();
    try {
      t.fake.state.planAnswer = change;
      const response = await t.verdict('linux-vm-engine', 'take');
      assert.deepEqual([response.status, response.json.error, response.json.message], [502, 'partial', PARTIAL], name);
      assert.equal(t.main(), t.seed, name);
      assert.equal(t.gitWrites().length, 0, name);
    } finally { await t.close(); }
  }
});
