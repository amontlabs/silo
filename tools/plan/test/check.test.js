import { test } from 'node:test';
import assert from 'node:assert/strict';
import { checkChange, checkFiles, formatProblem, ruledKeysDiffer } from '../check/index.js';
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
  })).filter((e) => !e.includes('is not a statement')), ['plan/words.json -: missing']);
  assert.deepEqual(errorsOf(variant((f) => { delete f['engine/README.md']; })), ['plan/engine/README.md -: missing: every part has its page']);
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/ghost.md'] = 'x'; })), ['plan/engine/decisions/ghost.md -: notes without an entry file']);
  assert.deepEqual(errorsOf(variant((f) => { f['parts.json'].push({ id: 'decisions', title: 'T', line: 'L' }); f['decisions/README.md'] = 'x'; })), [
    'plan/parts.json [2].id: "decisions" is not allowed as a part id',
  ]);
});

test('a part id that is also an entry, a principle or a property is refused, and so is one set under a part id; a word may share it', () => {
  const part = (id) => (f) => { f['parts.json'].push({ id, title: 'T', line: 'L' }); f[`${id}/README.md`] = 'x'; };
  assert.deepEqual(errorsOf(variant(part('model-tool'))), ['plan/parts.json [2].id: the part id "model-tool" is also plan/engine/decisions/model-tool.json']);
  assert.deepEqual(errorsOf(variant(part('less-sure-less-power'))), ['plan/parts.json [2].id: the part id "less-sure-less-power" is also the principle at plan/principles.json [0]']);
  assert.deepEqual(errorsOf(variant(part('rule-reads-only-facts'))), ['plan/parts.json [2].id: the part id "rule-reads-only-facts" is also the property at plan/record/properties.json [0]']);
  assert.deepEqual(errorsOf(variant((f) => { f['record/decisions/losses-show.json'].sets[0].id = 'engine'; })), [
    'plan/record/decisions/losses-show.json sets[0].id: the id "engine" is also a part of plan/parts.json',
  ]);
  assert.deepEqual(errorsOf(variant(part('fork'))), []);
});

test('files whose name starts with a dot are ignored', () => {
  assert.deepEqual(errorsOf(variant((f) => { f['.DS_Store'] = 'x'; f['engine/.draft.json'] = '{'; })), []);
});

test('rule 1: one namespace of ids, across entries and statements', () => {
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
  // A statement an entry sets, named like an entry.
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
  // A sets item may replace a statement of its own list, or move one of another list of the same kind; never take an id of another kind.
  assert.deepEqual(errorsOf(variant((f) => { f['record/decisions/losses-show.json'].sets[0].id = 'classifier-never-acts'; })), []);
  assert.deepEqual(errorsOf(variant((f) => { Object.assign(f['record/decisions/losses-show.json'].sets[0], { in: 'engine/properties', id: 'classifier-never-acts' }); })), []);
  assert.deepEqual(errorsOf(variant((f) => { Object.assign(f['record/decisions/losses-show.json'].sets[0], { in: 'properties', id: 'classifier-never-acts' }); })), []);
  assert.deepEqual(errorsOf(variant((f) => { f['decisions/nothing-without-a-reason.json'].sets[0].id = 'classifier-never-acts'; })), [
    'plan/decisions/nothing-without-a-reason.json sets[0].id: the id "classifier-never-acts" is already a property of record/properties',
  ]);
  // A taken decision has an answer or an adds, and every decision it needs is taken.
  assert.deepEqual(errorsOf(variant((f) => { delete f['engine/decisions/engine-language.json'].answer; })), ['plan/engine/decisions/engine-language.json answer: a taken decision without an answer or adds']);
  assert.deepEqual(errorsOf(variant((f) => { Object.assign(f['engine/decisions/linux-vm-engine.json'], { needs: ['model-tool'], verdict: 'take' }); })), [
    'plan/engine/decisions/linux-vm-engine.json needs[0]: "model-tool" is not taken, and this taken decision needs it',
  ]);
  // Not taken: it may wait for what it needs. A feature it needs is no decision to take first.
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/linux-vm-engine.json'].needs = ['model-tool']; })), []);
  assert.deepEqual(errorsOf(variant((f) => { Object.assign(f['engine/decisions/linux-vm-engine.json'], { needs: ['engine-language'], verdict: 'take' }); })), []);
});

