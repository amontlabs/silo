import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { PlanError, loadRef, parsePlan, readPlan, readPlanAt } from '../read/index.js';
import { SAMPLE_DIR, basePlan, cleanup, filesOf, git, initRepo, writePlan } from './helpers.js';

const sample = (name) => JSON.parse(readFileSync(join(SAMPLE_DIR, name), 'utf8'));
const notes = (name) => readFileSync(join(SAMPLE_DIR, name), 'utf8');

/** The contract's plan object for the sample tree, written out: kind, part, stage and position by hand, the keys from the files. */
function expectedSample() {
  const entry = (id, kind, part, stage, position, withNotes) => {
    const path = `${part ? `${part}/` : ''}${kind === 'decision' ? 'decisions' : 'features'}/${id}`;
    return { id, kind, part, stage, position, ...sample(`${path}.json`), ...(withNotes ? { notes: notes(`${path}.md`) } : {}) };
  };
  const entries = Object.assign(Object.create(null), Object.fromEntries([
    entry('the-design', 'decision', null, 'ground', 1, false),
    entry('engine-language', 'decision', 'engine', 'ground', 2, false),
    entry('nothing-without-a-reason', 'decision', null, 'ground', 3, false),
    entry('losses-show', 'decision', 'record', 'ground', 4, false),
    entry('no-detour', 'decision', 'record', 'ground', 5, true),
    entry('model-tool', 'decision', 'engine', 'ground', 6, true),
    entry('linux-vm-engine', 'decision', 'engine', 'ground', 7, true),
    entry('start-computer', 'feature', 'engine', 'app', 8, true),
  ].map((e) => [e.id, e])));
  return {
    parts: sample('parts.json'),
    words: sample('words.json').map((word) => ({ ...word, id: word.word })),
    principles: sample('principles.json'),
    properties: sample('record/properties.json').map((property) => ({ ...property, part: 'record' })),
    stages: sample('order.json').stages,
    entries,
  };
}

test('the sample tree reads as the contract plan object', () => {
  const plan = readPlan(SAMPLE_DIR);
  assert.deepEqual(plan, expectedSample());
  assert.deepEqual(plan.stages.map((stage) => [stage.id, stage.entries.length]), [['ground', 7], ['app', 1], ['later', 0]]);
  assert.equal(Object.hasOwn(plan.entries['losses-show'], 'notes'), false);
  assert.equal(plan.words.find((word) => word.word === 'boundary').id, 'boundary');
  // An entry id that is also a name on Object.prototype is still only an entry.
  assert.equal(plan.entries.constructor, undefined);
});

test('reading the tree at a git ref gives the same object as reading it from disk', () => {
  const root = writePlan(basePlan());
  try {
    initRepo(root);
    assert.deepEqual(readPlanAt(root, 'HEAD'), readPlan(SAMPLE_DIR));
    assert.deepEqual(readPlan(join(root, 'plan')), readPlan(SAMPLE_DIR));
    // The ref, not the working tree.
    writeFileSync(join(root, 'plan', 'record', 'decisions', 'no-detour.md'), 'changed, not committed\n');
    writeFileSync(join(root, 'untracked.txt'), 'x');
    assert.deepEqual(readPlanAt(root, 'HEAD'), readPlan(SAMPLE_DIR));
    assert.equal(readPlan(join(root, 'plan')).entries['no-detour'].notes, 'changed, not committed\n');
    git(root, 'commit', '-q', '-am', 'Notes');
    assert.equal(readPlanAt(root, 'HEAD').entries['no-detour'].notes, 'changed, not committed\n');
    assert.deepEqual(readPlanAt(root, 'HEAD~1'), readPlan(SAMPLE_DIR));
    assert.equal(loadRef(root, 'HEAD').has('untracked.txt'), false);
    assert.throws(() => readPlanAt(root, 'no-such-ref'));
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
    for (const dir of [join(root, 'website'), join(root, 'website', 'src'), join(root, 'plan', 'record', 'decisions')]) {
      assert.deepEqual(readPlanAt(dir, 'HEAD'), readPlan(SAMPLE_DIR));
      assert.equal(loadRef(dir, 'HEAD').size, Object.keys(basePlan()).length);
    }
  } finally {
    cleanup(root);
  }
});

