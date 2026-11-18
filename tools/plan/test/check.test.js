import { test } from 'node:test';
import assert from 'node:assert/strict';
import { checkFiles, formatProblem } from '../check/index.js';
import { basePlan, filesOf } from './helpers.js';

/** The plan with one change: `edit` gets a deep copy and returns nothing or the files. */
function variant(edit) {
  const files = structuredClone(basePlan());
  return edit(files) ?? files;
}

const errorsOf = (files, options) => checkFiles(filesOf(files), options).errors;

test('the base fixture passes', () => {
  assert.deepEqual(errorsOf(basePlan()), []);
});

test('DESIGN.md is a file of the plan', () => {
  assert.deepEqual(errorsOf(variant((f) => { f['DESIGN.md'] = '# Design\n'; })), []);
});

// One failing fixture per rule: [rule, edit, file, field, words in the message].
const CASES = [
  ['a missing file', (f) => { delete f['order.json']; }, 'plan/order.json', '-', 'missing'],
  ['PRODUCT.md missing', (f) => { delete f['PRODUCT.md']; }, 'plan/PRODUCT.md', '-', 'missing'],
  ['JSON that does not parse', (f) => { f['features/core.json'] = '{ "name": '; }, 'plan/features/core.json', '-', 'not JSON'],
  ['a file that is not the plan', (f) => { f['notes.txt'] = 'x'; }, 'plan/notes.txt', '-', 'not a file of the plan'],
  ['a feature file name that is not an id', (f) => { f['features/Bad_Name.json'] = { name: 'N', what: 'W', area: 'engine', needs: [] }; f['order.json'].stages[0].features.push('Bad_Name'); }, 'plan/features/Bad_Name.json', '-', 'not a feature id'],
  ['notes without a feature file', (f) => { f['features/ghost.md'] = 'x'; }, 'plan/features/ghost.md', '-', 'notes without a feature file'],
  ['areas: not an array', (f) => { f['areas.json'] = {}; }, 'plan/areas.json', '-', 'not an array'],
  ['areas: an unknown key', (f) => { f['areas.json'][0].color = 'red'; }, 'plan/areas.json', '[0].color', 'unknown key'],
  ['areas: an empty title', (f) => { f['areas.json'][0].title = ''; }, 'plan/areas.json', '[0].title', 'missing or empty'],
  ['areas: an id that is not an id', (f) => { f['areas.json'][1].id = 'Forks!'; }, 'plan/areas.json', '[1].id', 'is not an id'],
  ['areas: an id twice', (f) => { f['areas.json'][1].id = 'engine'; }, 'plan/areas.json', '[1].id', 'appears twice'],
  ['order: not an object', (f) => { f['order.json'] = []; }, 'plan/order.json', '-', 'not an object'],
  ['order: an unknown key', (f) => { f['order.json'].extra = 1; }, 'plan/order.json', 'extra', 'unknown key'],
  ['order: stages not an array', (f) => { f['order.json'].stages = {}; }, 'plan/order.json', 'stages', 'not an array'],
  ['order: a stage without a line', (f) => { delete f['order.json'].stages[0].line; }, 'plan/order.json', 'stages[0].line', 'missing or empty'],
  ['order: a stage without features', (f) => { delete f['order.json'].stages[1].features; }, 'plan/order.json', 'stages[1].features', 'not an array'],
  ['order: a stage id twice', (f) => { f['order.json'].stages[1].id = 'shell'; }, 'plan/order.json', 'stages[1].id', 'appears twice'],
  ['order: gone is not last', (f) => { f['order.json'].stages.reverse(); }, 'plan/order.json', 'stages[0].id', 'is not the last'],
  ['order: a feature placed twice', (f) => { f['order.json'].stages[1].features.push('core'); }, 'plan/order.json', 'core', 'placed 2 times'],
  ['order: an id without a file', (f) => { f['order.json'].stages[1].features.push('nobody'); }, 'plan/order.json', 'nobody', 'no feature file'],
  ['order: a feature not placed', (f) => { f['order.json'].stages[1].features = []; }, 'plan/features/fork.json', '-', 'not in order.json'],
  ['feature: not an object', (f) => { f['features/core.json'] = []; }, 'plan/features/core.json', '-', 'not an object'],
  ['feature: an unknown key', (f) => { f['features/core.json'].stage = 'shell'; }, 'plan/features/core.json', 'stage', 'unknown key'],
  ['feature: a null instead of an absent key', (f) => { f['features/core.json'].verdict = null; }, 'plan/features/core.json', 'verdict', 'null'],
  ['feature: no name', (f) => { delete f['features/core.json'].name; }, 'plan/features/core.json', 'name', 'missing or empty'],
  ['feature: an empty what', (f) => { f['features/core.json'].what = ' '; }, 'plan/features/core.json', 'what', 'missing or empty'],
  ['feature: a name with a comment opener', (f) => { f['features/core.json'].name = 'Core <!-- x'; }, 'plan/features/core.json', 'name', 'holds <!-- or -->'],
  ['feature: a what with a comment closer', (f) => { f['features/core.json'].what = 'The engine. -->'; }, 'plan/features/core.json', 'what', 'holds <!-- or -->'],
  ['feature: no area outside gone', (f) => { delete f['features/core.json'].area; }, 'plan/features/core.json', 'area', 'missing'],
  ['feature: an unknown area', (f) => { f['features/core.json'].area = 'nowhere'; }, 'plan/features/core.json', 'area', 'is not an area'],
  ['feature: needs missing', (f) => { delete f['features/core.json'].needs; }, 'plan/features/core.json', 'needs', 'missing or not an array'],
  ['feature: a need that is not a feature', (f) => { f['features/cli.json'].needs = ['nobody']; }, 'plan/features/cli.json', 'needs[0]', 'is not a feature'],
  ['feature: a need that is not a text', (f) => { f['features/cli.json'].needs = [3]; }, 'plan/features/cli.json', 'needs[0]', 'not a feature id'],
  ['feature: needs itself', (f) => { f['features/cli.json'].needs = ['cli']; }, 'plan/features/cli.json', 'needs[0]', 'needs itself'],
  ['feature: a need twice', (f) => { f['features/cli.json'].needs = ['core', 'core']; }, 'plan/features/cli.json', 'needs[1]', 'appears twice'],
  ['feature: a cycle in needs', (f) => { f['features/core.json'].needs = ['fork']; }, 'plan/features/fork.json', 'needs', 'a cycle: core → fork → core'],
  ['feature: a longer cycle in needs', (f) => { f['features/core.json'].needs = ['fork']; f['features/fork.json'].needs = ['cli']; }, 'plan/features/fork.json', 'needs', 'a cycle: cli → core → fork → cli'],
  ['feature: surfaces empty', (f) => { f['features/cli.json'].surfaces = {}; }, 'plan/features/cli.json', 'surfaces', 'empty'],
  ['feature: surfaces with an unknown key', (f) => { f['features/cli.json'].surfaces.web = 'x'; }, 'plan/features/cli.json', 'surfaces.web', 'unknown key'],
  ['feature: a surface that is not a text', (f) => { f['features/cli.json'].surfaces.cli = 3; }, 'plan/features/cli.json', 'surfaces.cli', 'not a text'],
  ['feature: surfaces not an object', (f) => { f['features/cli.json'].surfaces = 'silo'; }, 'plan/features/cli.json', 'surfaces', 'not an object'],
  ['feature: suggest not an object', (f) => { f['features/fork.json'].suggest = 'take'; }, 'plan/features/fork.json', 'suggest', 'not an object'],
  ['feature: suggest.verdict refine', (f) => { f['features/fork.json'].suggest.verdict = 'refine'; }, 'plan/features/fork.json', 'suggest.verdict', 'is not a verdict'],
  ['feature: suggest with an unknown key', (f) => { f['features/fork.json'].suggest.when = 'now'; }, 'plan/features/fork.json', 'suggest.when', 'unknown key'],
  ['feature: suggest.why not a text', (f) => { f['features/fork.json'].suggest.why = 1; }, 'plan/features/fork.json', 'suggest.why', 'not a text'],
  ['feature: suggested_in not an issue number', (f) => { f['features/fork.json'].suggested_in = 0; }, 'plan/features/fork.json', 'suggested_in', 'not an issue number'],
  ['feature: verdict refine', (f) => { f['features/fork.json'].verdict = 'refine'; }, 'plan/features/fork.json', 'verdict', 'is not a verdict'],
  ['feature: built not a date', (f) => { Object.assign(f['features/core.json'], { verdict: 'take', built: '2026-13-01' }); }, 'plan/features/core.json', 'built', 'is not a date'],
  ['feature: built without take', (f) => { Object.assign(f['features/core.json'], { verdict: 'later', built: '2026-11-01' }); }, 'plan/features/core.json', 'built', 'without the verdict take'],
  ['decisions: not an array', (f) => { f['decisions.json'] = {}; }, 'plan/decisions.json', '-', 'not an array'],
  ['decisions: an unknown key', (f) => { f['decisions.json'][0].who = 'owner'; }, 'plan/decisions.json', '[0].who', 'unknown key'],
  ['decisions: an id that is not an id', (f) => { f['decisions.json'][0].id = 'Not An Id'; }, 'plan/decisions.json', '[0].id', 'is not an id'],
  ['decisions: an id twice', (f) => { f['decisions.json'][1].id = 'everywhere'; }, 'plan/decisions.json', '[1].id', 'appears twice'],
  ['decisions: a date that is not a date', (f) => { f['decisions.json'][0].date = 'yesterday'; }, 'plan/decisions.json', '[0].date', 'is not a date'],
  ['decisions: no text', (f) => { f['decisions.json'][0].text = ''; }, 'plan/decisions.json', '[0].text', 'missing or empty'],
  ['decisions: about names nothing', (f) => { f['decisions.json'][1].about = ['nowhere']; }, 'plan/decisions.json', '[1].about[0]', '"nowhere" is not an area'],
  ['decisions: about names a feature', (f) => { f['decisions.json'][1].about = ['forks', 'fork']; }, 'plan/decisions.json', '[1].about[1]', '"fork" is a feature, not an area'],
  ['decisions: about names an area twice', (f) => { f['decisions.json'][1].about = ['forks', 'engine', 'forks']; }, 'plan/decisions.json', '[1].about[2]', '"forks" appears twice'],
  ['decisions: about empty', (f) => { f['decisions.json'][1].about = []; }, 'plan/decisions.json', '[1].about', 'not a list of area ids'],
  ['canonical: keys out of order', (f) => { f['features/core.json'] = `${JSON.stringify({ what: 'The engine.', name: 'Core', area: 'engine', needs: [] }, null, 2)}\n`; }, 'plan/features/core.json', '-', 'not in canonical form'],
  ['canonical: no final newline', (f) => { f['areas.json'] = JSON.stringify(f['areas.json'], null, 2); }, 'plan/areas.json', '-', 'not in canonical form'],
  ['canonical: four spaces', (f) => { f['order.json'] = `${JSON.stringify(f['order.json'], null, 4)}\n`; }, 'plan/order.json', '-', 'not in canonical form'],
  ['canonical: decisions', (f) => { f['decisions.json'] = `${JSON.stringify(f['decisions.json'])}\n`; }, 'plan/decisions.json', '-', 'not in canonical form'],
];

