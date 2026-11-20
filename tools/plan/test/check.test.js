import { test } from 'node:test';
import assert from 'node:assert/strict';
import { checkChange, checkFiles, formatProblem } from '../check/index.js';
import { basePlan, filesOf } from './helpers.js';

/** The sample plan with one change: `edit` gets a deep copy and returns nothing or the files. */
function variant(edit) {
  const files = structuredClone(basePlan());
  return edit(files) ?? files;
}

/** Each error as `file field: message`, the fix left out. */
const errorsOf = (files, options) => checkFiles(filesOf(files), options).errors.map((e) => `${e.file} ${e.field}: ${e.message}`);

const stage = (files, id) => files['order.json'].stages.find((s) => s.id === id);
const WORD_IDS = ['computer', 'fork', 'boundary', 'channel', 'fact', 'report', 'opinion', 'rule', 'precaution'];

test('the sample plan passes with no error', () => {
  const { errors, notes } = checkFiles(filesOf(basePlan()), { exists: () => true });
  assert.deepEqual(errors, []);
  assert.deepEqual(notes, []);
});

test('the folder: a file that is not the plan, a missing list, a part without its page', () => {
  assert.deepEqual(errorsOf(variant((f) => { f['DESIGN.md'] = '# Design\n'; })), ['plan/DESIGN.md -: not a file of the plan']);
  assert.deepEqual(errorsOf(variant((f) => { f['features/x.json'] = { name: 'N', what: 'W' }; })), ['plan/features/x.json -: not a file of the plan']);
  assert.deepEqual(errorsOf(variant((f) => { f['nowhere/decisions/x.json'] = { asks: 'Q?' }; })), ['plan/nowhere/decisions/x.json -: not a file of the plan']);
  assert.deepEqual(errorsOf(variant((f) => {
    delete f['words.json'];
    f['decisions/the-design.json'].adds = f['decisions/the-design.json'].adds.filter((id) => !WORD_IDS.includes(id));
  })).filter((e) => !e.includes('is not a sentence')), ['plan/words.json -: missing']);
  assert.deepEqual(errorsOf(variant((f) => { delete f['engine/README.md']; })), ['plan/engine/README.md -: missing: every part has its page']);
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/ghost.md'] = 'x'; })), ['plan/engine/decisions/ghost.md -: notes without an entry file']);
  assert.deepEqual(errorsOf(variant((f) => { f['parts.json'].push({ id: 'decisions', title: 'T', line: 'L' }); f['decisions/README.md'] = 'x'; })), [
    'plan/parts.json [2].id: "decisions" is not allowed as a part id',
  ]);
});

test('files whose name starts with a dot are ignored', () => {
  assert.deepEqual(errorsOf(variant((f) => { f['.DS_Store'] = 'x'; f['engine/.draft.json'] = '{'; })), []);
});

test('rule 1: one namespace of ids, across entries and sentences', () => {
  // A decision named like a word.
  assert.deepEqual(errorsOf(variant((f) => {
    f['engine/decisions/fork.json'] = { asks: 'How is a computer forked?' };
    stage(f, 'later').entries.push('fork');
  })), ['plan/words.json [1].word: the id "fork" is also plan/engine/decisions/fork.json']);
  // A property named like a principle.
  assert.deepEqual(errorsOf(variant((f) => {
    f['record/properties.json'][0].id = 'less-sure-less-power';
    f['decisions/the-design.json'].adds = f['decisions/the-design.json'].adds.filter((id) => id !== 'rule-reads-only-facts');
  })), ['plan/record/properties.json [0].id: the id "less-sure-less-power" is also plan/principles.json [0]']);
  // The same id in two folders.
  assert.deepEqual(errorsOf(variant((f) => { f['record/decisions/model-tool.json'] = { asks: 'Q?' }; })), ['plan/record/decisions/model-tool.json -: the id "model-tool" is also plan/engine/decisions/model-tool.json']);
  // A file name that is not an id.
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/Bad_Id.json'] = { asks: 'Q?' }; })), ['plan/engine/decisions/Bad_Id.json -: "Bad_Id" is not an id (^[a-z0-9][a-z0-9-]{0,62}$)']);
  // A sentence an entry sets, named like an entry.
  assert.deepEqual(errorsOf(variant((f) => { f['decisions/nothing-without-a-reason.json'].sets[0].id = 'model-tool'; })), [
    'plan/decisions/nothing-without-a-reason.json sets[0].id: the id "model-tool" is also plan/engine/decisions/model-tool.json',
  ]);
});