test('rule 6: every statement is added by exactly one taken entry', () => {
  const design = (f) => f['decisions/the-design.json'];
  assert.deepEqual(errorsOf(variant((f) => { design(f).adds = design(f).adds.filter((id) => id !== 'fork'); })), ['plan/words.json [1].word: "fork" is added by no taken entry']);
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/engine-language.json'].adds = ['fork']; })), ['plan/words.json [1].word: "fork" is added by the-design, engine-language']);
  assert.deepEqual(errorsOf(variant((f) => { design(f).adds.push('vm'); })), ['plan/decisions/the-design.json adds[14]: "vm" is not a statement']);
  // A principle in its list that no taken entry brought.
  assert.deepEqual(errorsOf(variant((f) => { f['principles.json'].push({ id: 'least-privilege', says: 'S.', asks: 'A?' }); })), ['plan/principles.json [1].id: "least-privilege" is added by no taken entry']);
});

test('rule 7: from, keeps, needs and removes name what exists; a property with no from is a note', () => {
  assert.deepEqual(errorsOf(variant((f) => { f['record/properties.json'][0].from = 'nobody'; })), ['plan/record/properties.json [0].from: "nobody" is not a principle']);
  assert.deepEqual(errorsOf(variant((f) => { f['engine/features/start-computer.json'].keeps = ['nothing-at-all']; })), ['plan/engine/features/start-computer.json keeps[0]: "nothing-at-all" is not a property']);
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/model-tool.json'].needs = ['nobody']; })), ['plan/engine/decisions/model-tool.json needs[0]: "nobody" is not a decision or a feature']);
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/model-tool.json'].removes = ['nothing']; })), ['plan/engine/decisions/model-tool.json removes[0]: "nothing" is not a statement']);
  assert.deepEqual(errorsOf(variant((f) => { f['record/decisions/losses-show.json'].sets[0].from = 'nobody'; })), ['plan/record/decisions/losses-show.json sets[0].from: "nobody" is not a principle']);
  // A property may follow from a principle the same entry sets; not from one another entry sets.
  const ownPrinciple = { in: 'record/properties', id: 'reason-recorded', says: 'Every grant records its reason.', kind: 'safety', from: 'least-privilege', check: ['test'] };
  assert.deepEqual(errorsOf(variant((f) => { f['decisions/nothing-without-a-reason.json'].sets.push(ownPrinciple); })), []);
  assert.deepEqual(errorsOf(variant((f) => { f['record/decisions/losses-show.json'].sets[0].from = 'least-privilege'; })), ['plan/record/decisions/losses-show.json sets[0].from: "least-privilege" is not a principle']);
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
  // Stage ids are not in the namespace of entries and statements, and none is reserved.
  assert.deepEqual(errorsOf(variant((f) => { stage(f, 'later').id = 'gone'; })), []);
  assert.deepEqual(errorsOf(variant((f) => { stage(f, 'later').id = 'fork'; })), []);
  // The names reserved for folders are not reserved for stages.
  assert.deepEqual(errorsOf(variant((f) => { stage(f, 'later').id = 'features'; stage(f, 'app').id = 'decisions'; })), []);
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
  assert.deepEqual(errorsOf(withAt, { exists: () => false }), ['plan/record/properties.json [3].at[0]: "engine/test/classifier.zig" is not a regular file of the repository']);
  // Without exists, the file test is left out and a note says so.
  const { errors, notes } = checkFiles(filesOf(withAt));
  assert.deepEqual(errors, []);
  assert.deepEqual(notes.map((n) => n.message), ['the files named by at were not looked for']);
  // The note is there even when no property has an at.
  assert.deepEqual(checkFiles(filesOf(basePlan())).notes.map((n) => n.message), ['the files named by at were not looked for']);
});

