import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { checkFiles } from '../check/index.js';
import { loadDir } from '../read/index.js';
import { basePlan, cleanup, fakeLive, git, gitWith, initRepo, run, temp, writePlan } from './helpers.js';

const readJson = (root, rel) => JSON.parse(readFileSync(join(root, 'plan', rel), 'utf8'));
const order = (root) => readJson(root, 'order.json').stages.map((stage) => [stage.id, stage.features]);
const checkErrors = (root) => checkFiles(loadDir(join(root, 'plan'))).errors;

async function withPlan(files, body) {
  const root = writePlan(files);
  try {
    await body(root);
  } finally {
    cleanup(root);
  }
}

test('plan alone lists the commands, the coming ones marked', async () => {
  const { code, out } = await run(temp(), []);
  assert.equal(code, 0);
  for (const name of ['status', 'show', 'add', 'finish', 'check']) assert.match(out, new RegExp(`plan ${name}`));
  assert.match(out, /Coming with the GitHub side:[\s\S]*plan decide[\s\S]*plan doctor/);
  const json = JSON.parse((await run(temp(), ['--json'])).out);
  assert.deepEqual(json.commands.filter((item) => !item.coming).map((item) => item.name), ['status', 'show', 'add', 'finish', 'check']);
});

test('wrong use exits 2; a coming command is not built yet', async () => {
  await withPlan(basePlan(), async (root) => {
    assert.equal((await run(root, ['nonsense'])).code, 2);
    const coming = await run(root, ['decide', 'fork', 'take']);
    assert.equal(coming.code, 2);
    assert.match(coming.err, /not built yet/);
    assert.equal((await run(root, ['show'])).code, 2);
    assert.equal((await run(root, ['check', '--nope'])).code, 2);
  });
});

test('outside a plan the command refuses', async () => {
  const dir = temp();
  try {
    const { code, err } = await run(dir, ['status']);
    assert.equal(code, 1);
    assert.match(err, /no plan\/ found/);
  } finally {
    cleanup(dir);
  }
});

test('check: 0 on a valid plan, 1 with the errors, from a subdirectory too', async () => {
  await withPlan(basePlan(), async (root) => {
    mkdirSync(join(root, 'sub'));
    const ok = await run(join(root, 'sub'), ['check']);
    assert.equal(ok.code, 0);
    assert.match(ok.out, /plan check: ok \(4 features in 3 stages, 2 areas, 2 decisions\)/);
    writeFileSync(join(root, 'plan', 'features', 'core.json'), '{"name":"Core"}\n');
    const bad = await run(root, ['check', '--json']);
    assert.equal(bad.code, 1);
    const json = JSON.parse(bad.out);
    assert.equal(json.ok, false);
    assert.ok(json.errors.some((error) => error.file === 'plan/features/core.json' && error.field === 'what'));
  });
});

test('check mentions the map when an area is new against the merged plan', async () => {
  await withPlan(basePlan(), async (root) => {
    initRepo(root);
    const areas = readJson(root, 'areas.json');
    areas.push({ id: 'access', title: 'Access', line: 'Keys.' });
    writeFileSync(join(root, 'plan', 'areas.json'), `${JSON.stringify(areas, null, 2)}\n`);
    const { code, out } = await run(root, ['check']);
    assert.equal(code, 0);
    assert.match(out, /the area "access" is new.*map\.ts/);
  });
});