test('rule 2: canonical JSON, known keys, no empty optional key', () => {
  const decision = { verdict: 'take', asks: 'Which language is the engine written in?', answer: 'Zig.' };
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/engine-language.json'] = `${JSON.stringify(decision, null, 2)}\n`; })), ['plan/engine/decisions/engine-language.json -: not in canonical form']);
  assert.deepEqual(errorsOf(variant((f) => { f['parts.json'] = JSON.stringify(f['parts.json'], null, 2); })), ['plan/parts.json -: not in canonical form']);
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/model-tool.json'] = '{ "asks": '; })).map((e) => e.replace(/\(.*\)/, '()')), ['plan/engine/decisions/model-tool.json -: not JSON ()']);
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/model-tool.json'].owner = 'me'; })), ['plan/engine/decisions/model-tool.json owner: unknown key']);
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/model-tool.json'].needs = []; })), ['plan/engine/decisions/model-tool.json needs: empty']);
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/model-tool.json'].answer = null; })), ['plan/engine/decisions/model-tool.json answer: empty']);
  assert.deepEqual(errorsOf(variant((f) => { f['principles.json'][0].known_as = ''; })), ['plan/principles.json [0].known_as: empty']);
  assert.deepEqual(errorsOf(variant((f) => { f['engine/features/start-computer.json'].surfaces = {}; })), ['plan/engine/features/start-computer.json surfaces: empty']);
});

test('rule 3: required keys, and a property\'s check and at', () => {
  assert.deepEqual(errorsOf(variant((f) => { delete f['engine/decisions/model-tool.json'].asks; })), ['plan/engine/decisions/model-tool.json asks: missing or empty']);
  assert.deepEqual(errorsOf(variant((f) => { delete f['engine/features/start-computer.json'].what; })), ['plan/engine/features/start-computer.json what: missing or empty']);
  assert.deepEqual(errorsOf(variant((f) => { delete f['words.json'][0].means; })), ['plan/words.json [0].means: missing or empty']);
  assert.deepEqual(errorsOf(variant((f) => { delete f['principles.json'][0].asks; })), ['plan/principles.json [0].asks: missing or empty']);
  assert.deepEqual(errorsOf(variant((f) => { delete f['record/properties.json'][0].kind; })), ['plan/record/properties.json [0].kind: missing or empty']);
  // A principle's why is optional.
  assert.deepEqual(errorsOf(variant((f) => { delete f['principles.json'][0].why; })), []);
  const property = (f) => f['record/properties.json'][0];
  assert.deepEqual(errorsOf(variant((f) => { property(f).check = 'simulator'; })), ['plan/record/properties.json [0].check: not a list']);
  assert.deepEqual(errorsOf(variant((f) => { property(f).check = []; })), ['plan/record/properties.json [0].check: missing or empty']);
  assert.deepEqual(errorsOf(variant((f) => { property(f).check = ['simulator', 'unit']; })), ['plan/record/properties.json [0].check: "unit" is not a check']);
  assert.deepEqual(errorsOf(variant((f) => { property(f).check = ['test', 'test']; })), ['plan/record/properties.json [0].check[1]: "test" appears twice']);
  assert.deepEqual(errorsOf(variant((f) => { property(f).check = ['test', 'assertion', 'hostile test']; })), []);
  assert.deepEqual(errorsOf(variant((f) => { property(f).at = 'engine/src/rule.zig'; })), ['plan/record/properties.json [0].at: not a list']);
  assert.deepEqual(errorsOf(variant((f) => { property(f).at = ['a.zig', 'a.zig']; })), ['plan/record/properties.json [0].at[1]: "a.zig" appears twice']);
  assert.deepEqual(errorsOf(variant((f) => { property(f).kind = 'nice'; })), ['plan/record/properties.json [0].kind: "nice" is not a kind']);
});

test('rule 4: verdict and suggest are take or drop', () => {
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/model-tool.json'].verdict = 'later'; })), ['plan/engine/decisions/model-tool.json verdict: "later" is not a verdict']);
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/model-tool.json'].suggest = { verdict: 'refine', why: 'W.' }; })), ['plan/engine/decisions/model-tool.json suggest.verdict: "refine" is not a verdict']);
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/model-tool.json'].suggest = { verdict: 'take' }; })), ['plan/engine/decisions/model-tool.json suggest.why: missing or empty']);
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/model-tool.json'].suggest = { verdict: 'drop', why: 'W.' }; })), []);
});

test('rule 5: sets and removes before take, adds only with it, each sets item in a list', () => {
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/engine-language.json'].removes = ['fork']; })), ['plan/engine/decisions/engine-language.json removes: removes on a taken entry']);
  assert.deepEqual(errorsOf(variant((f) => { f['record/decisions/losses-show.json'].verdict = 'take'; })), ['plan/record/decisions/losses-show.json sets: sets on a taken entry']);
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/model-tool.json'].adds = ['fork']; })), ['plan/engine/decisions/model-tool.json adds: adds without the verdict take']);
  assert.deepEqual(errorsOf(variant((f) => { f['record/decisions/losses-show.json'].sets[0].in = 'nowhere/properties'; })), ['plan/record/decisions/losses-show.json sets[0].in: "nowhere" is not a part']);
  assert.deepEqual(errorsOf(variant((f) => { f['record/decisions/losses-show.json'].sets[0].in = 'facts'; })), ['plan/record/decisions/losses-show.json sets[0].in: "facts" is not a list']);
  assert.deepEqual(errorsOf(variant((f) => { delete f['record/decisions/losses-show.json'].sets[0].kind; })), ['plan/record/decisions/losses-show.json sets[0].kind: missing or empty']);
  // A sets item may replace a sentence of its own list, not take an id of another list.
  assert.deepEqual(errorsOf(variant((f) => { f['record/decisions/losses-show.json'].sets[0].id = 'classifier-never-acts'; })), []);
  assert.deepEqual(errorsOf(variant((f) => { f['decisions/nothing-without-a-reason.json'].sets[0].id = 'classifier-never-acts'; })), [
    'plan/decisions/nothing-without-a-reason.json sets[0].id: the id "classifier-never-acts" is already a sentence of record/properties',
  ]);
});

test('rule 6: every sentence is added by exactly one taken entry', () => {
  const design = (f) => f['decisions/the-design.json'];
  assert.deepEqual(errorsOf(variant((f) => { design(f).adds = design(f).adds.filter((id) => id !== 'fork'); })), ['plan/words.json [1].word: "fork" is added by no taken entry']);
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/engine-language.json'].adds = ['fork']; })), ['plan/words.json [1].word: "fork" is added by the-design, engine-language']);
  assert.deepEqual(errorsOf(variant((f) => { design(f).adds.push('vm'); })), ['plan/decisions/the-design.json adds[14]: "vm" is not a sentence']);
  // A principle in its list that no taken entry brought.
  assert.deepEqual(errorsOf(variant((f) => { f['principles.json'].push({ id: 'least-privilege', says: 'S.', asks: 'A?' }); })), ['plan/principles.json [1].id: "least-privilege" is added by no taken entry']);
});

test('rule 7: from, keeps, needs and removes name what exists; a property with no from is a note', () => {
  assert.deepEqual(errorsOf(variant((f) => { f['record/properties.json'][0].from = 'nobody'; })), ['plan/record/properties.json [0].from: "nobody" is not a principle']);
  assert.deepEqual(errorsOf(variant((f) => { f['engine/features/start-computer.json'].keeps = ['nothing-at-all']; })), ['plan/engine/features/start-computer.json keeps[0]: "nothing-at-all" is not a property']);
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/model-tool.json'].needs = ['nobody']; })), ['plan/engine/decisions/model-tool.json needs[0]: "nobody" is not a decision or a feature']);
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/model-tool.json'].removes = ['nothing']; })), ['plan/engine/decisions/model-tool.json removes[0]: "nothing" is not a sentence']);
  assert.deepEqual(errorsOf(variant((f) => { f['record/decisions/losses-show.json'].sets[0].from = 'nobody'; })), ['plan/record/decisions/losses-show.json sets[0].from: "nobody" is not a principle']);
  const { errors, notes } = checkFiles(filesOf(variant((f) => { delete f['record/properties.json'][0].from; })), { exists: () => true });
  assert.deepEqual(errors, []);
  assert.deepEqual(notes.map((n) => `${n.file} ${n.field}: ${n.message}`), ['plan/record/properties.json [0]: the property "rule-reads-only-facts" follows from no principle']);
});

test('rule 8: the order', () => {
  assert.deepEqual(errorsOf(variant((f) => { stage(f, 'app').entries = []; stage(f, 'ground').entries.unshift('start-computer'); })), [
    'plan/order.json start-computer: "start-computer" comes before "engine-language", which it needs',
    'plan/order.json start-computer: "start-computer" comes before "linux-vm-engine", which it needs',
  ]);
  assert.deepEqual(errorsOf(variant((f) => { stage(f, 'later').entries.push('model-tool'); })), ['plan/order.json stages[2].entries[0]: "model-tool" is placed twice']);
  assert.deepEqual(errorsOf(variant((f) => { stage(f, 'app').entries = []; })), ['plan/engine/features/start-computer.json -: "start-computer" is in no stage']);
  assert.deepEqual(errorsOf(variant((f) => { stage(f, 'later').entries.push('nobody'); })), ['plan/order.json stages[2].entries[0]: "nobody" is not a decision or a feature']);
  assert.deepEqual(errorsOf(variant((f) => { stage(f, 'later').id = 'app'; })), ['plan/order.json stages[2].id: "app" appears twice']);
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/linux-vm-engine.json'].verdict = 'drop'; })), ['plan/engine/features/start-computer.json needs[1]: "linux-vm-engine" is dropped']);
  // A cycle is an order that cannot be kept.
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/engine-language.json'].needs = ['start-computer']; })), [
    'plan/order.json engine-language: "engine-language" comes before "start-computer", which it needs',
  ]);
  // Stage ids are not in the namespace of entries and sentences, and none is reserved.
  assert.deepEqual(errorsOf(variant((f) => { stage(f, 'later').id = 'gone'; })), []);
  assert.deepEqual(errorsOf(variant((f) => { stage(f, 'later').id = 'fork'; })), []);
});