test('rule 9: every path of an at is relative and stays inside the repository, whatever exists says', () => {
  const withPath = (path) => variant((f) => { f['record/properties.json'][0].at = [path]; });
  for (const path of ['/etc/passwd', '../outside.txt', 'engine/../../outside.txt', './engine/x.zig', 'engine//x.zig', 'engine/x.zig/', 'C:/x.zig', 'engine\\x.zig']) {
    for (const options of [{ exists: () => true }, {}]) {
      const errors = errorsOf(withPath(path), options);
      assert.equal(errors.length, 1, `${path}: ${errors}`);
      assert.match(errors[0], /^plan\/record\/properties\.json \[0\]\.at\[0\]: /);
    }
  }
  // In a sets item too.
  assert.equal(errorsOf(variant((f) => { f['record/decisions/losses-show.json'].sets[0].at = ['/etc/passwd']; }), { exists: () => true }).length, 1);
  assert.deepEqual(errorsOf(withPath('engine/test/x.zig'), { exists: () => true }), []);
});

test('a malformed field is a problem, never a throw: the check always returns { errors, notes }', () => {
  // An object with a toString key throws when it is turned into text the usual way.
  const hostile = [{ toString: 1 }, { valueOf: 1, toString: 'x' }, [{ toString: 1 }], {}, [], null, 7, true, 'x'];
  const edits = [
    (f, v) => { f['engine/decisions/model-tool.json'].verdict = v; },
    (f, v) => { f['engine/decisions/model-tool.json'].suggest = { verdict: v, why: 'W.' }; },
    (f, v) => { f['engine/decisions/model-tool.json'].suggest = v; },
    (f, v) => { f['engine/features/start-computer.json'].built = v; },
    (f, v) => { f['record/decisions/losses-show.json'].sets[0].in = v; },
    (f, v) => { f['record/decisions/losses-show.json'].sets[0].kind = v; },
    (f, v) => { f['record/decisions/losses-show.json'].sets[0].id = v; },
    (f, v) => { f['record/decisions/losses-show.json'].sets[0].from = v; },
    (f, v) => { f['record/decisions/losses-show.json'].sets[0].at = v; },
    (f, v) => { f['record/decisions/losses-show.json'].sets = [v]; },
    (f, v) => { f['record/properties.json'][0].kind = v; },
    (f, v) => { f['record/properties.json'][0].check = v; },
    (f, v) => { f['record/properties.json'][0].at = [v]; },
    (f, v) => { f['words.json'][0].word = v; },
    (f, v) => { f['words.json'][2].not = [v]; },
    (f, v) => { f['engine/features/start-computer.json'].needs = [v]; },
    (f, v) => { f['decisions/the-design.json'].adds = [v]; },
    (f, v) => { f['order.json'].stages[0].id = v; },
    (f, v) => { f['order.json'].stages[0].entries = [v]; },
    (f, v) => { f['parts.json'][0].id = v; },
  ];
  for (const [index, edit] of edits.entries()) {
    for (const value of hostile) {
      const files = filesOf(variant((f) => { edit(f, structuredClone(value)); }));
      let result;
      assert.doesNotThrow(() => { result = checkFiles(files, { exists: () => true }); }, `edit ${index} with ${JSON.stringify(value)}`);
      assert.ok(Array.isArray(result.errors) && Array.isArray(result.notes));
      assert.doesNotThrow(() => checkChange(files, filesOf(basePlan())), `edit ${index} with ${JSON.stringify(value)}`);
      assert.doesNotThrow(() => checkChange(filesOf(basePlan()), files), `edit ${index} with ${JSON.stringify(value)}`);
    }
  }
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/model-tool.json'].verdict = { toString: 1 }; })), ['plan/engine/decisions/model-tool.json verdict: "{"toString":1}" is not a verdict']);
});

test('rule 2: a text holding U+FFFD is valid UTF-8 and passes', () => {
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/model-tool.json'].asks = 'Which tool checks the model \uFFFD of access?'; })), []);
});

