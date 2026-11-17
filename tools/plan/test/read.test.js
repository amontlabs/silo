import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { PlanError, loadRef, readPlan, readPlanAt } from '../read/index.js';
import { basePlan, cleanup, git, initRepo, writePlan } from './helpers.js';

function assertBase(plan) {
  assert.deepEqual(plan.areas.map((area) => area.id), ['engine', 'forks']);
  assert.deepEqual(plan.stages.map((stage) => [stage.id, stage.features]), [['shell', ['core', 'cli']], ['loop', ['fork']], ['gone', ['old']]]);
  assert.deepEqual(Object.keys(plan.features).sort(), ['cli', 'core', 'fork', 'old']);
  assert.deepEqual(plan.features.fork, {
    name: 'Fork', what: 'Copy a computer.', area: 'forks', needs: ['core'], suggest: { verdict: 'take', why: 'The heart.' },
    id: 'fork', stage: 'loop', position: 3, notes: '## Notes\n\n- A note.\n',
  });
  assert.equal(plan.features.old.position, 4);
  assert.equal(plan.features.old.area, undefined);
  assert.equal(plan.features.core.notes, null);
  assert.equal(plan.decisions.length, 2);
  assert.equal(plan.product, '# Product\n');
  // A feature id that is also a name on Object.prototype is still only a feature.
  assert.equal(plan.features.constructor, undefined);
}

test('reads plan/ from a directory', () => {
  const root = writePlan(basePlan());
  try {
    assertBase(readPlan(join(root, 'plan')));
  } finally {
    cleanup(root);
  }
});

test('reads plan/ from a git ref of a scratch repository, not the working tree', () => {
  const root = writePlan(basePlan());
  try {
    initRepo(root);
    writeFileSync(join(root, 'plan', 'features', 'fork.md'), 'changed, not committed\n');
    writeFileSync(join(root, 'untracked.txt'), 'x');
    assertBase(readPlanAt(root, 'HEAD'));
    assert.equal(readPlan(join(root, 'plan')).features.fork.notes, 'changed, not committed\n');
    git(root, 'commit', '-q', '-am', 'Notes');
    assert.equal(readPlanAt(root, 'HEAD').features.fork.notes, 'changed, not committed\n');
    assertBase(readPlanAt(root, 'HEAD~1'));
    assert.equal(loadRef(root, 'HEAD').has('untracked.txt'), false);
    assert.throws(() => readPlanAt(root, 'no-such-ref'));
  } finally {
    cleanup(root);
  }
});

test('a missing file or a file that is not JSON stops the strict readers', () => {
  const files = basePlan();
  delete files['areas.json'];
  files['features/core.json'] = '{';
  const root = writePlan(files);
  try {
    assert.throws(() => readPlan(join(root, 'plan')), (error) => error instanceof PlanError && error.problems.length === 2 && /plan\/areas\.json: -: missing/.test(error.message));
  } finally {
    cleanup(root);
  }
});

test('reads plan/ from a git ref given any directory inside the repository', () => {
  const root = writePlan(basePlan());
  try {
    mkdirSync(join(root, 'website', 'src'), { recursive: true });
    writeFileSync(join(root, 'website', 'src', 'x.ts'), 'x');
    initRepo(root);
    for (const dir of [join(root, 'website'), join(root, 'website', 'src'), join(root, 'plan', 'features')]) {
      assertBase(readPlanAt(dir, 'HEAD'));
      assert.equal(loadRef(dir, 'HEAD').size, 9);
    }
  } finally {
    cleanup(root);
  }
});

test('dotfiles in plan/ are not read, from a directory or a git ref', () => {
  const root = writePlan(basePlan());
  try {
    writeFileSync(join(root, 'plan', '.DS_Store'), 'x');
    writeFileSync(join(root, 'plan', 'features', '.draft.json'), '{');
    initRepo(root);
    assertBase(readPlan(join(root, 'plan')));
    assertBase(readPlanAt(root, 'HEAD'));
    assert.equal(loadRef(root, 'HEAD').has('.DS_Store'), false);
  } finally {
    cleanup(root);
  }
});