test('add after a feature, first in a stage, and from a suggestion', async () => {
  await withPlan(basePlan(), async (root) => {
    const after = await run(root, ['add', 'quiet', '--name', 'Quiet forks', '--area', 'forks', '--after', 'core', '--what', 'Forks that do not answer twice.']);
    assert.equal(after.code, 0, after.err);
    assert.equal(after.out, 'plan/features/quiet.json\nplan/order.json');
    assert.deepEqual(order(root)[0], ['shell', ['core', 'quiet', 'cli']]);
    assert.deepEqual(readJson(root, 'features/quiet.json'), { name: 'Quiet forks', what: 'Forks that do not answer twice.', area: 'forks', needs: [] });

    const last = await run(root, ['add', 'tree', '--name', 'Tree', '--area', 'forks', '--after', 'fork', '--what', 'The tree.']);
    assert.equal(last.code, 0, last.err);
    assert.deepEqual(order(root)[1], ['loop', ['fork', 'tree']]);

    const first = await run(root, ['add', 'merge', '--name', 'Merge', '--area', 'forks', '--first-in', 'loop', '--what', 'Merge.', '--from', '#42', '--json']);
    assert.equal(first.code, 0, first.err);
    assert.deepEqual(JSON.parse(first.out), { ok: true, id: 'merge', stage: 'loop', paths: ['plan/features/merge.json', 'plan/order.json'], todo: null });
    assert.deepEqual(order(root)[1], ['loop', ['merge', 'fork', 'tree']]);
    assert.equal(readJson(root, 'features/merge.json').suggested_in, 42);
    assert.deepEqual(checkErrors(root), []);
  });
});

test('add without --what leaves a plan the check refuses until it is written', async () => {
  await withPlan(basePlan(), async (root) => {
    const { code, out } = await run(root, ['add', 'idea', '--name', 'Idea', '--area', 'engine', '--first-in', 'shell']);
    assert.equal(code, 0);
    assert.match(out, /write what it does in plan\/features\/idea\.json/);
    assert.deepEqual(checkErrors(root).map((e) => `${e.file} ${e.field}`), ['plan/features/idea.json what']);
  });
});

test('add refuses and changes nothing', async () => {
  await withPlan(basePlan(), async (root) => {
    const before = loadDir(join(root, 'plan'));
    const cases = [
      [['add', 'fork', '--name', 'F', '--area', 'forks', '--after', 'core'], 1, /exists already/],
      [['add', 'new', '--name', 'N', '--area', 'nowhere', '--after', 'core'], 1, /no area "nowhere"/],
      [['add', 'new', '--name', 'N', '--area', 'forks', '--after', 'nobody'], 1, /no feature "nobody"/],
      [['add', 'new', '--name', 'N', '--area', 'forks', '--first-in', 'nowhere'], 1, /no stage "nowhere"/],
      [['add', 'new', '--name', 'N', '--area', 'forks', '--first-in', 'gone'], 1, /stage "gone"/],
      [['add', 'new', '--name', 'N', '--area', 'forks', '--after', 'old'], 1, /stage "gone"/],
      [['add', 'new', '--name', 'N', '--area', 'forks'], 2, /one of --after and --first-in/],
      [['add', 'new', '--name', 'N', '--area', 'forks', '--after', 'core', '--first-in', 'loop'], 2, /one of --after and --first-in/],
      [['add', 'Bad', '--name', 'N', '--area', 'forks', '--after', 'core'], 2, /not a feature id/],
      [['add', 'new', '--area', 'forks', '--after', 'core'], 2, /usage/],
      [['add', 'new', '--name', 'N', '--area', 'forks', '--after', 'core', '--from', 'abc'], 2, /issue number/],
    ];
    for (const [argv, code, message] of cases) {
      const result = await run(root, argv);
      assert.equal(result.code, code, argv.join(' '));
      assert.match(result.err, message, argv.join(' '));
    }
    assert.deepEqual(loadDir(join(root, 'plan')), before);
  });
});

test('add: random sequences keep every feature in order.json exactly once, where it was asked', async () => {
  let seed = 7;
  const random = (n) => {
    seed = (seed * 1103515245 + 12345) % 2147483648;
    return seed % n;
  };
  for (let round = 0; round < 6; round += 1) {
    await withPlan(basePlan(), async (root) => {
      const model = { shell: ['core', 'cli'], loop: ['fork'], gone: ['old'] };
      for (let step = 0; step < 12; step += 1) {
        const id = `f${round}-${step}`;
        const placeable = [...model.shell, ...model.loop];
        let argv;
        if (random(2) === 0) {
          const after = placeable[random(placeable.length)];
          const stage = model.shell.includes(after) ? 'shell' : 'loop';
          model[stage].splice(model[stage].indexOf(after) + 1, 0, id);
          argv = ['--after', after];
        } else {
          const stage = ['shell', 'loop'][random(2)];
          model[stage].unshift(id);
          argv = ['--first-in', stage];
        }
        const result = await run(root, ['add', id, '--name', id, '--area', 'engine', '--what', 'W', ...argv]);
        assert.equal(result.code, 0, result.err);
        assert.deepEqual(order(root), Object.entries(model));
        assert.deepEqual(checkErrors(root), []);
      }
    });
  }
});