test('a word under not is only listed: using it in the plan is no error', () => {
  // The sample's words list "wall", "door" and "guess" under not.
  assert.deepEqual(errorsOf(variant((f) => { f['engine/README.md'] += '\nThe Wall stops it.\n'; })), []);
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/model-tool.json'].asks = 'Which tool checks every DOOR?'; })), []);
  assert.deepEqual(errorsOf(variant((f) => { f['record/decisions/losses-show.json'].sets[0].says = 'A guess is never a fact.'; })), []);
  // The list itself is still checked for its shape.
  assert.notDeepEqual(errorsOf(variant((f) => { f['words.json'][2].not = 'wall'; })), []);
});

test('rule 11: an entry whose text changed while its verdict stayed', () => {
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
  // Keys outside the text may change.
  assert.deepEqual(changeOf((f) => { f['engine/features/start-computer.json'].surfaces = { app: 'Start' }; }), []);
  // The owner ruled again after the change: the rule does not apply to that entry, and only to it.
  const ruled = (id) => id === 'engine-language';
  assert.deepEqual(checkChange(filesOf(variant((f) => { f['engine/decisions/engine-language.json'].answer = 'Rust.'; })), base, { ruledByOwner: ruled }).errors, []);
  assert.deepEqual(checkChange(filesOf(variant((f) => { f['engine/features/start-computer.json'].what = 'Boot.'; })), base, { ruledByOwner: ruled }).errors.map((e) => e.field), ['what']);
  // The offline check does not look at the base.
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/engine-language.json'].answer = 'Rust.'; })), []);
});

test('a problem reads file: field: message. Fix: …', () => {
  const [error] = checkFiles(filesOf(variant((f) => { f['engine/decisions/model-tool.json'].verdict = 'later'; }))).errors;
  assert.equal(formatProblem(error), 'plan/engine/decisions/model-tool.json: verdict: "later" is not a verdict. Fix: write take or drop, or leave it out');
});

test('a built feature with a malformed needs gives problems, never a throw', () => {
  const files = variant((f) => {
    f['engine/decisions/linux-vm-engine.json'].verdict = 'take';
    f['engine/features/start-computer.json'].built = '2026-11-20';
    f['engine/features/start-computer.json'].needs = [{ toString: null }, 'engine-language'];
  });
  const errors = errorsOf(files);
  assert.ok(errors.includes('plan/engine/features/start-computer.json needs[0]: not a text'), errors.join('\n'));
  assert.ok(errors.includes('plan/engine/features/start-computer.json built: built while {"toString":null} is not done'), errors.join('\n'));
});

test('a taken decision whose adds stands for its answer passes the check', () => {
  assert.deepEqual(errorsOf(variant((f) => { delete f['decisions/the-design.json'].answer; })), []);
});

/** Writes `"replaced_by": by` in each taken decision of `ids`, as plan take does. @param {any} f @param {string} by @param {unknown} ids */
function markReplaced(f, by, ids) {
  if (!Array.isArray(ids)) return;
  for (const id of ids) {
    const name = Object.keys(f).find((file) => file === `decisions/${id}.json` || file.endsWith(`/decisions/${id}.json`));
    if (name && id !== by && f[name].verdict === 'take') f[name] = { ...f[name], replaced_by: by };
  }
}

/**
 * The sample with a decision `id` (zig-again unless given), placed last, that names what `replaces`
 * gives (by default engine-language, which is taken and adds nothing), with the given verdict. Taken,
 * it is named back in the `replaced_by` of each decision it replaces.
 * @param {any} f the files @param {Record<string, unknown>} [keys] its keys @param {string} [id]
 */
function replacer(f, keys = {}, id = 'zig-again') {
  const decision = { asks: 'Which language now?', answer: 'Zig, still.', replaces: ['engine-language'], verdict: 'take', ...keys };
  f[`engine/decisions/${id}.json`] = decision;
  stage(f, 'later').entries.push(id);
  if (decision.verdict === 'take') markReplaced(f, id, decision.replaces);
  return f;
}

