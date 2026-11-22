import { test } from 'node:test';
import assert from 'node:assert/strict';
import { chmodSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
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
/** Writes a file of plan/ in canonical form. @param {string} root @param {string} rel @param {any} value */
const writeJson = (root, rel, value) => writeFileSync(join(root, 'plan', rel), canonicalText(canonicalFile(rel)(value)));

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
  for (const usage of ['plan ', 'plan show <id>', 'plan take <id> [--by agents]', 'plan drop <id> [--by agents]', 'plan check']) assert.ok(out.includes(`  ${usage}`), usage);
  const json = JSON.parse((await run(temp(), ['--help', '--json'])).out);
  assert.deepEqual(json.commands.map((item) => item.usage), ['plan', 'plan show <id>', 'plan take <id> [--by agents]', 'plan drop <id> [--by agents]', 'plan check']);
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

test('a plan that is a symbolic link stops the search for the root with the reader\'s error, dangling or not: no plan higher up is used', async () => {
  const outer = writePlan(basePlan());
  const target = writePlan(basePlan());
  try {
    initRepo(outer);
    const sub = join(outer, 'sub');
    mkdirSync(sub);
    for (const link of [join(outer, 'missing'), join(target, 'plan')]) {
      symlinkSync(link, join(sub, 'plan'));
      for (const argv of [['check'], [], ['show', 'the-design'], ['take', 'losses-show']]) {
        const { code, out, err } = await run(sub, argv);
        assert.equal(code, 1, `${link} ${argv.join(' ')}: ${out}`);
        assert.match(err, /plan\/: -: a symbolic link/, `${link} ${argv.join(' ')}`);
      }
      rmSync(join(sub, 'plan'));
    }
    assert.ok(clean(outer));
    assert.equal(git(outer, 'log', '-1', '--format=%s').trim(), 'Plan');
  } finally {
    cleanup(outer);
    cleanup(target);
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

test('plan: a feature is in one list only, and the lists come in order: being built, ready to build, waiting', async () => {
  const files = basePlan();
  files['engine/decisions/linux-vm-engine.json'] = { ...files['engine/decisions/linux-vm-engine.json'], verdict: 'take' };
  // start-computer is being built, list-computers is ready (its needs are done, its build has not started),
  // stop-computer waits for list-computers. They stand in the plan in the opposite order of the lists.
  files['engine/features/start-computer.json'] = { ...files['engine/features/start-computer.json'], started: '2026-11-18' };
  files['engine/features/list-computers.json'] = { name: 'List computers', what: 'Show the computers there are.', needs: ['linux-vm-engine'], verdict: 'take' };
  files['engine/features/stop-computer.json'] = { name: 'Stop a computer', what: 'Stop a computer that runs.', needs: ['list-computers'], verdict: 'take' };
  files['order.json'] = { ...files['order.json'], stages: files['order.json'].stages.map((stage) => (stage.id === 'app' ? { ...stage, entries: ['stop-computer', 'list-computers', 'start-computer'] } : stage)) };
  await withRepo(async (root) => {
    const json = JSON.parse((await run(root, ['--json'])).out);
    assert.deepEqual(json.building, [{ id: 'start-computer', name: 'Start a computer' }]);
    assert.deepEqual(json.ready, [{ id: 'list-computers', name: 'List computers' }]);
    assert.deepEqual(json.waiting, [{ id: 'stop-computer', name: 'Stop a computer', for: ['list-computers'] }]);
    const { out } = await run(root, []);
    const at = (text) => {
      const index = out.indexOf(text);
      assert.notEqual(index, -1, `${text} is not listed:\n${out}`);
      assert.equal(out.indexOf(text, index + 1), -1, `${text} is listed twice:\n${out}`);
      return index;
    };
    const lists = ['Being built:', 'Ready to build:', 'Waiting:'].map(at);
    assert.deepEqual(lists, [...lists].sort((a, b) => a - b));
    const [building, ready, waiting] = lists;
    const [started, list, stop] = ['start-computer  Start a computer', 'list-computers  List computers', 'stop-computer  waits for list-computers'].map(at);
    assert.ok(building < started && started < ready, 'start-computer is under Being built');
    assert.ok(ready < list && list < waiting, 'list-computers is under Ready to build');
    assert.ok(waiting < stop, 'stop-computer is under Waiting');
    assert.match((await run(root, ['show', 'start-computer'])).out, /^feature start-computer \(engine\): building, /);
    assert.equal(JSON.parse((await run(root, ['show', 'start-computer', '--json'])).out).status, 'building');
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
    assert.deepEqual(json.building, []);
    assert.match(out, /^Being built: none\.$/m);
    assert.deepEqual(json.ready, []);
    assert.deepEqual(json.waiting, [{ id: 'start-computer', name: 'Start a computer', for: ['linux-vm-engine'] }]);
  });
});

test('plan: the next decision with no answer shows the statements it sets; with an answer, the answer', async () => {
  const files = basePlan();
  const { answer: _answer, ...onlySets } = files['decisions/nothing-without-a-reason.json'];
  files['decisions/nothing-without-a-reason.json'] = onlySets;
  await withRepo(async (root) => {
    const { code, out } = await run(root, []);
    assert.equal(code, 0);
    assert.match(out, /^ {2}sets:\n {4}principles: least-privilege: Nothing is given without a reason: no access, no visibility, no resource\.$/m);
    assert.doesNotMatch(out, /no answer yet/);
    const json = JSON.parse((await run(root, ['--json'])).out);
    assert.deepEqual(json.next.sets, onlySets.sets);
    assert.equal(json.next.answer, undefined);
  }, files);
  await withRepo(async (root) => {
    const { out } = await run(root, []);
    assert.match(out, /^ {2}answer: Nothing without a reason/m);
    assert.doesNotMatch(out, /sets:/);
  });
});

test('show: an entry with its needs, what needs it and what binds it; a statement with who adds and keeps it; an unknown id is 1', async () => {
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

    const statement = JSON.parse((await run(root, ['show', 'precaution-grants-nothing', '--json'])).out);
    assert.equal(statement.kind, 'property');
    assert.equal(statement.list, 'record/properties');
    assert.equal(statement.added_by, 'the-design');
    assert.deepEqual(statement.kept_by, ['start-computer']);

    assert.equal((await run(root, ['show', 'nothing-here'])).code, 1);
  });
});

test('show on a part: its taken entries as ids, then the ones with no verdict in order with the next marked, then its properties', async () => {
  await withRepo(async (root) => {
    const json = JSON.parse((await run(root, ['show', 'engine', '--json'])).out);
    assert.equal(json.kind, 'part');
    assert.equal(json.title, 'The engine');
    assert.deepEqual(json.taken, ['engine-language', 'start-computer']);
    assert.deepEqual(json.dropped, []);
    assert.equal(json.next, 'model-tool');
    assert.deepEqual(json.open, [
      { id: 'model-tool', kind: 'decision', status: 'open', text: 'Which tool checks the model of access?' },
      { id: 'linux-vm-engine', kind: 'decision', status: 'answered', text: 'Which VM engine runs Linux computers, on a Mac and on a Linux host?' },
    ]);
    assert.deepEqual(json.properties, []);

    const { code, out } = await run(root, ['show', 'engine']);
    assert.equal(code, 0);
    assert.equal(out, [
      'part engine: The engine',
      'What runs the computers, and what it is built with.',
      '',
      'Taken: 2',
      '  engine-language, start-computer',
      '',
      'To decide, in order:',
      'next  model-tool       decision  open       Which tool checks the model of access?',
      '      linux-vm-engine  decision  answered   Which VM engine runs Linux computers, on a Mac and on a Linux host?',
      '',
      'Properties: none.',
    ].join('\n'));

    const record = JSON.parse((await run(root, ['show', 'record', '--json'])).out);
    assert.deepEqual(record.taken, []);
    assert.deepEqual(record.open.map((entry) => entry.id), ['losses-show', 'no-detour']);
    assert.deepEqual(record.properties[0], { id: 'rule-reads-only-facts', says: 'A rule reads only facts.' });
    const text = (await run(root, ['show', 'record'])).out;
    assert.match(text, /^Taken: 0\.$/m);
    assert.match(text, /^next {2}losses-show {2}decision {2}answered {3}What does the record say when Silo missed something\?$/m);
    assert.match(text, /^Properties:\n {2}rule-reads-only-facts: A rule reads only facts\.$/m);

    assert.equal((await run(root, ['show', 'engine', '--all'])).code, 2);
  });

  // A part named by a word: plan show on the id shows the part, with the word.
  const files = basePlan();
  files['parts.json'] = [...files['parts.json'], { id: 'fork', title: 'Forks', line: 'Copies of a computer.' }];
  files['fork/README.md'] = '# Forks\n';
  await withRepo(async (root) => {
    const { code, out } = await run(root, ['show', 'fork']);
    assert.equal(code, 0, out);
    assert.match(out, /^part fork: Forks\nCopies of a computer\.\nThe word "fork": /);
    const json = JSON.parse((await run(root, ['show', 'fork', '--json'])).out);
    assert.equal(json.kind, 'part');
    assert.equal(json.word.word, 'fork');
  }, files);
});

test('show on an entry: the entry and its notes first, then what binds it, the whole product\'s decisions as ids unless --all', async () => {
  await withRepo(async (root) => {
    const { out } = await run(root, ['show', 'linux-vm-engine']);
    const binds = out.indexOf('What binds it:');
    assert.ok(binds > out.indexOf('Notes:') && out.indexOf('Notes:') > out.indexOf('asks: '), out);
    assert.ok(out.indexOf('Needed by:') < binds);
    const after = out.slice(binds);
    assert.match(after, /^ {2}principle less-sure-less-power: The less sure the trigger, the less it may do\.$/m);
    assert.doesNotMatch(after, /Asks:/);
    assert.match(after, /^ {2}decision engine-language \(engine\): Zig\./m);
    assert.match(after, /^ {2}Taken decisions of the whole product \(plan show linux-vm-engine --all prints them\):\n {4}the-design$/m);
    assert.doesNotMatch(after, /The design of 18 November/);

    const all = (await run(root, ['show', 'linux-vm-engine', '--all'])).out;
    assert.match(all, /^ {2}Taken decisions of the whole product:\n {4}decision the-design: The design of 18 November 2026/m);

    // A taken decision with no answer shows the ids it adds.
    const files = basePlan();
    const { answer: _answer, ...noAnswer } = files['decisions/the-design.json'];
    files['decisions/the-design.json'] = noAnswer;
    files['engine/decisions/linux-vm-engine.json'] = { ...files['engine/decisions/linux-vm-engine.json'], needs: ['the-design'] };
    await withRepo(async (other) => {
      const shown = (await run(other, ['show', 'linux-vm-engine'])).out;
      assert.match(shown, /^ {2}decision the-design: adds computer, fork, boundary, /m);
      assert.doesNotMatch(shown, /whole product/);
      const json = JSON.parse((await run(other, ['show', 'linux-vm-engine', '--json'])).out);
      assert.ok(json.binds.decisions.find((d) => d.id === 'the-design').adds.includes('fork'));
    }, files);
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

test('a take that replaces a decision commits the changed files only: the decision it replaces keeps its file, its notes and its place', async () => {
  const files = basePlan();
  const { answer: _answer, ...onlySets } = files['record/decisions/losses-show.json'];
  files['record/decisions/losses-show.json'] = onlySets;
  files['record/decisions/losses-show.md'] = 'Why the record shows its losses.\n';
  await withRepo(async (root) => {
    assert.equal((await run(root, ['take', 'losses-show'])).code, 0);
    writeJson(root, 'record/decisions/gap-reworded.json', {
      asks: 'How is a loss said?',
      answer: 'As a gap in the record.',
      replaces: ['losses-show'],
      sets: [{ in: 'record/properties', id: 'every-loss-is-a-gap', says: 'Every loss of facts or reports leaves a gap in the record.', kind: 'safety', check: ['simulator'] }],
    });
    const order = readJson(root, 'order.json');
    order.stages.at(-1).entries.push('gap-reworded');
    writeJson(root, 'order.json', order);
    git(root, 'add', 'plan');
    git(root, 'commit', '-qm', 'Plan: gap-reworded');
    const { code, out, err } = await run(root, ['take', 'gap-reworded']);
    assert.equal(code, 0, `${out}${err}`);
    assert.equal(git(root, 'log', '-1', '--format=%s').trim(), 'Owner: take gap-reworded');
    assert.deepEqual(git(root, 'show', '--name-status', '--format=', 'HEAD').trim().split('\n').sort(), [
      'M\tplan/record/decisions/gap-reworded.json',
      'M\tplan/record/decisions/losses-show.json',
      'M\tplan/record/properties.json',
    ]);
    assert.deepEqual(readJson(root, 'record/decisions/losses-show.json'), { asks: onlySets.asks, verdict: 'take', replaced_by: 'gap-reworded' });
    assert.equal(readFileSync(join(root, 'plan', 'record', 'decisions', 'losses-show.md'), 'utf8'), 'Why the record shows its losses.\n');
    assert.deepEqual(readJson(root, 'record/decisions/gap-reworded.json').replaces, ['losses-show']);
    assert.ok(clean(root));
    assert.equal((await run(root, ['check'])).code, 0);

    // Taken again: it says it is taken and replaced, and exits 0.
    const again = await run(root, ['take', 'losses-show']);
    assert.equal(again.code, 0);
    assert.equal(again.out, 'plan take: losses-show is already taken, and replaced by gap-reworded');
    // Dropped: refused, naming the decision that replaced it.
    const dropped = await run(root, ['drop', 'losses-show']);
    assert.equal(dropped.code, 1);
    assert.match(dropped.err, /losses-show is replaced by gap-reworded/);
  }, files);
});

/** The sample with zig-again, placed last, taken, replacing engine-language. */
function replacedPlan() {
  const files = basePlan();
  files['engine/decisions/zig-again.json'] = { asks: 'Which language now?', answer: 'Zig, still.', replaces: ['engine-language'], verdict: 'take' };
  files['engine/decisions/engine-language.json'] = { ...files['engine/decisions/engine-language.json'], replaced_by: 'zig-again' };
  files['order.json'] = structuredClone(files['order.json']);
  files['order.json'].stages.at(-1).entries.push('zig-again');
  return files;
}

test('plan: the next decision that replaces others shows each one with its answer, or what it adds', async () => {
  const files = basePlan();
  files['decisions/nothing-without-a-reason.json'] = { ...files['decisions/nothing-without-a-reason.json'], replaces: ['engine-language', 'the-design'] };
  const { answer: _answer, ...design } = files['decisions/the-design.json'];
  files['decisions/the-design.json'] = design;
  await withRepo(async (root) => {
    const { code, out } = await run(root, []);
    assert.equal(code, 0, out);
    assert.match(out, /^ {2}replaces:\n {4}engine-language: Zig\. The engine is the core piece.*\n {4}the-design: adds computer, fork, boundary, /m);
    const json = JSON.parse((await run(root, ['--json'])).out);
    assert.deepEqual(json.next.replaces[0], { id: 'engine-language', answer: basePlan()['engine/decisions/engine-language.json'].answer });
    assert.deepEqual(json.next.replaces[1], { id: 'the-design', adds: design.adds });
  }, files);
});

test('plan and show print a replaced decision whose adds is not a list, which the check reports, without failing', async () => {
  const files = basePlan();
  files['decisions/nothing-without-a-reason.json'] = { ...files['decisions/nothing-without-a-reason.json'], replaces: ['the-design'] };
  const { answer: _answer, ...design } = files['decisions/the-design.json'];
  files['decisions/the-design.json'] = { ...design, adds: 'computer' };
  await withRepo(async (root) => {
    const next = await run(root, []);
    assert.equal(next.code, 0, next.out);
    assert.match(next.out, /^ {4}the-design: adds nothing$/m);
    const shown = await run(root, ['show', 'nothing-without-a-reason']);
    assert.equal(shown.code, 0, shown.out);
  }, files);
});

test('show: a decision that replaces lists what it replaces; a replaced one says so first and binds nothing; a need replaced says by what', async () => {
  await withRepo(async (root) => {
    const zig = await run(root, ['show', 'zig-again']);
    assert.match(zig.out, /^replaces:\n {2}engine-language: Zig\. The engine is the core piece/m);
    const zigJson = JSON.parse((await run(root, ['show', 'zig-again', '--json'])).out);
    assert.equal(zigJson.status, 'taken');
    assert.deepEqual(zigJson.keys.replaces, ['engine-language']);
    assert.deepEqual(zigJson.replaces, [{ id: 'engine-language', answer: basePlan()['engine/decisions/engine-language.json'].answer }]);

    const old = await run(root, ['show', 'engine-language']);
    assert.match(old.out, /^decision engine-language \(engine\): replaced, stage ground, 2 of 9\nReplaced by: zig-again\n\nasks: /);
    const oldJson = JSON.parse((await run(root, ['show', 'engine-language', '--json'])).out);
    assert.equal(oldJson.status, 'replaced');
    assert.equal(oldJson.replaced_by, 'zig-again');
    assert.equal(oldJson.keys.replaced_by, 'zig-again');
    assert.equal(old.out.match(/^replaced_by: zig-again$/m)?.length, 1);

    // In force means taken and not replaced: the replaced decision binds nothing; its need still prints.
    const feature = await run(root, ['show', 'start-computer']);
    assert.match(feature.out, /^Needs:\n {2}engine-language {2}replaced by zig-again\n/m);
    const binds = feature.out.slice(feature.out.indexOf('What binds it:'));
    assert.doesNotMatch(binds, /engine-language/);
    assert.match(binds, /^ {2}decision zig-again \(engine\): Zig, still\.$/m);
    const featureJson = JSON.parse((await run(root, ['show', 'start-computer', '--json'])).out);
    assert.deepEqual(featureJson.needs[0], { id: 'engine-language', status: 'replaced', replaced_by: 'zig-again' });
    assert.deepEqual(featureJson.binds.decisions.map((d) => d.id).sort(), ['the-design', 'zig-again']);
    const all = (await run(root, ['show', 'start-computer', '--all'])).out;
    assert.doesNotMatch(all.slice(all.indexOf('What binds it:')), /engine-language/);
    // A root decision replaced is left out of the whole product's too.
    const rootReplaced = (await run(root, ['show', 'linux-vm-engine'])).out;
    assert.doesNotMatch(rootReplaced.slice(rootReplaced.indexOf('What binds it:')), /engine-language/);

    const part = await run(root, ['show', 'engine']);
    assert.match(part.out, /^Taken: 2\n {2}start-computer, zig-again\nReplaced: 1\n {2}engine-language\n/m);
    const partJson = JSON.parse((await run(root, ['show', 'engine', '--json'])).out);
    assert.deepEqual(partJson.taken, ['start-computer', 'zig-again']);
    assert.deepEqual(partJson.replaced, ['engine-language']);
  }, replacedPlan());
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

test('drop commits "Owner: drop <id>"; dropping a taken entry that brought statements is refused', async () => {
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
    // A plain edit of the text after the owner's verdict: uncommitted, then committed.
    writeJson(root, name, { ...readJson(root, name), asks: 'What does the record say when Silo missed a report?' });
    const edited = JSON.parse((await run(root, ['check', '--json'])).out);
    assert.deepEqual(edited.errors.map((e) => `${e.file} ${e.field}`), [`plan/${name} asks`]);
    git(root, 'commit', '-q', '-am', 'Reword again');
    assert.equal((await run(root, ['check'])).code, 1);
    // A commit by the owner on another entry does not lift it.
    assert.equal((await run(root, ['drop', 'no-detour'])).code, 1);
  }, files);
});

test('rule 11 covers replaces: a change under the verdict is refused, and the owner\'s verdict after it lifts the rule', async () => {
  const files = basePlan();
  files['engine/decisions/zig-tooling.json'] = { asks: 'Which build tool?', answer: 'zig build.', verdict: 'take' };
  files['engine/decisions/zig-again.json'] = { asks: 'Which language now?', answer: 'Zig, still.', replaces: ['engine-language'], verdict: 'drop' };
  files['order.json'] = structuredClone(files['order.json']);
  files['order.json'].stages.at(-1).entries.push('zig-tooling', 'zig-again');
  await withRepo(async (root) => {
    git(root, 'update-ref', 'refs/remotes/origin/main', 'HEAD');
    const name = 'engine/decisions/zig-again.json';
    writeJson(root, name, { ...readJson(root, name), replaces: ['zig-tooling'] });
    const changed = JSON.parse((await run(root, ['check', '--json'])).out);
    assert.deepEqual(changed.errors.map((e) => `${e.file} ${e.field}`), [`plan/${name} replaces`]);
    // The verdict cleared with the change, then the owner's no again: the rule no longer applies.
    const { verdict: _verdict, ...entry } = readJson(root, name);
    writeJson(root, name, entry);
    git(root, 'commit', '-q', '-am', 'Plan: zig-again replaces zig-tooling');
    const dropped = await run(root, ['drop', 'zig-again']);
    assert.equal(dropped.code, 0, dropped.err);
    assert.equal((await run(root, ['check'])).code, 0);
  }, files);
});

test('a file under plan/ marked assume-unchanged or skip-worktree stops take and drop, which name it', async () => {
  await withRepo(async (root) => {
    const before = head(root);
    writeFileSync(join(root, 'plan', 'engine', 'README.md'), 'Edited, hidden from git status.\n');
    git(root, 'update-index', '--assume-unchanged', 'plan/engine/README.md');
    assert.ok(clean(root), 'git status no longer shows the edit');
    const assumed = await run(root, ['take', 'losses-show']);
    assert.equal(assumed.code, 1);
    assert.match(assumed.err, /plan\/engine\/README\.md/);
    assert.match(assumed.err, /assume-unchanged/);
    assert.equal(head(root), before);
    git(root, 'update-index', '--no-assume-unchanged', 'plan/engine/README.md');
    git(root, 'checkout', '--', 'plan/engine/README.md');

    writeFileSync(join(root, 'plan', 'record', 'README.md'), 'Edited, hidden from git status.\n');
    git(root, 'update-index', '--skip-worktree', 'plan/record/README.md');
    assert.ok(clean(root), 'git status no longer shows the edit');
    const skipped = await run(root, ['drop', 'no-detour']);
    assert.equal(skipped.code, 1);
    assert.match(skipped.err, /plan\/record\/README\.md/);
    assert.match(skipped.err, /skip-worktree/);
    assert.equal(head(root), before);
    git(root, 'update-index', '--no-skip-worktree', 'plan/record/README.md');
    git(root, 'checkout', '--', 'plan/record/README.md');
    assert.equal((await run(root, ['drop', 'no-detour'])).code, 0);
  });
});

test('rule 11: a take that moves a statement out of an entry the owner re-ruled leaves that entry exempt', async () => {
  await withRepo(async (root) => {
    git(root, 'update-ref', 'refs/remotes/origin/main', 'HEAD');
    const design = 'decisions/the-design.json';
    const taken = readJson(root, design);
    // Entry B, the-design: its verdict cleared and its answer changed, then taken again by the owner.
    const { verdict: _verdict, adds: _adds, ...open } = taken;
    writeJson(root, design, { ...open, answer: `${taken.answer} Revised.` });
    git(root, 'commit', '-q', '-am', 'Revise the-design');
    writeJson(root, design, { ...taken, answer: `${taken.answer} Revised.` });
    git(root, 'commit', '-q', '-am', 'Owner: take the-design');
    const before = await run(root, ['check']);
    assert.equal(before.code, 0, before.out + before.err);
    // Entry A, linux-vm-engine, moves one of the-design's properties to the engine part.
    const vm = 'engine/decisions/linux-vm-engine.json';
    const property = readJson(root, 'record/properties.json').find((p) => p.id === 'classifier-never-acts');
    writeJson(root, vm, { ...readJson(root, vm), sets: [{ in: 'engine/properties', ...property }] });
    git(root, 'commit', '-q', '-am', 'linux-vm-engine moves classifier-never-acts');
    const take = await run(root, ['take', 'linux-vm-engine']);
    assert.equal(take.code, 0, take.err);
    assert.ok(!readJson(root, design).adds.includes('classifier-never-acts'));
    const after = await run(root, ['check']);
    assert.equal(after.code, 0, after.out + after.err);
  });
});

/** The problems `plan check --json` lists, as `file field`. */
const problems = async (root) => JSON.parse((await run(root, ['check', '--json'])).out).errors.map((e) => `${e.file} ${e.field}`);

test('a verdict that the command did not write fails the check, naming the file, the field and the fix', async () => {
  await withRepo(async (root) => {
    git(root, 'update-ref', 'refs/remotes/origin/main', 'HEAD');
    const name = 'engine/decisions/linux-vm-engine.json';
    // Typed in and not committed yet.
    writeJson(root, name, { ...readJson(root, name), verdict: 'take' });
    const typed = JSON.parse((await run(root, ['check', '--json'])).out);
    assert.deepEqual(typed.errors.map((e) => `${e.file} ${e.field}`), [`plan/${name} verdict`]);
    assert.match(typed.errors[0].fix, /undo that change; the owner's yes is the command plan take linux-vm-engine/);
    const text = await run(root, ['check']);
    assert.equal(text.code, 1);
    assert.match(text.out, /plan\/engine\/decisions\/linux-vm-engine\.json: verdict: .* Fix: undo that change/);
    // Committed under any other subject.
    git(root, 'commit', '-q', '-am', 'Plan: take linux-vm-engine');
    assert.deepEqual(await problems(root), [`plan/${name} verdict`]);
    // Even under the other verdict's subject.
    git(root, 'commit', '-q', '--amend', '-m', 'Owner: drop linux-vm-engine');
    assert.deepEqual(await problems(root), [`plan/${name} verdict`]);
    // Taken away by hand: not decided again, which passes.
    const { verdict: _verdict, ...open } = readJson(root, name);
    writeJson(root, name, open);
    assert.deepEqual(await problems(root), []);
  });
});

test('a new entry born with a verdict, and a verdict changed by hand, fail the check', async () => {
  const files = basePlan();
  files['record/decisions/no-detour.json'] = { ...files['record/decisions/no-detour.json'], verdict: 'drop' };
  await withRepo(async (root) => {
    git(root, 'update-ref', 'refs/remotes/origin/main', 'HEAD');
    // A copy of a decided entry under a new id.
    writeJson(root, 'engine/decisions/copied.json', { ...readJson(root, 'engine/decisions/engine-language.json') });
    const order = readJson(root, 'order.json');
    order.stages.at(-1).entries.push('copied');
    writeJson(root, 'order.json', order);
    git(root, 'add', 'plan');
    git(root, 'commit', '-q', '-m', 'Plan: copied');
    assert.deepEqual(await problems(root), ['plan/engine/decisions/copied.json verdict']);
    // take to drop, and drop to take, by hand.
    const flipped = { 'engine/decisions/engine-language.json': 'drop', 'record/decisions/no-detour.json': 'take' };
    for (const [name, verdict] of Object.entries(flipped)) writeJson(root, name, { ...readJson(root, name), verdict });
    git(root, 'commit', '-q', '-am', 'Plan: flip');
    assert.deepEqual((await problems(root)).filter((p) => p.endsWith(' verdict')).sort(), [
      'plan/engine/decisions/copied.json verdict',
      'plan/engine/decisions/engine-language.json verdict',
      'plan/record/decisions/no-detour.json verdict',
    ]);
  }, files);
});

test('plan take and plan drop leave the check passing, also after the entry was edited by hand and a verdict typed in', async () => {
  await withRepo(async (root) => {
    git(root, 'update-ref', 'refs/remotes/origin/main', 'HEAD');
    const name = 'engine/decisions/linux-vm-engine.json';
    writeJson(root, name, { ...readJson(root, name), verdict: 'take', answer: 'Firecracker.' });
    git(root, 'commit', '-q', '-am', 'Plan: typed take');
    assert.deepEqual(await problems(root), [`plan/${name} verdict`]);
    // Cleared by hand and the text edited, then ruled on by the command.
    const { verdict: _verdict, ...open } = readJson(root, name);
    writeJson(root, name, { ...open, answer: 'Cloud Hypervisor.' });
    git(root, 'commit', '-q', '-am', 'Plan: clear it');
    const ruled = await run(root, ['take', 'linux-vm-engine']);
    assert.equal(ruled.code, 0, ruled.err);
    assert.deepEqual(await problems(root), []);
    // A drop, which writes its own verdict.
    const dropped = await run(root, ['drop', 'no-detour']);
    assert.equal(dropped.code, 0, dropped.err);
    assert.deepEqual(await problems(root), []);
    // The take of another entry rewrites adds and replaced_by in files it does not rule on.
    assert.equal((await run(root, ['take', 'losses-show'])).code, 0);
    assert.deepEqual(await problems(root), []);
    // A typed verdict after the owner's, in a later commit, is refused again.
    const vm = readJson(root, name);
    writeJson(root, name, { ...vm, verdict: 'drop' });
    git(root, 'commit', '-q', '-am', 'Plan: flip it');
    assert.ok((await problems(root)).includes(`plan/${name} verdict`));
  });
});

test('a verdict on a branch merged by a merge commit still counts', async () => {
  await withRepo(async (root) => {
    git(root, 'update-ref', 'refs/remotes/origin/main', 'HEAD');
    git(root, 'checkout', '-q', '-b', 'owner');
    assert.equal((await run(root, ['take', 'losses-show'])).code, 0);
    git(root, 'checkout', '-q', 'main');
    writeFileSync(join(root, 'NOTES.md'), 'other work\n');
    git(root, 'add', 'NOTES.md');
    git(root, 'commit', '-q', '-m', 'Other work');
    git(root, 'merge', '-q', '--no-ff', '-m', 'Merge branch owner', 'owner');
    assert.deepEqual(await problems(root), []);
  });
});

test('a merged plan that holds no entry is no base for verdicts: the entries it lacks are not refused', async () => {
  await withRepo(async (root) => {
    // The remote holds the old plan: a commit with no entry of today's.
    const empty = git(root, 'commit-tree', '4b825dc642cb6eb9a060e54bf8d69288fbee4904', '-m', 'Old plan').trim();
    git(root, 'update-ref', 'refs/remotes/origin/main', empty);
    assert.deepEqual(await problems(root), []);
    // The commands work against it too.
    assert.equal((await run(root, ['take', 'losses-show'])).code, 0);
    assert.deepEqual(await problems(root), []);
  });
});

const EVIDENCE_NAME = 'engine/decisions/linux-vm-engine.json';

test('evidence: a take on a branch, with main editing only the suggest of the entry, still counts after the merge', async () => {
  const name = EVIDENCE_NAME;
  await withRepo(async (root) => {
    // Taken on a branch while main edits only the suggest of the same entry, then merged.
    git(root, 'update-ref', 'refs/remotes/origin/main', 'HEAD');
    git(root, 'checkout', '-q', '-b', 'owner', 'main');
    assert.equal((await run(root, ['take', 'linux-vm-engine'])).code, 0);
    git(root, 'checkout', '-q', 'main');
    writeJson(root, name, { ...readJson(root, name), suggest: { verdict: 'take', why: 'It is the engine we know.' } });
    git(root, 'commit', '-q', '-am', 'Suggest a take');
    try {
      git(root, 'merge', '--no-commit', '--no-ff', 'owner');
    } catch {
      // The suggest and the verdict are neighbours in the file: the merge is resolved by hand below.
    }
    writeJson(root, name, { ...JSON.parse(git(root, 'show', `owner:plan/${name}`)), suggest: { verdict: 'take', why: 'It is the engine we know.' } });
    git(root, 'add', 'plan');
    git(root, 'commit', '-q', '-m', 'Merge branch owner');
    assert.equal(readJson(root, name).verdict, 'take');
    assert.equal(readJson(root, name).suggest.verdict, 'take');
    assert.deepEqual(await problems(root), []);
  });
});

test('evidence: a take still counts when the file and notes of the entry were moved to another part', async () => {
  const name = EVIDENCE_NAME;
  await withRepo(async (root) => {
    // Taken, then the file and its notes moved to another part, id and content unchanged.
    git(root, 'update-ref', 'refs/remotes/origin/main', 'HEAD');
    const taken = await run(root, ['take', 'linux-vm-engine']);
    assert.equal(taken.code, 0, taken.err);
    git(root, 'mv', 'plan/engine/decisions/linux-vm-engine.json', 'plan/record/decisions/linux-vm-engine.json');
    git(root, 'mv', 'plan/engine/decisions/linux-vm-engine.md', 'plan/record/decisions/linux-vm-engine.md');
    git(root, 'commit', '-q', '-m', 'Move linux-vm-engine to the record');
    assert.deepEqual(await problems(root), []);
  });
});

test('evidence: a typed take for one answer is refused when the merge is resolved to it over the take of another', async () => {
  const name = EVIDENCE_NAME;
  await withRepo(async (root) => {
    // A typed take for answer A on one branch, the owner's take of answer B on another, the merge resolved to A.
    git(root, 'update-ref', 'refs/remotes/origin/main', 'HEAD');
    const before = readJson(root, name);
    git(root, 'checkout', '-q', '-b', 'typed');
    writeJson(root, name, { ...before, answer: 'Answer A.', verdict: 'take' });
    git(root, 'commit', '-q', '-am', 'Plan: typed take');
    git(root, 'checkout', '-q', 'main');
    git(root, 'checkout', '-q', '-b', 'owner');
    writeJson(root, name, { ...before, answer: 'Answer B.' });
    git(root, 'commit', '-q', '-am', 'Plan: answer B');
    assert.equal((await run(root, ['take', 'linux-vm-engine'])).code, 0);
    git(root, 'checkout', '-q', 'main');
    git(root, 'merge', '-q', '--no-ff', '-m', 'Merge branch owner', 'owner');
    try {
      git(root, 'merge', '--no-commit', '--no-ff', 'typed');
    } catch {
      // The two branches wrote the same lines: the merge is resolved by hand below.
    }
    writeJson(root, name, { ...before, answer: 'Answer A.', verdict: 'take' });
    git(root, 'add', 'plan');
    git(root, 'commit', '-q', '-m', 'Merge branch typed');
    assert.equal(readJson(root, name).answer, 'Answer A.');
    assert.deepEqual(await problems(root), [`plan/${name} verdict`]);
  });
});

test('evidence: the ruled text edited and then put back as it was still counts', async () => {
  const name = EVIDENCE_NAME;
  await withRepo(async (root) => {
    // The owner takes, the text is edited and then put back as it was: it holds what the owner ruled on.
    git(root, 'update-ref', 'refs/remotes/origin/main', 'HEAD');
    assert.equal((await run(root, ['take', 'linux-vm-engine'])).code, 0);
    const ruled = readJson(root, name);
    writeJson(root, name, { ...ruled, answer: 'Something else.' });
    git(root, 'commit', '-q', '-am', 'Edit the answer');
    assert.ok((await problems(root)).length > 0);
    writeJson(root, name, ruled);
    git(root, 'commit', '-q', '-am', 'Put it back');
    assert.deepEqual(await problems(root), []);
  });
});

test('evidence: the latest ruling counts: a text put back by hand to an earlier ruled answer is refused', async () => {
  const name = EVIDENCE_NAME;
  await withRepo(async (root) => {
    // The owner takes A, the verdict is removed and the text edited to B, the owner takes B, the text is put back to A by hand.
    git(root, 'update-ref', 'refs/remotes/origin/main', 'HEAD');
    const open = readJson(root, name);
    writeJson(root, name, { ...open, answer: 'Answer A.' });
    git(root, 'commit', '-q', '-am', 'Plan: answer A');
    assert.equal((await run(root, ['take', 'linux-vm-engine'])).code, 0);
    writeJson(root, name, { ...open, answer: 'Answer B.' });
    git(root, 'commit', '-q', '-am', 'Plan: clear it, answer B');
    assert.equal((await run(root, ['take', 'linux-vm-engine'])).code, 0);
    assert.deepEqual(await problems(root), []);
    writeJson(root, name, { ...readJson(root, name), answer: 'Answer A.' });
    git(root, 'commit', '-q', '-am', 'Plan: back to A');
    assert.deepEqual(await problems(root), [`plan/${name} verdict`]);
  });
});

test('evidence: an older ruling never comes before a later one of its descendants, whatever their commit times', async () => {
  const name = EVIDENCE_NAME;
  await withRepo(async (root) => {
    git(root, 'update-ref', 'refs/remotes/origin/main', 'HEAD');
    // The owner takes A on main. On a branch from it, the verdict is cleared, the answer becomes B,
    // and the owner takes B with a commit time before A's. Main goes on with other work; then the merge.
    const open = readJson(root, name);
    assert.equal((await run(root, ['take', 'linux-vm-engine'])).code, 0);
    git(root, 'checkout', '-q', '-b', 'later');
    writeJson(root, name, { ...open, answer: 'Answer B.' });
    git(root, 'commit', '-q', '-am', 'Plan: clear it, answer B');
    process.env.GIT_COMMITTER_DATE = '2020-01-01T00:00:00Z';
    try {
      assert.equal((await run(root, ['take', 'linux-vm-engine'])).code, 0);
    } finally {
      delete process.env.GIT_COMMITTER_DATE;
    }
    git(root, 'checkout', '-q', 'main');
    writeFileSync(join(root, 'NOTES.md'), 'other work\n');
    git(root, 'add', 'NOTES.md');
    git(root, 'commit', '-q', '-m', 'Other work');
    git(root, 'merge', '-q', '--no-ff', '-m', 'Merge branch later', 'later');
    assert.equal(readJson(root, name).answer, 'Answer B.');
    assert.deepEqual(await problems(root), []);
  });
});

test('evidence: a take squashed with the move of the entry to another part still counts', async () => {
  await withRepo(async (root) => {
    git(root, 'update-ref', 'refs/remotes/origin/main', 'HEAD');
    git(root, 'mv', 'plan/engine/decisions/linux-vm-engine.json', 'plan/record/decisions/linux-vm-engine.json');
    git(root, 'mv', 'plan/engine/decisions/linux-vm-engine.md', 'plan/record/decisions/linux-vm-engine.md');
    writeJson(root, 'record/decisions/linux-vm-engine.json', { ...JSON.parse(git(root, 'show', 'HEAD:plan/engine/decisions/linux-vm-engine.json')), verdict: 'take' });
    git(root, 'add', 'plan');
    git(root, 'commit', '-q', '-m', 'Owner: take linux-vm-engine');
    assert.deepEqual(await problems(root), []);
  });
});

test('evidence: an id with a character of a regular expression gives the file\'s problems, not a generic error', async () => {
  await withRepo(async (root) => {
    git(root, 'update-ref', 'refs/remotes/origin/main', 'HEAD');
    writeJson(root, 'engine/decisions/odd(.json', { asks: 'Which?', answer: 'This.', verdict: 'take' });
    git(root, 'add', 'plan');
    git(root, 'commit', '-q', '-m', 'Owner: take odd(');
    const checked = await run(root, ['check', '--json']);
    const listed = JSON.parse(checked.out);
    assert.equal(listed.ok, false);
    assert.ok(listed.errors.some((e) => e.file === 'plan/engine/decisions/odd(.json'), checked.out);
  });
});

test('take --by agents writes the key after the verdict and commits "Agents: take <id>"; drop --by agents "Agents: drop <id>"; the check passes after', async () => {
  await withRepo(async (root) => {
    git(root, 'update-ref', 'refs/remotes/origin/main', 'HEAD');
    const taken = await run(root, ['take', 'losses-show', '--by', 'agents']);
    assert.equal(taken.code, 0, taken.out + taken.err);
    assert.equal(git(root, 'log', '-1', '--format=%s').trim(), 'Agents: take losses-show');
    assert.match(taken.out, /^Agents: take losses-show/);
    assert.deepEqual(git(root, 'show', '--name-only', '--format=', 'HEAD').trim().split('\n').sort(), ['plan/record/decisions/losses-show.json', 'plan/record/properties.json']);
    const file = readJson(root, 'record/decisions/losses-show.json');
    const keys = Object.keys(file);
    assert.equal(file.verdict, 'take');
    assert.equal(keys[keys.indexOf('verdict') + 1], 'by');
    assert.equal(file.by, 'agents');
    assert.equal(readFileSync(join(root, 'plan/record/decisions/losses-show.json'), 'utf8'), canonicalText(canonicalFile('record/decisions/losses-show.json')(file)));
    assert.equal((await run(root, ['check'])).code, 0);

    const dropped = await run(root, ['drop', 'no-detour', '--by=agents']);
    assert.equal(dropped.code, 0, dropped.out + dropped.err);
    assert.equal(git(root, 'log', '-1', '--format=%s').trim(), 'Agents: drop no-detour');
    assert.equal(readJson(root, 'record/decisions/no-detour.json').by, 'agents');
    assert.equal((await run(root, ['check'])).code, 0);
    // Without --by, both are the owner's as ever.
    assert.equal((await run(root, ['take', 'linux-vm-engine'])).code, 0);
    assert.equal(git(root, 'log', '-1', '--format=%s').trim(), 'Owner: take linux-vm-engine');
    assert.equal(Object.hasOwn(readJson(root, 'engine/decisions/linux-vm-engine.json'), 'by'), false);
  });
});

test('drop --by agents on an entry the owner took is refused and writes nothing; a --by other than agents is wrong use', async () => {
  await withRepo(async (root) => {
    const before = head(root);
    const refused = await run(root, ['drop', 'engine-language', '--by', 'agents']);
    assert.equal(refused.code, 1);
    assert.match(refused.err, /taken by the owner/);
    assert.equal(head(root), before);
    assert.ok(clean(root));
    for (const argv of [['take', 'linux-vm-engine', '--by', 'owner'], ['drop', 'linux-vm-engine', '--by', ''], ['take', 'linux-vm-engine', '--by'], ['take', 'linux-vm-engine', '--by', 'agents', '--by', 'agents']]) {
      assert.equal((await run(root, argv)).code, 2, argv.join(' '));
    }
    assert.equal(head(root), before);
    assert.equal((await run(root, ['take', '--by', 'agents', 'linux-vm-engine'])).code, 0);
  });
});

test('the check reads "Agents: take" as the evidence of a verdict with by agents, and nothing else: a by typed in, or a subject that does not match, fails', async () => {
  await withRepo(async (root) => {
    git(root, 'update-ref', 'refs/remotes/origin/main', 'HEAD');
    assert.equal((await run(root, ['take', 'linux-vm-engine', '--by', 'agents'])).code, 0);
    assert.equal((await run(root, ['check'])).code, 0);
    const name = 'engine/decisions/linux-vm-engine.json';
    const file = readJson(root, name);
    // The same file under the owner's subject: who wrote it is not the owner.
    git(root, 'commit', '-q', '--amend', '-m', 'Owner: take linux-vm-engine');
    assert.equal((await run(root, ['check'])).code, 1);
    git(root, 'commit', '-q', '--amend', '-m', 'Agents: take linux-vm-engine');
    assert.equal((await run(root, ['check'])).code, 0);
    // A file without by under the agents' subject.
    const { by: _by, ...owner } = file;
    writeJson(root, name, owner);
    git(root, 'commit', '-q', '-am', 'Agents: take linux-vm-engine');
    assert.equal((await run(root, ['check'])).code, 1);
  });
  await withRepo(async (root) => {
    git(root, 'update-ref', 'refs/remotes/origin/main', 'HEAD');
    // by typed into a verdict the owner wrote, with no command.
    assert.equal((await run(root, ['take', 'linux-vm-engine'])).code, 0);
    git(root, 'update-ref', 'refs/remotes/origin/main', 'HEAD');
    const name = 'engine/decisions/linux-vm-engine.json';
    writeJson(root, name, { ...readJson(root, name), by: 'agents' });
    const typed = JSON.parse((await run(root, ['check', '--json'])).out);
    assert.deepEqual(typed.errors.map((e) => `${e.file} ${e.field}`), [`plan/${name} by`]);
    // by with no verdict, and a by that is not agents.
    writeJson(root, name, { ...readJson(root, name), verdict: undefined, by: 'agents' });
    assert.match((await run(root, ['check'])).out, /by without a verdict/);
    writeJson(root, name, { ...readJson(root, name), verdict: 'take', by: 'owner' });
    assert.match((await run(root, ['check'])).out, /"owner" is not a value of by/);
  });
});