test('finish writes built today (UTC) on a taken feature whose needs are built', async () => {
  const files = basePlan();
  files['features/core.json'].verdict = 'take';
  files['features/cli.json'].verdict = 'take';
  files['features/fork.json'].verdict = 'later';
  await withPlan(files, async (root) => {
    const early = await run(root, ['finish', 'cli']);
    assert.equal(early.code, 1);
    assert.match(early.err, /needs core, not built yet/);
    const done = await run(root, ['finish', 'core']);
    assert.equal(done.code, 0, done.err);
    assert.equal(done.out, 'plan/features/core.json: built 2026-11-20\ngit commit -m "Built core" -- plan/features/core.json');
    assert.equal(readFileSync(join(root, 'plan', 'features', 'core.json'), 'utf8'), '{\n  "name": "Core",\n  "what": "The engine.",\n  "area": "engine",\n  "needs": [],\n  "verdict": "take",\n  "built": "2026-11-20"\n}\n');
    assert.equal((await run(root, ['finish', 'cli'])).code, 0);
    const twice = await run(root, ['finish', 'core']);
    assert.equal(twice.code, 1);
    assert.match(twice.err, /built already/);
    const later = await run(root, ['finish', 'fork']);
    assert.equal(later.code, 1);
    assert.match(later.err, /no verdict take/);
    assert.equal((await run(root, ['finish', 'old'])).code, 1);
    assert.equal((await run(root, ['finish', 'nobody'])).code, 1);
    assert.deepEqual(checkErrors(root), []);
  });
});

test('status: latest verdicts from git log, the next to decide, taken features, live parts not available', async () => {
  const files = basePlan();
  files['features/core.json'] = { ...files['features/core.json'], verdict: 'take', built: '2026-11-01' };
  files['features/cli.json'].verdict = 'take';
  await withPlan(files, async (root) => {
    initRepo(root);
    for (let i = 1; i <= 7; i += 1) {
      writeFileSync(join(root, 'plan', 'features', 'fork.md'), `note ${i}\n`);
      gitWith({ GIT_COMMITTER_DATE: `2026-11-${10 + i}T12:00:00Z` }, root, 'commit', '-q', '-am', i === 4 ? 'Not a verdict' : `Owner: take f${i}`);
    }
    writeFileSync(join(root, 'README.md'), 'x');
    git(root, 'add', 'README.md');
    git(root, 'commit', '-q', '-m', 'Owner: touches no feature');
    const { code, out } = await run(root, ['status', '--json']);
    assert.equal(code, 0);
    const json = JSON.parse(out);
    assert.deepEqual(json.latest.map((item) => item.subject), ['Owner: take f7', 'Owner: take f6', 'Owner: take f5', 'Owner: take f3', 'Owner: take f2']);
    assert.deepEqual(json.latest.map((item) => item.age), ['3 days ago', '4 days ago', '5 days ago', '7 days ago', '8 days ago']);
    assert.deepEqual(json.next, { id: 'fork', name: 'Fork' });
    assert.deepEqual(json.taken, [{ id: 'cli', name: 'CLI', ready: true, waiting: [] }]);
    assert.equal(json.building, null);
    assert.equal(json.ideas, null);
    assert.equal(json.live.available, false);
    assert.equal(json.line, 'Engine is next.');
    const text = (await run(root, ['status'])).out;
    assert.match(text, /Latest verdicts:\n  take f7  \(3 days ago, [0-9a-f]{7}\)/);
    assert.match(text, /Next to decide: fork, Fork/);
    assert.match(text, /Being built:\n  not available: /);
    assert.match(text, /cli, CLI: ready/);
  });
});

