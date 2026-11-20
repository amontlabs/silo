import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, readFileSync, renameSync, rmSync, symlinkSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { PlanError, loadDir, loadRef, parsePlan, readPlan, readPlanAt } from '../read/index.js';
import { SAMPLE_DIR, basePlan, cleanup, filesOf, git, initRepo, temp, writePlan } from './helpers.js';

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

test('a symbolic link under plan/ is an error and is never followed: a file, a folder, plan/ itself, on disk and at a ref', () => {
  const outside = temp('plan-outside-');
  const root = writePlan(basePlan());
  try {
    writeFileSync(join(outside, 'x.md'), 'outside\n');
    mkdirSync(join(outside, 'features'));
    writeFileSync(join(outside, 'features', 'y.json'), '{}\n');
    const isLink = (path) => (error) => error instanceof PlanError && error.problems.some((p) => p.file === path && /symbolic link/.test(p.message));
    // A file.
    symlinkSync(join(outside, 'x.md'), join(root, 'plan', 'engine', 'decisions', 'engine-language.md'));
    assert.throws(() => loadDir(join(root, 'plan')), isLink('plan/engine/decisions/engine-language.md'));
    // A folder, even one whose files would be valid.
    rmSync(join(root, 'plan', 'engine', 'decisions', 'engine-language.md'));
    symlinkSync(join(outside, 'features'), join(root, 'plan', 'record', 'features'));
    assert.throws(() => readPlan(join(root, 'plan')), isLink('plan/record/features'));
    // At a ref: git keeps the link as a link.
    initRepo(root);
    assert.throws(() => loadRef(root, 'HEAD'), isLink('plan/record/features'));
    // plan/ itself.
    const linked = temp('plan-linked-');
    symlinkSync(join(root, 'plan'), join(linked, 'plan'));
    rmSync(join(root, 'plan', 'record', 'features'));
    assert.throws(() => loadDir(join(linked, 'plan')), isLink('plan/'));
    cleanup(linked);
  } finally {
    cleanup(root);
    cleanup(outside);
  }
});

test('a path holding a newline at a git ref is read like any other, and the files after it too', () => {
  const root = writePlan(basePlan());
  try {
    // Sorted by git between decisions/the-design.json and engine/: it comes before most entries.
    writeFileSync(join(root, 'plan', 'decisions', 'x\ny.md'), 'two\nlines\n');
    writeFileSync(join(root, 'plan', 'decisions', 'a\nb'), 'second\n');
    initRepo(root);
    const files = loadRef(root, 'HEAD');
    const disk = loadDir(join(root, 'plan'));
    assert.deepEqual(new Map([...files].sort()), new Map([...disk].sort()));
    assert.equal(files.get('decisions/x\ny.md'), 'two\nlines\n');
    assert.equal(files.get('engine/decisions/engine-language.json'), filesOf(basePlan()).get('engine/decisions/engine-language.json'));
  } finally {
    cleanup(root);
  }
});

test('a dot-named symbolic link under plan/ is an error too, on disk and at a ref, and one inside a dot-named folder', () => {
  const outside = temp('plan-outside-');
  const root = writePlan(basePlan());
  try {
    writeFileSync(join(outside, 'x.md'), 'outside\n');
    const isLink = (path) => (error) => error instanceof PlanError && error.problems.some((p) => p.file === path && /symbolic link/.test(p.message));
    symlinkSync(outside, join(root, 'plan', '.cache'));
    assert.throws(() => loadDir(join(root, 'plan')), isLink('plan/.cache'));
    initRepo(root);
    assert.throws(() => loadRef(root, 'HEAD'), isLink('plan/.cache'));
    rmSync(join(root, 'plan', '.cache'));
    mkdirSync(join(root, 'plan', '.drafts'));
    symlinkSync(join(outside, 'x.md'), join(root, 'plan', '.drafts', 'x.md'));
    assert.throws(() => loadDir(join(root, 'plan')), isLink('plan/.drafts/x.md'));
    git(root, 'add', '-A');
    git(root, 'commit', '-q', '-m', 'Drafts');
    assert.throws(() => loadRef(root, 'HEAD'), isLink('plan/.drafts/x.md'));
    // A dot-named regular file is still skipped.
    rmSync(join(root, 'plan', '.drafts'), { recursive: true });
    writeFileSync(join(root, 'plan', '.DS_Store'), 'x');
    assert.equal(loadDir(join(root, 'plan')).has('.DS_Store'), false);
  } finally {
    cleanup(root);
    cleanup(outside);
  }
});

test('a committed plan that is itself a symbolic link is an error at a ref, not an empty plan', () => {
  const root = writePlan(basePlan());
  try {
    renameSync(join(root, 'plan'), join(root, 'real'));
    symlinkSync('real', join(root, 'plan'));
    initRepo(root);
    const isLink = (error) => error instanceof PlanError && error.problems.some((p) => p.file === 'plan/' && /symbolic link/.test(p.message));
    assert.throws(() => loadRef(root, 'HEAD'), isLink);
    assert.throws(() => readPlanAt(root, 'HEAD'), isLink);
  } finally {
    cleanup(root);
  }
});

test('bytes that are not valid UTF-8 are an error, on disk and at a ref; a real U+FFFD passes', () => {
  const root = writePlan(basePlan());
  try {
    const bad = (error) => error instanceof PlanError && error.problems.some((p) => p.file === 'plan/engine/README.md' && /not valid UTF-8/.test(p.message));
    writeFileSync(join(root, 'plan', 'engine', 'README.md'), Buffer.from([0x45, 0x6e, 0xff, 0x0a]));
    assert.throws(() => loadDir(join(root, 'plan')), bad);
    initRepo(root);
    assert.throws(() => loadRef(root, 'HEAD'), bad);
    writeFileSync(join(root, 'plan', 'engine', 'README.md'), 'A real �, and a BOM ﻿.\n');
    git(root, 'commit', '-q', '-am', 'Real');
    assert.equal(loadDir(join(root, 'plan')).get('engine/README.md'), 'A real �, and a BOM ﻿.\n');
    assert.equal(loadRef(root, 'HEAD').get('engine/README.md'), 'A real �, and a BOM ﻿.\n');
  } finally {
    cleanup(root);
  }
});