test('rule 9: built, and the files named by at', () => {
  const feature = (f) => f['engine/features/start-computer.json'];
  assert.deepEqual(errorsOf(variant((f) => { feature(f).built = '2026-11-20'; })), ['plan/engine/features/start-computer.json built: built while linux-vm-engine is not done']);
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/linux-vm-engine.json'].verdict = 'take'; feature(f).built = '2026-11-20'; })), []);
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/linux-vm-engine.json'].verdict = 'take'; delete feature(f).verdict; feature(f).built = '2026-11-20'; })), ['plan/engine/features/start-computer.json built: built without the verdict take']);
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/linux-vm-engine.json'].verdict = 'take'; feature(f).built = '2026-02-30'; })), ['plan/engine/features/start-computer.json built: "2026-02-30" is not a date']);
  // A built feature's properties each have an at.
  const builtWithProperty = (f) => {
    f['engine/decisions/linux-vm-engine.json'].verdict = 'take';
    feature(f).adds = ['classifier-never-acts'];
    feature(f).built = '2026-11-20';
    f['decisions/the-design.json'].adds = f['decisions/the-design.json'].adds.filter((id) => id !== 'classifier-never-acts');
  };
  assert.deepEqual(errorsOf(variant(builtWithProperty)), ['plan/record/properties.json [3].at: the property "classifier-never-acts" of the built feature "start-computer" has no at']);
  const withAt = variant((f) => { builtWithProperty(f); f['record/properties.json'][3].at = ['engine/test/classifier.zig']; });
  assert.deepEqual(errorsOf(withAt, { exists: (path) => path === 'engine/test/classifier.zig' }), []);
  assert.deepEqual(errorsOf(withAt, { exists: () => false }), ['plan/record/properties.json [3].at[0]: "engine/test/classifier.zig" does not exist']);
  // Without exists, the file test is left out and a note says so.
  const { errors, notes } = checkFiles(filesOf(withAt));
  assert.deepEqual(errors, []);
  assert.deepEqual(notes.map((n) => n.message), ['the files named by at were not looked for']);
});