test('status: outside git the verdicts say so; with live state the live parts show', async () => {
  const files = basePlan();
  files['features/cli.json'].verdict = 'take';
  files['features/fork.json'] = { ...files['features/fork.json'], verdict: 'take', suggested_in: 9 };
  await withPlan(files, async (root) => {
    const live = fakeLive({
      entries: { cli: { number: 2, url: 'u', verdict: 'take', building: true, branch: 'feat/cli', up: 0, comments: [], history: [] } },
      suggestions: [{ number: 9, url: 'u9', title: 'Placed', up: 3, open: true }, { number: 10, url: 'u10', title: 'Loose', up: 1, open: true }, { number: 11, url: 'u11', title: 'Closed', up: 0, open: false }],
    });
    const json = JSON.parse((await run(root, ['status', '--json'], { live })).out);
    assert.equal(json.latest, null);
    assert.deepEqual(json.building, [{ id: 'cli', name: 'CLI', branch: 'feat/cli' }]);
    assert.deepEqual(json.taken, [{ id: 'fork', name: 'Fork', ready: false, waiting: ['core'] }]);
    assert.deepEqual(json.ideas.map((item) => item.number), [10]);
    assert.equal(json.live, null);
    const failed = { async read() { return { available: false, reason: 'GitHub did not answer', failed: true }; } };
    const result = await run(root, ['status'], { live: failed });
    assert.equal(result.code, 3);
    assert.match(result.out, /not available: GitHub did not answer/);
    assert.match(result.out, /Next to decide: core/);
  });
});

test('show: fields, place, needs, needed by, decisions, notes, and the issue when live', async () => {
  const files = basePlan();
  await withPlan(files, async (root) => {
    const json = JSON.parse((await run(root, ['show', 'fork', '--json'])).out);
    assert.deepEqual(json.fields, { name: 'Fork', what: 'Copy a computer.', area: 'forks', needs: ['core'], suggest: { verdict: 'take', why: 'The heart.' } });
    assert.deepEqual([json.stage, json.position, json.of, json.status], [{ id: 'loop', title: 'The loop' }, 3, 4, 'undecided']);
    assert.deepEqual(json.needs, [{ id: 'core', name: 'Core', status: 'undecided' }]);
    assert.deepEqual(json.neededBy, []);
    assert.deepEqual(json.decisions.map((d) => d.id), ['everywhere', 'forks-rule']);
    assert.equal(json.notes, '## Notes\n\n- A note.\n');
    assert.equal(json.issue, null);
    assert.equal(json.live.available, false);
    const core = JSON.parse((await run(root, ['show', 'core', '--json'])).out);
    assert.deepEqual(core.neededBy.map((item) => item.id), ['cli', 'fork']);
    assert.deepEqual(core.decisions.map((d) => d.id), ['everywhere']);
    const text = (await run(root, ['show', 'fork'])).out;
    assert.match(text, /^fork: Fork\n {2}Copy a computer\./);
    assert.match(text, /Stage: loop \(The loop\), position 3 of 4/);
    assert.match(text, /Notes:\n {2}## Notes/);
    assert.match(text, /Issue:\n {2}not available/);
    assert.equal((await run(root, ['show', 'nobody'])).code, 1);

    const sha = (c) => c.repeat(40);
    const live = fakeLive({
      entries: {
        fork: {
          number: 5, url: 'https://github.com/o/r/issues/5', verdict: 'take', building: false, up: 4,
          comments: [{ author: 'ann', createdAt: '2026-11-18T10:00:00Z', body: 'Looks good\nmore' }, { author: 'bob', createdAt: '2026-11-16T10:00:00Z', body: 'First' }],
          history: [{ createdAt: '2026-11-17T10:00:00Z', change: { commit: sha('a'), before: sha('b'), fields: ['what'] } }],
        },
      },
    });
    const withLive = JSON.parse((await run(root, ['show', 'fork', '--json'], { live })).out);
    assert.equal(withLive.status, 'planned');
    assert.deepEqual(withLive.issue.thread.map((item) => item.kind), ['comment', 'history', 'comment']);
    const liveText = (await run(root, ['show', 'fork'], { live })).out;
    assert.match(liveText, /#5 https:\/\/github\.com\/o\/r\/issues\/5 \(4 votes\)\n {2}2026-11-16 bob: First\n {2}2026-11-17 updated in aaaaaaa: what\n {2}2026-11-18 ann: Looks good/);
  });
});
