import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { basePlan, cleanup, git, initRepo, run, temp, writePlan } from './helpers.js';

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
    const { code, out } = await run(root, ['take', 'losses-show']);
    assert.equal(code, 0, out);
    assert.equal(git(root, 'log', '-1', '--format=%s').trim(), 'Owner: take losses-show');
    assert.deepEqual(git(root, 'show', '--name-only', '--format=', 'HEAD').trim().split('\n').sort(), ['plan/record/decisions/losses-show.json', 'plan/record/properties.json']);
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