test('rule 10: the words under not are refused on word boundaries, in any case, except inside a not list', () => {
  assert.deepEqual(errorsOf(variant((f) => { f['engine/README.md'] += '\nThe Wall stops it.\n'; })), ['plan/engine/README.md -: "wall" is not a word of the plan']);
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/model-tool.json'].asks = 'Which tool checks every DOOR?'; })), ['plan/engine/decisions/model-tool.json asks: "door" is not a word of the plan']);
  assert.deepEqual(errorsOf(variant((f) => { f['record/decisions/losses-show.json'].sets[0].says = 'A guess is never a fact.'; })), ['plan/record/decisions/losses-show.json sets[0].says: "guess" is not a word of the plan']);
  // Inside another word, it is not the word.
  assert.deepEqual(errorsOf(variant((f) => { f['engine/README.md'] += '\nA wallet, doors, a watchdog, labels.\n'; })), []);
  // A phrase: matched across any run of spaces, and only whole.
  const phrase = (f) => { f['words.json'][2].not.push('hard edge'); };
  assert.deepEqual(errorsOf(variant((f) => { phrase(f); f['PRODUCT.md'] += '\nIt is a Hard\n  Edge.\n'; })), ['plan/PRODUCT.md -: "hard edge" is not a word of the plan']);
  assert.deepEqual(errorsOf(variant((f) => { phrase(f); f['PRODUCT.md'] += '\nA hard edges case, an edge.\n'; })), []);
  // Inside a not list, here of a word an entry sets, the words are allowed.
  assert.deepEqual(errorsOf(variant((f) => {
    f['engine/decisions/model-tool.json'].sets = [{ in: 'words', word: 'gate', means: 'A part of the boundary.', not: ['wall'] }];
  })), []);
});