const notesOf = (files) => checkFiles(filesOf(files), { exists: () => true }).notes.map((n) => `${n.file} ${n.field}: ${n.message}`);

test('replaces on a taken decision is allowed, and the decision it names may then have neither an answer nor an adds', () => {
  assert.deepEqual(errorsOf(variant((f) => { replacer(f); })), []);
  assert.deepEqual(errorsOf(variant((f) => { replacer(f); delete f['engine/decisions/engine-language.json'].answer; })), []);
  // Not replaced, it needs one or the other.
  assert.deepEqual(errorsOf(variant((f) => { replacer(f, { verdict: undefined }); delete f['engine/decisions/engine-language.json'].answer; })), [
    'plan/engine/decisions/engine-language.json answer: a taken decision without an answer or adds',
  ]);
});

test('replaces is a list of decisions other than its own entry, each once', () => {
  const zig = 'plan/engine/decisions/zig-again.json';
  assert.deepEqual(errorsOf(variant((f) => { replacer(f, { replaces: 'engine-language' }); })), [`${zig} replaces: not a list`]);
  assert.deepEqual(errorsOf(variant((f) => { replacer(f, { replaces: [] }); })), [`${zig} replaces: empty`]);
  assert.deepEqual(errorsOf(variant((f) => { replacer(f, { replaces: ['engine-language', 'engine-language'] }); })), [`${zig} replaces[1]: "engine-language" appears twice`]);
  assert.deepEqual(errorsOf(variant((f) => { replacer(f, { replaces: [3] }); })), [`${zig} replaces[0]: not a text`]);
  assert.deepEqual(errorsOf(variant((f) => { replacer(f, { replaces: ['zig-again'] }); })), [`${zig} replaces[0]: "zig-again" replaces itself`]);
  assert.deepEqual(errorsOf(variant((f) => { replacer(f, { replaces: ['start-computer'] }); })), [`${zig} replaces[0]: "start-computer" is not a decision`]);
  assert.deepEqual(errorsOf(variant((f) => { replacer(f, { replaces: ['fork'] }); })), [`${zig} replaces[0]: "fork" is not a decision`]);
  // A feature has no such key.
  assert.deepEqual(errorsOf(variant((f) => { f['engine/features/start-computer.json'].replaces = ['engine-language']; })), ['plan/engine/features/start-computer.json replaces: unknown key']);
});

test('replaces names a taken decision, on a decision that is taken or has no verdict; a dropped one is skipped', () => {
  const zig = 'plan/engine/decisions/zig-again.json';
  assert.deepEqual(errorsOf(variant((f) => { replacer(f, { verdict: 'drop', replaces: ['linux-vm-engine'] }); })), []);
  for (const verdict of [undefined, 'take']) {
    assert.deepEqual(errorsOf(variant((f) => { replacer(f, { verdict, replaces: ['linux-vm-engine'] }); })), [`${zig} replaces[0]: "linux-vm-engine" is not taken`], String(verdict));
  }
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/model-tool.json'].verdict = 'drop'; replacer(f, { verdict: undefined, replaces: ['model-tool'] }); })), [`${zig} replaces[0]: "model-tool" is not taken`]);
  const [error] = checkFiles(filesOf(variant((f) => { replacer(f, { replaces: ['linux-vm-engine'] }); }))).errors;
  assert.match(error.fix, /edit "linux-vm-engine" instead/);
});

test('an entry sets a statement or removes it, not both', () => {
  const files = variant((f) => {
    const decision = f['decisions/nothing-without-a-reason.json'];
    decision.sets = [...decision.sets, { in: 'words', word: 'computer', means: 'A whole virtual machine.' }];
    decision.removes = ['fork', 'computer'];
  });
  assert.deepEqual(errorsOf(files), ['plan/decisions/nothing-without-a-reason.json removes[1]: "computer" is also set by this entry']);
});