for (const [rule, edit, file, field, words] of CASES) {
  test(`fails: ${rule}`, () => {
    const errors = errorsOf(variant(edit));
    const found = errors.find((error) => error.file === file && error.field === field && error.message.includes(words));
    assert.ok(found, `expected ${file}: ${field}: …${words}…, got:\n${errors.map(formatProblem).join('\n')}`);
    assert.ok(found.fix.length > 0);
  });
}

test('a feature of the stage gone may have no area; any other may not', () => {
  assert.deepEqual(errorsOf(variant((f) => { f['features/old.json'].area = 'engine'; })), []);
  const moved = errorsOf(variant((f) => { f['order.json'].stages[2].features = []; f['order.json'].stages[1].features.push('old'); }));
  assert.deepEqual(moved.map((e) => `${e.file} ${e.field}`), ['plan/features/old.json area']);
});

test('errors read file: field: message. Fix: …', () => {
  const [error] = errorsOf(variant((f) => { f['features/core.json'].area = 'nowhere'; }));
  assert.equal(formatProblem(error), 'plan/features/core.json: area: "nowhere" is not an area. Fix: use one of the ids in plan/areas.json: engine, forks');
});

test('a new area is mentioned with the map to draw', () => {
  const files = variant((f) => { f['areas.json'].push({ id: 'access', title: 'Access', line: 'Keys.' }); });
  const { errors, notes } = checkFiles(filesOf(files), { knownAreas: ['engine', 'forks'] });
  assert.deepEqual(errors, []);
  assert.equal(notes.length, 1);
  assert.match(notes[0], /"access" is new.*website\/src\/workshop\/map\.ts/);
  assert.deepEqual(checkFiles(filesOf(files)).notes, []);
});

test('a decision about a feature points to its notes', () => {
  const errors = errorsOf(variant((f) => { f['decisions.json'][1].about = ['fork']; }));
  assert.deepEqual(errors.map(formatProblem), ['plan/decisions.json: [1].about[0]: "fork" is a feature, not an area. Fix: a decision about one feature goes in plan/features/fork.md; about names areas only']);
});

test('files whose name starts with a dot are ignored, wherever they are in plan/', () => {
  const files = variant((f) => { f['.DS_Store'] = 'x'; f['features/.DS_Store'] = 'x'; f['features/.draft.json'] = '{'; f['.git/config'] = 'x'; });
  assert.deepEqual(errorsOf(files), []);
  assert.deepEqual(errorsOf(variant((f) => { f['DS_Store'] = 'x'; })).map((e) => e.file), ['plan/DS_Store']);
});
