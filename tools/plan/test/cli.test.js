import { test } from 'node:test';
import assert from 'node:assert/strict';
import { chmodSync, mkdirSync, readFileSync, symlinkSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { basePlan, canonicalFile, cleanup, git, initRepo, run, temp, writePlan } from './helpers.js';
import { canonicalText } from '../model/canonical.js';

// The command commits with the identity and config of its environment: a fixed one, and no user config.
Object.assign(process.env, {
  GIT_AUTHOR_NAME: 'Test', GIT_AUTHOR_EMAIL: 'test@example.invalid', GIT_COMMITTER_NAME: 'Test', GIT_COMMITTER_EMAIL: 'test@example.invalid',
  GIT_CONFIG_GLOBAL: '/dev/null', GIT_CONFIG_SYSTEM: '/dev/null',
});

const readJson = (root, rel) => JSON.parse(readFileSync(join(root, 'plan', rel), 'utf8'));
const head = (root) => git(root, 'rev-parse', 'HEAD').trim();
const clean = (root) => git(root, 'status', '--porcelain') === '';

/** A git repository holding the sample plan, removed after the body. */
async function withRepo(body, files = basePlan()) {
  const root = writePlan(files);
  try {
    initRepo(root);
    await body(root);
  } finally {
    cleanup(root);
  }
}

test('plan --help lists the five commands; --json too', async () => {
  const { code, out } = await run(temp(), ['--help']);
  assert.equal(code, 0);
  for (const usage of ['plan ', 'plan show <id>', 'plan take <id>', 'plan drop <id>', 'plan check']) assert.ok(out.includes(`  ${usage}`), usage);
  const json = JSON.parse((await run(temp(), ['--help', '--json'])).out);
  assert.deepEqual(json.commands.map((item) => item.usage), ['plan', 'plan show <id>', 'plan take <id>', 'plan drop <id>', 'plan check']);
});

test('wrong use exits 2', async () => {
  await withRepo(async (root) => {
    for (const argv of [['nonsense'], ['show'], ['show', 'a', 'b'], ['take'], ['drop', 'a', 'b'], ['check', 'x'], ['check', '--nope'], ['next'], ['sync'], ['status'], ['--help', 'x']]) {
      assert.equal((await run(root, argv)).code, 2, argv.join(' '));
    }
  });
});

test('outside a plan the command refuses with 1', async () => {
  const dir = temp();
  try {
    const { code, err } = await run(dir, []);
    assert.equal(code, 1);
    assert.match(err, /no plan\/ found/);
  } finally {
    cleanup(dir);
  }
});

test('plan: a feature whose needs are all done is ready', async () => {
  const files = basePlan();
  files['engine/decisions/linux-vm-engine.json'] = { ...files['engine/decisions/linux-vm-engine.json'], verdict: 'take' };
  await withRepo(async (root) => {
    const json = JSON.parse((await run(root, ['--json'])).out);
    assert.deepEqual(json.ready, [{ id: 'start-computer', name: 'Start a computer' }]);
    assert.deepEqual(json.waiting, []);
    assert.match((await run(root, [])).out, /^Ready to build:\n {2}start-computer {2}Start a computer$/m);
  }, files);
});

test('plan: the first entry with no verdict, the features ready and those that wait', async () => {
  await withRepo(async (root) => {
    const { code, out } = await run(join(root, 'plan'), []);
    assert.equal(code, 0);
    assert.match(out, /^Next: decision nothing-without-a-reason, stage ground$/m);
    assert.match(out, /start-computer {2}waits for linux-vm-engine/);
    const json = JSON.parse((await run(root, ['--json'])).out);
    assert.equal(json.next.id, 'nothing-without-a-reason');
    assert.equal(json.next.kind, 'decision');
    assert.equal(json.next.asks, basePlan()['decisions/nothing-without-a-reason.json'].asks);
    assert.deepEqual(json.ready, []);
    assert.deepEqual(json.waiting, [{ id: 'start-computer', name: 'Start a computer', for: ['linux-vm-engine'] }]);
  });
});

test('show: an entry with its needs, what needs it and what binds it; a sentence with who adds and keeps it; an unknown id is 1', async () => {
  await withRepo(async (root) => {
    const entry = JSON.parse((await run(root, ['show', 'start-computer', '--json'])).out);
    assert.equal(entry.kind, 'feature');
    assert.equal(entry.status, 'taken');
    assert.equal(entry.stage, 'app');
    assert.deepEqual(entry.needs, [{ id: 'engine-language', status: 'taken' }, { id: 'linux-vm-engine', status: 'answered' }]);
    assert.ok(entry.binds.principles.some((p) => p.id === 'less-sure-less-power'));
    assert.ok(entry.binds.properties.some((p) => p.id === 'precaution-grants-nothing'));
    assert.deepEqual(entry.binds.decisions.map((d) => d.id).sort(), ['engine-language', 'the-design']);
    assert.ok(entry.notes);

    const needed = JSON.parse((await run(root, ['--json', 'show', 'linux-vm-engine'])).out);
    assert.deepEqual(needed.needed_by, [{ id: 'start-computer', status: 'taken' }]);

    const text = await run(root, ['show', 'start-computer']);
    assert.equal(text.code, 0);
    assert.match(text.out, /^feature start-computer \(engine\): taken, stage app, 8 of 8$/m);

    const sentence = JSON.parse((await run(root, ['show', 'precaution-grants-nothing', '--json'])).out);
    assert.equal(sentence.kind, 'property');
    assert.equal(sentence.list, 'record/properties');
    assert.equal(sentence.added_by, 'the-design');
    assert.deepEqual(sentence.kept_by, ['start-computer']);

    assert.equal((await run(root, ['show', 'nothing-here'])).code, 1);
  });
});

test('check: 0 on the sample, 1 with errors, --json, and an entry changed under its verdict against HEAD', async () => {
  await withRepo(async (root) => {
    const ok = await run(root, ['check']);
    assert.equal(ok.code, 0, ok.out + ok.err);
    assert.deepEqual(JSON.parse((await run(root, ['check', '--json'])).out).errors, []);

    const file = join(root, 'plan', 'engine', 'decisions', 'engine-language.json');
    const entry = readJson(root, 'engine/decisions/engine-language.json');
    writeFileSync(file, `${JSON.stringify({ ...entry, answer: 'Rust.' }, null, 2)}\n`);
    const changed = JSON.parse((await run(root, ['check', '--json'])).out);
    assert.equal(changed.ok, false);
    assert.ok(changed.errors.some((problem) => problem.file.includes('engine-language')));
    assert.equal((await run(root, ['check'])).code, 1);
  });
});

test('take commits only the changed files as "Owner: take <id>", and the check passes after', async () => {
  await withRepo(async (root) => {
    // A file staged outside plan/ stays staged and out of the verdict's commit.
    writeFileSync(join(root, 'NOTES.md'), 'unrelated\n');
    git(root, 'add', 'NOTES.md');
    const { code, out } = await run(root, ['take', 'losses-show']);
    assert.equal(code, 0, out);
    assert.equal(git(root, 'log', '-1', '--format=%s').trim(), 'Owner: take losses-show');
    assert.deepEqual(git(root, 'show', '--name-only', '--format=', 'HEAD').trim().split('\n').sort(), ['plan/record/decisions/losses-show.json', 'plan/record/properties.json']);
    assert.equal(git(root, 'status', '--porcelain'), 'A  NOTES.md\n');
    git(root, 'rm', '-q', '--cached', 'NOTES.md');
    git(root, 'clean', '-qf');
    assert.ok(clean(root));
    assert.equal(readJson(root, 'record/decisions/losses-show.json').verdict, 'take');
    assert.equal((await run(root, ['check'])).code, 0);

    const again = await run(root, ['take', 'losses-show']);
    assert.equal(again.code, 0);
    assert.match(again.out, /already taken/);
    assert.equal(git(root, 'log', '-1', '--format=%s').trim(), 'Owner: take losses-show');
  });
});

test('a refused take writes and commits nothing: no answer, uncommitted changes in plan/', async () => {
  await withRepo(async (root) => {
    const before = head(root);
    const noAnswer = await run(root, ['take', 'model-tool']);
    assert.equal(noAnswer.code, 1);
    assert.match(noAnswer.err, /no answer/);
    assert.equal(head(root), before);
    assert.ok(clean(root));

    writeFileSync(join(root, 'plan', 'engine', 'README.md'), 'Edited.\n');
    const dirty = await run(root, ['take', 'losses-show']);
    assert.equal(dirty.code, 1);
    assert.match(dirty.err, /uncommitted/);
    assert.equal(head(root), before);
    assert.equal(git(root, 'status', '--porcelain').trim(), 'M plan/engine/README.md');
  });
});

test('drop commits "Owner: drop <id>"; dropping a taken entry that brought sentences is refused', async () => {
  await withRepo(async (root) => {
    const { code } = await run(root, ['drop', 'no-detour']);
    assert.equal(code, 0);
    assert.equal(git(root, 'log', '-1', '--format=%s').trim(), 'Owner: drop no-detour');
    assert.equal(readJson(root, 'record/decisions/no-detour.json').verdict, 'drop');
    const before = head(root);
    assert.equal((await run(root, ['drop', 'the-design'])).code, 1);
    assert.equal(head(root), before);
    assert.ok(clean(root));
  });
});

test('take and drop outside git refuse', async () => {
  const root = writePlan(basePlan());
  try {
    const { code, err } = await run(root, ['take', 'losses-show']);
    assert.equal(code, 1);
    assert.match(err, /not a git checkout/);
  } finally {
    cleanup(root);
  }
});

/** Writes an entry file in canonical form. @param {string} root @param {string} rel @param {any} value */
const writeJson = (root, rel, value) => writeFileSync(join(root, 'plan', rel), canonicalText(canonicalFile(rel)(value)));

test('a symbolic link under plan/ stops every command, and take writes nothing outside plan/', async () => {
  const outside = temp('plan-outside-');
  const files = basePlan();
  const properties = canonicalText(canonicalFile('record/properties.json')(files['record/properties.json']));
  delete files['record/properties.json'];
  const root = writePlan(files);
  try {
    writeFileSync(join(outside, 'properties.json'), properties);
    symlinkSync(join(outside, 'properties.json'), join(root, 'plan', 'record', 'properties.json'));
    initRepo(root);
    const before = head(root);
    const taken = await run(root, ['take', 'losses-show']);
    assert.equal(taken.code, 1);
    assert.match(taken.err, /plan\/record\/properties\.json: -: a symbolic link/);
    assert.equal(readFileSync(join(outside, 'properties.json'), 'utf8'), properties);
    assert.equal(head(root), before);
    assert.ok(clean(root));
    for (const argv of [['check'], [], ['drop', 'no-detour'], ['show', 'the-design']]) assert.equal((await run(root, argv)).code, 1, argv.join(' '));
  } finally {
    cleanup(root);
    cleanup(outside);
  }
});

test('uncommitted changes are found whatever the settings: untracked files hidden by config', async () => {
  await withRepo(async (root) => {
    const before = head(root);
    git(root, 'config', 'status.showUntrackedFiles', 'no');
    writeFileSync(join(root, 'plan', 'engine', 'decisions', 'engine-language.md'), 'Notes, never committed.\n');
    const untracked = await run(root, ['take', 'losses-show']);
    assert.equal(untracked.code, 1);
    assert.match(untracked.err, /uncommitted changes \(plan\/engine\/decisions\/engine-language\.md\)/);
    assert.equal(head(root), before);
  });
});

test('uncommitted changes are found: a rename from a dot-named file to a file the reader reads', async () => {
  await withRepo(async (root) => {
    // Committed under a dot name, which the reader skips, then staged as a file it reads.
    writeFileSync(join(root, 'plan', '.draft'), 'Notes for the engine language.\n');
    git(root, 'add', 'plan/.draft');
    git(root, 'commit', '-q', '-m', 'Draft');
    git(root, 'mv', 'plan/.draft', 'plan/engine/decisions/engine-language.md');
    const before = head(root);
    const renamed = await run(root, ['take', 'losses-show']);
    assert.equal(renamed.code, 1);
    assert.match(renamed.err, /uncommitted changes \(plan\/engine\/decisions\/engine-language\.md\)/);
    assert.equal(head(root), before);
    // A dot-named file alone, which the reader skips too, does not count.
    git(root, 'mv', 'plan/engine/decisions/engine-language.md', 'plan/.draft');
    writeFileSync(join(root, 'plan', '.DS_Store'), 'x');
    assert.equal((await run(root, ['take', 'losses-show'])).code, 0);
  });
});

test('when git cannot say whether plan/ is clean, the verdict is refused', async () => {
  await withRepo(async (root) => {
    const before = head(root);
    writeFileSync(join(root, '.git', 'index'), 'not an index');
    const { code, err } = await run(root, ['take', 'losses-show']);
    assert.equal(code, 1);
    assert.match(err, /could not say whether plan\/ has uncommitted changes/);
    assert.equal(git(root, 'rev-parse', 'HEAD').trim(), before);
    assert.equal(readJson(root, 'record/decisions/losses-show.json').verdict, undefined);
    assert.equal(readJson(root, 'record/properties.json').length, 4);
  });
});

test('the files of a verdict are written all or none: when one cannot be written, the earlier ones are put back', async () => {
  await withRepo(async (root) => {
    const before = head(root);
    // take losses-show writes record/properties.json, then the entry: the entry cannot be written.
    const entry = join(root, 'plan', 'record', 'decisions', 'losses-show.json');
    chmodSync(entry, 0o444);
    try {
      const { code, err } = await run(root, ['take', 'losses-show']);
      assert.equal(code, 1);
      assert.match(err, /could not write plan\/record\/decisions\/losses-show\.json/);
      assert.match(err, /nothing changed/);
    } finally {
      chmodSync(entry, 0o644);
    }
    assert.equal(head(root), before);
    assert.ok(clean(root), git(root, 'status', '--porcelain'));
  });
});

test('rule 9 in the repository: an at that is a directory, or a link that leads outside, is an error', async () => {
  const outside = temp('plan-outside-');
  const files = basePlan();
  files['record/properties.json'][0].at = ['engine'];
  files['record/properties.json'][1].at = ['engine/out.zig'];
  files['record/properties.json'][2].at = ['engine/in.zig'];
  files['record/properties.json'][3].at = ['engine/real.zig'];
  const root = writePlan(files);
  try {
    writeFileSync(join(outside, 'out.zig'), 'x');
    mkdirSync(join(root, 'engine'));
    writeFileSync(join(root, 'engine', 'real.zig'), 'x');
    symlinkSync(join(outside, 'out.zig'), join(root, 'engine', 'out.zig'));
    symlinkSync('real.zig', join(root, 'engine', 'in.zig'));
    initRepo(root);
    const { code, out } = await run(root, ['check', '--json']);
    assert.equal(code, 1);
    assert.deepEqual(JSON.parse(out).errors.map((e) => `${e.field}: ${e.message}`), [
      '[0].at[0]: "engine" is not a regular file of the repository',
      '[1].at[0]: "engine/out.zig" is not a regular file of the repository',
    ]);
  } finally {
    cleanup(root);
    cleanup(outside);
  }
});

test('rule 11 in the command reads origin/main when it exists, else HEAD', async () => {
  await withRepo(async (root) => {
    git(root, 'update-ref', 'refs/remotes/origin/main', 'HEAD');
    const entry = readJson(root, 'engine/decisions/engine-language.json');
    writeJson(root, 'engine/decisions/engine-language.json', { ...entry, answer: 'Rust.' });
    git(root, 'commit', '-q', '-am', 'Rust');
    // HEAD holds the change: only a base of origin/main sees it.
    const changed = JSON.parse((await run(root, ['check', '--json'])).out);
    assert.deepEqual(changed.errors.map((e) => `${e.file} ${e.field}`), ['plan/engine/decisions/engine-language.json answer']);
    // A verdict on another entry is refused while the plan fails rule 11, and names the base.
    const taken = await run(root, ['take', 'losses-show']);
    assert.equal(taken.code, 1);
    assert.match(taken.err, /against origin\/main/);
    git(root, 'update-ref', '-d', 'refs/remotes/origin/main');
    assert.equal((await run(root, ['check'])).code, 0);
  });
});

test('rule 11: the owner\'s verdict after the change lifts the rule; a later plain edit brings it back', async () => {
  const files = basePlan();
  files['record/decisions/losses-show.json'] = { ...files['record/decisions/losses-show.json'], verdict: 'drop' };
  await withRepo(async (root) => {
    git(root, 'update-ref', 'refs/remotes/origin/main', 'HEAD');
    const name = 'record/decisions/losses-show.json';
    // On the branch: the verdict cleared and the question changed, then dropped again by the owner.
    const { verdict: _verdict, ...entry } = readJson(root, name);
    writeJson(root, name, { ...entry, asks: 'What does the record say when Silo missed a fact?' });
    git(root, 'commit', '-q', '-am', 'Reword losses-show');
    assert.equal((await run(root, ['check'])).code, 0);
    const dropped = await run(root, ['drop', 'losses-show']);
    assert.equal(dropped.code, 0, dropped.err);
    const ok = await run(root, ['check']);
    assert.equal(ok.code, 0, ok.out + ok.err);
    // A plain edit of the sentence after the owner's verdict: uncommitted, then committed.
    writeJson(root, name, { ...readJson(root, name), asks: 'What does the record say when Silo missed a report?' });
    const edited = JSON.parse((await run(root, ['check', '--json'])).out);
    assert.deepEqual(edited.errors.map((e) => `${e.file} ${e.field}`), [`plan/${name} asks`]);
    git(root, 'commit', '-q', '-am', 'Reword again');
    assert.equal((await run(root, ['check'])).code, 1);
    // A commit by the owner on another entry does not lift it.
    assert.equal((await run(root, ['drop', 'no-detour'])).code, 1);
  }, files);
});