test('a decision is replaced by one taken decision only', () => {
  const errors = errorsOf(variant((f) => {
    replacer(f);
    f['engine/decisions/zig-forever.json'] = { asks: 'And now?', answer: 'Zig.', replaces: ['engine-language'], verdict: 'take' };
    stage(f, 'later').entries.push('zig-forever');
  }));
  assert.deepEqual(errors, ['plan/engine/decisions/zig-forever.json replaces[0]: "engine-language" is replaced by zig-again, not by this decision']);
  // A second one with no verdict is a note, not an error: it should name the decision that replaced it.
  const pending = variant((f) => {
    replacer(f);
    f['engine/decisions/zig-forever.json'] = { asks: 'And now?', answer: 'Zig.', replaces: ['engine-language'] };
    stage(f, 'later').entries.push('zig-forever');
  });
  assert.deepEqual(errorsOf(pending), []);
  assert.ok(notesOf(pending).includes('plan/engine/decisions/zig-forever.json replaces[0]: "engine-language" is already replaced by zig-again'), notesOf(pending).join('\n'));
  // Dropped, it says nothing.
  const dropped = variant((f) => { replacer(f); f['engine/decisions/zig-forever.json'] = { asks: 'And now?', answer: 'Zig.', replaces: ['engine-language'], verdict: 'drop' }; stage(f, 'later').entries.push('zig-forever'); });
  assert.deepEqual(errorsOf(dropped), []);
  assert.ok(!notesOf(dropped).some((n) => n.includes('zig-forever')));
});

test('a replaced decision has no adds', () => {
  const errors = errorsOf(variant((f) => { replacer(f, { replaces: ['the-design'] }); }));
  assert.deepEqual(errors, [`plan/decisions/the-design.json adds: "the-design" is replaced by zig-again and still adds ${basePlan()['decisions/the-design.json'].adds.join(', ')}`]);
  const [error] = checkFiles(filesOf(variant((f) => { replacer(f, { replaces: ['the-design'] }); }))).errors;
  assert.match(error.fix, /zig-again sets again or removes each of those statements/);
});

test('a chain of replaces among taken decisions never comes back to its start', () => {
  const errors = errorsOf(variant((f) => {
    replacer(f);
    f['engine/decisions/engine-language.json'].replaces = ['zig-again'];
    markReplaced(f, 'engine-language', ['zig-again']);
  }));
  assert.deepEqual(errors, [
    'plan/engine/decisions/engine-language.json replaces: the replaces come back to "engine-language": engine-language replaces zig-again replaces engine-language',
    'plan/engine/decisions/zig-again.json replaces: the replaces come back to "zig-again": zig-again replaces engine-language replaces zig-again',
  ]);
  // Three long.
  const three = errorsOf(variant((f) => {
    replacer(f);
    replacer(f, { replaces: ['zig-again'] }, 'zig-forever');
    f['engine/decisions/engine-language.json'].replaces = ['zig-forever'];
    markReplaced(f, 'engine-language', ['zig-forever']);
  }));
  assert.ok(three.includes('plan/engine/decisions/zig-forever.json replaces: the replaces come back to "zig-forever": zig-forever replaces zig-again replaces engine-language replaces zig-forever'), three.join('\n'));
  // A chain that does not come back is fine.
  assert.deepEqual(errorsOf(variant((f) => {
    replacer(f);
    replacer(f, { replaces: ['zig-again'] }, 'zig-forever');
  })), []);
});

test('an entry still to do that needs a replaced decision gets a note; a done or dropped one does not', () => {
  // start-computer is taken, not built, and needs engine-language.
  const note = 'plan/engine/features/start-computer.json needs[0]: "engine-language" is replaced by zig-again';
  assert.ok(notesOf(variant((f) => { replacer(f); })).includes(note));
  assert.ok(!notesOf(variant((f) => { replacer(f, { verdict: undefined }); })).includes(note));
  const built = variant((f) => {
    replacer(f);
    f['engine/decisions/linux-vm-engine.json'].verdict = 'take';
    f['engine/features/start-computer.json'].built = '2026-11-20';
  });
  assert.deepEqual(errorsOf(built), []);
  assert.ok(!notesOf(built).includes(note));
  const dropped = variant((f) => { replacer(f); f['engine/features/start-computer.json'].verdict = 'drop'; });
  assert.ok(!notesOf(dropped).includes(note));
  const [problem] = checkFiles(filesOf(variant((f) => { replacer(f); })), { exists: () => true }).notes.filter((n) => n.field === 'needs[0]');
  assert.match(problem.fix, /need "zig-again" instead/);
});