test('rule 11: an entry whose sentence changed while its verdict stayed', () => {
  const base = filesOf(basePlan());
  const changeOf = (edit) => checkChange(filesOf(variant(edit)), base).errors.map((e) => `${e.file} ${e.field}: ${e.message}`);
  assert.deepEqual(changeOf((f) => { f['engine/decisions/engine-language.json'].answer = 'Rust.'; }), ['plan/engine/decisions/engine-language.json answer: "engine-language" changed its answer while its verdict stayed take']);
  assert.deepEqual(changeOf((f) => { f['engine/features/start-computer.json'].what = 'Boot a computer.'; }), ['plan/engine/features/start-computer.json what: "start-computer" changed its what while its verdict stayed take']);
  const dropped = variant((f) => { f['record/decisions/losses-show.json'].verdict = 'drop'; });
  const droppedChange = structuredClone(dropped);
  droppedChange['record/decisions/losses-show.json'].sets[0].says = 'Every loss is recorded.';
  assert.deepEqual(checkChange(filesOf(droppedChange), filesOf(dropped)).errors.map((e) => e.field), ['sets']);
  // No verdict: free to change. A verdict that changes with it: the owner's new word.
  assert.deepEqual(changeOf((f) => { f['engine/decisions/linux-vm-engine.json'].answer = 'Firecracker.'; }), []);
  assert.deepEqual(changeOf((f) => { delete f['engine/decisions/engine-language.json'].verdict; f['engine/decisions/engine-language.json'].answer = 'Rust.'; }), []);
  // Keys outside the sentence may change.
  assert.deepEqual(changeOf((f) => { f['engine/features/start-computer.json'].surfaces = { app: 'Start' }; }), []);
  // The offline check does not look at the base.
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/engine-language.json'].answer = 'Rust.'; })), []);
});

test('a problem reads file: field: message. Fix: …', () => {
  const [error] = checkFiles(filesOf(variant((f) => { f['engine/decisions/model-tool.json'].verdict = 'later'; }))).errors;
  assert.equal(formatProblem(error), 'plan/engine/decisions/model-tool.json: verdict: "later" is not a verdict. Fix: write take or drop, or leave it out');
});