test('a missing list or a file that is not JSON stops the strict readers', () => {
  const files = basePlan();
  delete files['parts.json'];
  files['record/decisions/no-detour.json'] = '{';
  const root = writePlan(files);
  try {
    assert.throws(
      () => readPlan(join(root, 'plan')),
      (error) => error instanceof PlanError && error.problems.length === 2 && /plan\/parts\.json: -: missing/.test(error.message) && /no-detour\.json: -: not JSON/.test(error.message),
    );
  } finally {
    cleanup(root);
  }
});

test('dotfiles in plan/ are not read, from a directory or a git ref', () => {
  const root = writePlan(basePlan());
  try {
    writeFileSync(join(root, 'plan', '.DS_Store'), 'x');
    writeFileSync(join(root, 'plan', 'engine', 'features', '.draft.json'), '{');
    mkdirSync(join(root, 'plan', '.cache', 'decisions'), { recursive: true });
    writeFileSync(join(root, 'plan', '.cache', 'decisions', 'x.json'), '{}');
    initRepo(root);
    assert.deepEqual(readPlan(join(root, 'plan')), readPlan(SAMPLE_DIR));
    assert.deepEqual(readPlanAt(root, 'HEAD'), readPlan(SAMPLE_DIR));
    assert.equal(loadRef(root, 'HEAD').has('.DS_Store'), false);
  } finally {
    cleanup(root);
  }
});

test('parsePlan: where each file goes, as far as the files parse', () => {
  const files = basePlan();
  files['properties.json'] = [{ id: 'root-one', says: 'S.', kind: 'safety', check: 'types' }];
  files['engine/properties.json'] = [{ id: 'engine-one', says: 'S.', kind: 'liveness', check: 'model', at: 'x' }];
  files['decisions/outside.json'] = { asks: 'Not in the order?' };
  files['features/stray.json'] = { name: 'Root feature', what: 'Not a place for one.' };
  files['engine/notes.json'] = { x: 1 };
  files['words.json'] = [...files['words.json'], { word: 'Memory Snapshot', means: 'M.' }, 'not an object'];
  const { plan, problems, json } = parsePlan(filesOf(files));
  assert.deepEqual(problems, []);
  assert.deepEqual(plan.properties.map((p) => [p.id, p.part]), [
    ['root-one', null], ['engine-one', 'engine'], ['rule-reads-only-facts', 'record'], ['nothing-told-passes-as-seen', 'record'], ['precaution-grants-nothing', 'record'], ['classifier-never-acts', 'record'],
  ]);
  assert.deepEqual(plan.entries.outside, { id: 'outside', kind: 'decision', part: null, stage: null, position: null, asks: 'Not in the order?' });
  // A feature at the root and a file of no known kind are the check's to report; they are no entry.
  assert.equal(plan.entries.stray, undefined);
  assert.equal(plan.words.at(-1).id, 'memory-snapshot');
  assert.equal(plan.words.length, 10);
  // Every file it read that parsed, as it is; a file of no known kind is not read.
  assert.deepEqual(json.get('decisions/outside.json'), files['decisions/outside.json']);
  assert.deepEqual(json.get('words.json'), files['words.json']);
  assert.equal(json.has('engine/notes.json'), false);
  assert.equal(json.has('README.md'), false);
});

test('parsePlan: the same entry id twice keeps the first by path; keys of the file never replace the reader\'s', () => {
  const files = basePlan();
  files['record/decisions/model-tool.json'] = { asks: 'Again?', kind: 'feature', part: 'x', position: 99 };
  files['engine/features/start-computer.json'] = { ...files['engine/features/start-computer.json'], stage: 'later' };
  const { plan } = parsePlan(filesOf(files));
  assert.equal(plan.entries['model-tool'].part, 'engine');
  assert.equal(plan.entries['start-computer'].stage, 'app');
  assert.equal(plan.entries['start-computer'].kind, 'feature');
});