test('rule 11 covers replaces, and not replaced_by, which the take of another entry writes', () => {
  const base = filesOf(variant((f) => { replacer(f); }));
  // The change gives the-design a replaced_by too: only the replaces of zig-again counts.
  const changed = filesOf(variant((f) => { replacer(f, { replaces: ['engine-language', 'the-design'] }); }));
  assert.notEqual(changed.get('decisions/the-design.json'), base.get('decisions/the-design.json'));
  assert.deepEqual(checkChange(changed, base).errors.map((e) => `${e.file} ${e.field}`), ['plan/engine/decisions/zig-again.json replaces']);
  const name = 'engine/decisions/zig-again.json';
  assert.equal(ruledKeysDiffer(base.get(name) ?? null, changed.get(name) ?? null), true);
  const old = 'engine/decisions/engine-language.json';
  assert.equal(ruledKeysDiffer(filesOf(basePlan()).get(old) ?? null, base.get(old) ?? null), false);
  // zig-again is new here, with the owner's take: only its verdict could be refused.
  assert.deepEqual(checkChange(base, filesOf(basePlan()), { ruledByOwner: () => true }).errors, []);
});

test('replaced_by names the taken decision that names this one in replaces, and only a taken decision has it', () => {
  const old = 'plan/engine/decisions/engine-language.json';
  const zig = 'plan/engine/decisions/zig-again.json';
  // The taken decision's replaces needs the name back.
  assert.deepEqual(errorsOf(variant((f) => { replacer(f); delete f['engine/decisions/engine-language.json'].replaced_by; })), [
    `${zig} replaces[0]: "engine-language" has no replaced_by that names this decision`,
  ]);
  // A replaced_by that no taken decision's replaces backs.
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/engine-language.json'].replaced_by = 'the-design'; })), [
    `${old} replaced_by: "the-design" is not a taken decision that names "engine-language" in replaces`,
  ]);
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/engine-language.json'].replaced_by = 'nobody'; })), [
    `${old} replaced_by: "nobody" is not a taken decision that names "engine-language" in replaces`,
  ]);
  assert.deepEqual(errorsOf(variant((f) => { replacer(f, { verdict: undefined }); markReplaced(f, 'zig-again', ['engine-language']); })), [
    `${old} replaced_by: "zig-again" is not a taken decision that names "engine-language" in replaces`,
  ]);
  // Only on a taken decision.
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/model-tool.json'].replaced_by = 'engine-language'; })), [
    'plan/engine/decisions/model-tool.json replaced_by: replaced_by on a decision that is not taken',
    'plan/engine/decisions/model-tool.json replaced_by: "engine-language" is not a taken decision that names "model-tool" in replaces',
  ]);
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/engine-language.json'].replaced_by = 3; })), [`${old} replaced_by: not a text`]);
  assert.deepEqual(errorsOf(variant((f) => { f['engine/decisions/engine-language.json'].replaced_by = ''; })), [`${old} replaced_by: empty`]);
  // A feature has no such key.
  assert.deepEqual(errorsOf(variant((f) => { f['engine/features/start-computer.json'].replaced_by = 'engine-language'; })), ['plan/engine/features/start-computer.json replaced_by: unknown key']);
  // Last in the key order of a decision.
  const { errors } = checkFiles(new Map([...filesOf(variant((f) => { replacer(f); }))].map(([name, text]) => [name, name === 'engine/decisions/engine-language.json' ? `${JSON.stringify({ replaced_by: 'zig-again', ...JSON.parse(text) }, null, 2)}\n` : text])));
  assert.deepEqual(errors.map((e) => `${e.file} ${e.field}: ${e.message}`), [`${old} -: not in canonical form`]);
});

test('rule 11: a verdict the owner\'s command did not write is an error, one error on the field verdict', () => {
  const base = filesOf(basePlan());
  const name = 'engine/decisions/linux-vm-engine.json'; // no verdict in the base
  const taken = filesOf(variant((f) => { f[name].verdict = 'take'; }));
  assert.deepEqual(checkChange(taken, base).errors, [{
    file: `plan/${name}`,
    field: 'verdict',
    message: '"linux-vm-engine" has the verdict take, which the owner\'s command did not write',
    fix: "remove the verdict; the owner's yes is the command plan take linux-vm-engine",
  }]);
  const dropped = filesOf(variant((f) => { f[name].verdict = 'drop'; }));
  assert.match(checkChange(dropped, base).errors[0].fix, /the owner's no is the command plan drop linux-vm-engine/);
  // Given the evidence, the command's kind is asked for: a take is not covered by a drop.
  const asked = [];
  const ruled = (kind) => (id, file, verdict) => { asked.push([id, file, verdict]); return verdict === kind; };
  assert.deepEqual(checkChange(taken, base, { ruledByOwner: ruled('take') }).errors, []);
  assert.deepEqual(asked, [['linux-vm-engine', `plan/${name}`, 'take']]);
  assert.equal(checkChange(taken, base, { ruledByOwner: ruled('drop') }).errors.length, 1);
  // A new entry born with a verdict; the base holds other entries.
  const born = filesOf(variant((f) => { f['engine/decisions/born.json'] = { asks: 'Which?', answer: 'This.', verdict: 'take' }; stage(f, 'later').entries.push('born'); }));
  assert.deepEqual(checkChange(born, base).errors.map((e) => `${e.file} ${e.field}`), ['plan/engine/decisions/born.json verdict']);
  // A verdict changed by hand from take to drop, or drop to take.
  const flipped = filesOf(variant((f) => { f['engine/decisions/engine-language.json'].verdict = 'drop'; }));
  assert.deepEqual(checkChange(flipped, base).errors.map((e) => e.field), ['verdict']);
  const dropBase = filesOf(variant((f) => { f['engine/decisions/engine-language.json'].verdict = 'drop'; }));
  assert.deepEqual(checkChange(base, dropBase).errors.map((e) => e.field), ['verdict']);
  // A verdict taken away by hand, and an entry that kept its verdict, are fine.
  assert.deepEqual(checkChange(filesOf(variant((f) => { delete f['engine/decisions/engine-language.json'].verdict; })), base).errors, []);
  assert.deepEqual(checkChange(base, base).errors, []);
});

test('rule 11: a merged plan that holds no entry is no base for the verdicts', () => {
  const noEntries = new Map([['README.md', 'The old plan.\n']]);
  assert.deepEqual(checkChange(filesOf(basePlan()), noEntries).errors, []);
  assert.deepEqual(checkChange(filesOf(basePlan()), new Map()).errors, []);
});

test('rule 11, the verdict of every combination of the base, the file and the evidence', () => {
  // The reference: an error exactly when the file holds a verdict that differs from the base's and
  // the evidence is not the owner's command for that verdict (none given counts as none).
  const name = 'engine/decisions/engine-language.json';
  const entry = (verdict) => filesOf(variant((f) => { if (verdict === 'none') delete f[name].verdict; else f[name].verdict = verdict; }));
  const gone = filesOf(variant((f) => { delete f[name]; }));
  for (const before of ['absent', 'none', 'take', 'drop']) {
    for (const after of ['none', 'take', 'drop']) {
      for (const evidence of [undefined, 'no', 'take', 'drop']) {
        const base = before === 'absent' ? gone : entry(before);
        const ruledByOwner = evidence === undefined ? undefined : (id, file, verdict) => evidence === verdict;
        const errors = checkChange(entry(after), base, { ruledByOwner }).errors.filter((e) => e.field === 'verdict');
        const expected = after !== 'none' && before !== after && evidence !== after;
        assert.equal(errors.length, expected ? 1 : 0, `${before} -> ${after}, evidence ${evidence}`);
      }
    }
  }
});
