import { test } from 'node:test';
import assert from 'node:assert/strict';
import { checkFiles } from '../check/index.js';
import { canonicalDecision, canonicalFeature, canonicalText } from '../model/canonical.js';
import { wordId } from '../model/names.js';
import { entryStatus, replacedBy } from '../model/status.js';
import { parsePlan } from '../read/index.js';
import { drop, take } from '../write/index.js';
import { basePlan, filesOf } from './helpers.js';

/** The sample plan as the files the functions take, with changes on top. @param {Record<string, unknown>} [changes] */
const sample = (changes = {}) => filesOf({ ...basePlan(), ...changes });

/** The files after an outcome. @param {Map<string, string>} files @param {{ changed: Map<string, string> }} outcome */
const apply = (files, outcome) => {
  const after = new Map(files);
  for (const [name, text] of outcome.changed) after.set(name, text);
  return after;
};

/** The agents' verdict, when no commit of the history gave the entry the owner's. */
const AG = { by: 'agents', ownerRuled: () => false };

const parse = (files, name) => JSON.parse(/** @type {string} */ (files.get(name)));

/** Appends an entry to the last stage of order.json. @param {Map<string, string>} files @param {string} id */
function place(files, id) {
  const order = parse(files, 'order.json');
  order.stages.at(-1).entries.push(id);
  files.set('order.json', canonicalText(order));
}

test('the sample passes the check, so the cases below start from a valid plan', () => {
  assert.deepEqual(checkFiles(sample()).errors, []);
});

test('take writes the statements, adds and the verdict, and the result passes the check', () => {
  const files = sample();
  const before = parse(files, 'record/decisions/losses-show.json');
  const outcome = take(files, 'losses-show');
  assert.equal(outcome.status, 'done');
  assert.deepEqual([...outcome.changed.keys()].sort(), ['record/decisions/losses-show.json', 'record/properties.json']);
  const after = apply(files, outcome);
  const { in: _list, ...property } = before.sets[0];
  assert.deepEqual(parse(after, 'record/properties.json').at(-1), property);
  const entry = parse(after, 'record/decisions/losses-show.json');
  assert.deepEqual(entry, { asks: before.asks, answer: before.answer, adds: ['every-loss-is-a-gap'], verdict: 'take' });
  assert.equal(after.get('record/decisions/losses-show.json'), canonicalText(canonicalDecision(entry)));
  assert.deepEqual(checkFiles(after).errors, []);
});

test('take accepts a decision that puts forward only statements, and refuses one with neither an answer nor a sets', () => {
  const { answer: _answer, ...onlySets } = basePlan()['record/decisions/losses-show.json'];
  const files = sample({ 'record/decisions/losses-show.json': onlySets });
  assert.deepEqual(checkFiles(files).errors, []);
  const outcome = take(files, 'losses-show');
  assert.equal(outcome.status, 'done');
  const after = apply(files, outcome);
  assert.deepEqual(parse(after, 'record/decisions/losses-show.json'), { asks: onlySets.asks, adds: ['every-loss-is-a-gap'], verdict: 'take' });
  assert.deepEqual(checkFiles(after).errors, []);

  const { sets: _sets, ...nothing } = onlySets;
  const refusal = take(sample({ 'record/decisions/losses-show.json': nothing }), 'losses-show');
  assert.equal(refusal.status, 'refused');
  assert.match(refusal.message, /no answer and sets nothing/);
});

test('take refuses and writes nothing: no answer, a decision needed and not taken, a plan that fails the check, an unknown id', () => {
  const files = sample();
  const noAnswer = take(files, 'model-tool');
  assert.equal(noAnswer.status, 'refused');
  assert.match(noAnswer.message, /no answer/);
  assert.equal(noAnswer.changed.size, 0);

  const needs = sample({ 'engine/decisions/linux-vm-engine.json': { ...basePlan()['engine/decisions/linux-vm-engine.json'], needs: ['model-tool'] } });
  assert.deepEqual(checkFiles(needs).errors, []);
  const waiting = take(needs, 'linux-vm-engine');
  assert.equal(waiting.status, 'refused');
  assert.match(waiting.message, /needs model-tool/);
  assert.equal(waiting.changed.size, 0);

  const broken = sample({ 'principles.json': 'not JSON' });
  const failing = take(broken, 'losses-show');
  assert.equal(failing.status, 'refused');
  assert.ok(failing.errors?.length);
  assert.equal(failing.changed.size, 0);

  assert.equal(take(files, 'no-such-entry').status, 'refused');
});

test('take refuses a result that fails the check, and writes nothing', () => {
  // Removing the principle the record's properties follow from leaves their `from` naming nothing.
  const files = sample();
  files.set('decisions/no-principle.json', canonicalText(canonicalDecision({ asks: 'Drop the principle?', answer: 'Yes.', removes: ['less-sure-less-power'] })));
  place(files, 'no-principle');
  assert.deepEqual(checkFiles(files).errors, []);
  const outcome = take(files, 'no-principle');
  assert.equal(outcome.status, 'refused');
  assert.match(outcome.message, /would fail the check/);
  assert.ok(outcome.errors?.length);
  assert.equal(outcome.changed.size, 0);
});

test('take on a taken entry says so and changes nothing; on a built feature it refuses', () => {
  const files = sample();
  const outcome = take(files, 'engine-language');
  assert.equal(outcome.status, 'already');
  assert.equal(outcome.changed.size, 0);

  const plan = basePlan();
  const built = sample({
    'engine/decisions/linux-vm-engine.json': { ...plan['engine/decisions/linux-vm-engine.json'], verdict: 'take' },
    'engine/features/start-computer.json': { ...plan['engine/features/start-computer.json'], built: '2026-11-20' },
  });
  assert.deepEqual(checkFiles(built).errors, []);
  for (const verdict of [take, drop]) {
    const refused = verdict(built, 'start-computer');
    assert.equal(refused.status, 'refused');
    assert.match(refused.message, /is built/);
    assert.equal(refused.changed.size, 0);
  }
});

test('replacing a statement moves its id out of the earlier entry\'s adds', () => {
  const files = sample();
  files.set('decisions/sure-enough.json', canonicalText(canonicalDecision({
    asks: 'How sure must a trigger be?',
    answer: 'As sure as what it may do.',
    sets: [{ in: 'principles', id: 'less-sure-less-power', says: 'The less sure the trigger, the less it may do, and never more.', asks: 'What triggers it?', why: 'A wrong grant cannot be taken back.' }],
  })));
  place(files, 'sure-enough');
  assert.deepEqual(checkFiles(files).errors, []);
  const outcome = take(files, 'sure-enough');
  assert.equal(outcome.status, 'done', outcome.message);
  const after = apply(files, outcome);
  const principles = parse(after, 'principles.json');
  assert.equal(principles.length, 1);
  assert.equal(principles[0].says, 'The less sure the trigger, the less it may do, and never more.');
  assert.ok(!parse(after, 'decisions/the-design.json').adds.includes('less-sure-less-power'));
  assert.deepEqual(parse(after, 'decisions/sure-enough.json').adds, ['less-sure-less-power']);
  assert.deepEqual(checkFiles(after).errors, []);
});

test('a sets item with the id of a statement of another list of its kind moves it there, and the result passes the check', () => {
  const files = sample();
  const entry = parse(files, 'record/decisions/losses-show.json');
  entry.sets[0] = { in: 'engine/properties', id: 'classifier-never-acts', says: 'A classifier never acts on the engine.', kind: 'safety', from: 'less-sure-less-power', check: ['bounded exhaustive test'] };
  files.set('record/decisions/losses-show.json', canonicalText(canonicalDecision(entry)));
  assert.deepEqual(checkFiles(files).errors, []);
  const outcome = take(files, 'losses-show');
  assert.equal(outcome.status, 'done', `${outcome.message} ${JSON.stringify(outcome.errors ?? [])}`);
  const after = apply(files, outcome);
  assert.ok(!parse(after, 'record/properties.json').some((p) => p.id === 'classifier-never-acts'));
  assert.deepEqual(parse(after, 'engine/properties.json').map((p) => p.id), ['classifier-never-acts']);
  assert.ok(!parse(after, 'decisions/the-design.json').adds.includes('classifier-never-acts'));
  assert.deepEqual(checkFiles(after).errors, []);
});

test('one take sets a principle and a property that follows from it', () => {
  const files = sample();
  const entry = parse(files, 'decisions/nothing-without-a-reason.json');
  entry.sets.push({ in: 'record/properties', id: 'reason-recorded', says: 'Every grant records its reason.', kind: 'safety', from: 'least-privilege', check: ['test'] });
  files.set('decisions/nothing-without-a-reason.json', canonicalText(canonicalDecision(entry)));
  assert.deepEqual(checkFiles(files).errors, []);
  const outcome = take(files, 'nothing-without-a-reason');
  assert.equal(outcome.status, 'done', `${outcome.message} ${JSON.stringify(outcome.errors ?? [])}`);
  const after = apply(files, outcome);
  assert.deepEqual(parse(after, 'decisions/nothing-without-a-reason.json').adds, ['least-privilege', 'reason-recorded']);
  assert.equal(parse(after, 'record/properties.json').at(-1).from, 'least-privilege');
  assert.deepEqual(checkFiles(after).errors, []);
});

test('removes deletes the statement and takes it out of the adds that brought it; an entry that sets nothing gets no adds', () => {
  const files = sample();
  files.set('decisions/no-fork.json', canonicalText(canonicalDecision({ asks: 'Is a fork a word of its own?', answer: 'No.', removes: ['fork'] })));
  place(files, 'no-fork');
  const outcome = take(files, 'no-fork');
  assert.equal(outcome.status, 'done', outcome.message);
  const after = apply(files, outcome);
  assert.ok(!parse(after, 'words.json').some((word) => word.word === 'fork'));
  assert.ok(!parse(after, 'decisions/the-design.json').adds.includes('fork'));
  assert.deepEqual(parse(after, 'decisions/no-fork.json'), { asks: 'Is a fork a word of its own?', answer: 'No.', verdict: 'take' });
});

/**
 * The sample with losses-show taken, with notes, and a later decision gap-reworded, not taken yet,
 * that sets the same statement again.
 * @param {{ answer?: boolean, keys?: Record<string, unknown> }} [options] answer: losses-show keeps its answer; keys: of gap-reworded
 */
function replacing({ answer = false, keys = {} } = {}) {
  const { answer: _answer, ...onlySets } = basePlan()['record/decisions/losses-show.json'];
  let files = sample({ 'record/decisions/losses-show.json': answer ? basePlan()['record/decisions/losses-show.json'] : onlySets, 'record/decisions/losses-show.md': 'Why the record shows its losses.\n' });
  files = apply(files, take(files, 'losses-show'));
  files.set('record/decisions/gap-reworded.json', canonicalText(canonicalDecision({
    asks: 'How is a loss said?',
    answer: 'As a gap in the record.',
    sets: [{ in: 'record/properties', id: 'every-loss-is-a-gap', says: 'Every loss of facts or reports leaves a gap in the record.', kind: 'safety', check: ['simulator'] }],
    ...keys,
  })));
  place(files, 'gap-reworded');
  assert.deepEqual(checkFiles(files).errors, []);
  return files;
}

/** Adds a decision to the files and places it last. @param {Map<string, string>} files @param {string} id @param {Record<string, unknown>} decision */
function propose(files, id, decision) {
  files.set(`decisions/${id}.json`, canonicalText(canonicalDecision(decision)));
  place(files, id);
}

test('a take that replaces a decision keeps that decision, its notes and its place: it is replaced, and no longer in force', () => {
  for (const answer of [false, true]) {
    const files = replacing({ answer, keys: { replaces: ['losses-show'] } });
    const outcome = take(files, 'gap-reworded');
    assert.equal(outcome.status, 'done', `${outcome.message} ${JSON.stringify(outcome.errors ?? [])}`);
    assert.deepEqual([...outcome.changed.keys()].sort(), ['record/decisions/gap-reworded.json', 'record/decisions/losses-show.json', 'record/properties.json']);
    const after = apply(files, outcome);
    assert.deepEqual([...after.keys()].sort(), [...files.keys()].sort());
    const old = parse(after, 'record/decisions/losses-show.json');
    assert.equal(old.verdict, 'take');
    assert.equal(old.adds, undefined);
    // The replaced decision's own file says so, in the same take.
    assert.equal(old.replaced_by, 'gap-reworded');
    assert.equal(after.get('record/decisions/losses-show.json'), canonicalText(canonicalDecision(old)));
    assert.equal(Object.keys(old).at(-1), 'replaced_by');
    assert.equal(old.answer !== undefined, answer);
    assert.equal(after.get('record/decisions/losses-show.md'), 'Why the record shows its losses.\n');
    assert.equal(after.get('order.json'), files.get('order.json'));
    const later = parse(after, 'record/decisions/gap-reworded.json');
    assert.deepEqual(later.replaces, ['losses-show']);
    assert.deepEqual(later.adds, ['every-loss-is-a-gap']);
    assert.equal(later.sets, undefined);
    const { plan } = parsePlan(after);
    assert.equal(entryStatus(plan.entries['losses-show'], plan.entries), 'replaced');
    assert.deepEqual(checkFiles(after).errors, []);
    // Taken again, it says it is taken and replaced.
    const again = take(after, 'losses-show');
    assert.equal(again.status, 'already');
    assert.equal(again.message, 'losses-show is already taken, and replaced by gap-reworded');
    assert.equal(again.changed.size, 0);
  }
});

test('a decision with an answer whose every statement a later take moves stays in force, with no adds', () => {
  const files = replacing({ answer: true });
  const outcome = take(files, 'gap-reworded');
  assert.equal(outcome.status, 'done', `${outcome.message} ${JSON.stringify(outcome.errors ?? [])}`);
  const after = apply(files, outcome);
  const kept = parse(after, 'record/decisions/losses-show.json');
  assert.equal(kept.verdict, 'take');
  assert.equal(kept.adds, undefined);
  const { plan } = parsePlan(after);
  assert.equal(entryStatus(plan.entries['losses-show'], plan.entries), 'taken');
  assert.deepEqual(checkFiles(after).errors, []);
});

test('a take that would leave a taken decision it does not replace with neither an answer nor an adds is refused, and nothing is deleted', () => {
  const files = replacing();
  const outcome = take(files, 'gap-reworded');
  assert.equal(outcome.status, 'refused');
  assert.equal(outcome.changed.size, 0);
  assert.equal(outcome.message, 'gap-reworded moves every statement of losses-show, which has no answer, so it would be left with nothing in force: name it in the replaces of gap-reworded');

  // A feature cannot replace a decision: one that does comes first.
  const feature = new Map(files);
  feature.delete('record/decisions/gap-reworded.json');
  const order = parse(feature, 'order.json');
  for (const stage of order.stages) stage.entries = stage.entries.filter((id) => id !== 'gap-reworded');
  order.stages.at(-1).entries.push('show-gaps');
  feature.set('order.json', canonicalText(order));
  feature.set('record/features/show-gaps.json', canonicalText(canonicalFeature({
    name: 'Show the gaps',
    what: 'The record shows each gap.',
    sets: [{ in: 'record/properties', id: 'every-loss-is-a-gap', says: 'Every gap is shown.', kind: 'safety', check: ['test'] }],
  })));
  assert.deepEqual(checkFiles(feature).errors, []);
  const refusal = take(feature, 'show-gaps');
  assert.equal(refusal.status, 'refused');
  assert.match(refusal.message, /losses-show.*a decision that replaces it comes first$/);
  assert.equal(refusal.changed.size, 0);
});

test('a take is refused when a decision it replaces would keep an adds; removing the statement instead is enough', () => {
  const files = replacing({ keys: { replaces: ['losses-show'], sets: [{ in: 'record/properties', id: 'gaps-are-dated', says: 'Every gap has a date.', kind: 'safety', check: ['test'] }] } });
  const outcome = take(files, 'gap-reworded');
  assert.equal(outcome.status, 'refused');
  assert.equal(outcome.message, 'gap-reworded replaces losses-show, which would still add every-loss-is-a-gap: set them again or remove them in gap-reworded');
  assert.equal(outcome.changed.size, 0);

  const removing = replacing({ keys: { replaces: ['losses-show'], sets: undefined, removes: ['every-loss-is-a-gap'] } });
  const done = take(removing, 'gap-reworded');
  assert.equal(done.status, 'done', `${done.message} ${JSON.stringify(done.errors ?? [])}`);
  const after = apply(removing, done);
  assert.ok(!parse(after, 'record/properties.json').some((p) => p.id === 'every-loss-is-a-gap'));
  assert.deepEqual(parse(after, 'record/decisions/gap-reworded.json'), { asks: 'How is a loss said?', answer: 'As a gap in the record.', replaces: ['losses-show'], verdict: 'take' });
  assert.deepEqual(checkFiles(after).errors, []);
});

test('a take is refused when a decision it replaces is not taken, or was already replaced by another: the message names both', () => {
  // Not taken: the plan already fails the check.
  const pending = sample();
  propose(pending, 'vm-again', { asks: 'Which engine?', answer: 'Firecracker.', replaces: ['linux-vm-engine'] });
  const refusal = take(pending, 'vm-again');
  assert.equal(refusal.status, 'refused');
  assert.ok(refusal.errors?.some((e) => e.field === 'replaces[0]' && /"linux-vm-engine" is not taken/.test(e.message)));
  assert.equal(refusal.changed.size, 0);

  const files = replacing({ keys: { replaces: ['losses-show'] } });
  const after = apply(files, take(files, 'gap-reworded'));
  propose(after, 'gap-again', { asks: 'Again?', answer: 'Yes.', replaces: ['losses-show'] });
  assert.deepEqual(checkFiles(after).errors, []);
  const twice = take(after, 'gap-again');
  assert.equal(twice.status, 'refused');
  assert.equal(twice.message, 'gap-again replaces losses-show, which gap-reworded already replaced: a decision is replaced by one decision only, so gap-again replaces gap-reworded, the later one, instead');
  assert.equal(twice.changed.size, 0);
});

test('drop refuses a replaced decision and a taken decision that replaces; a decision with no verdict that has replaces is dropped and keeps the key', () => {
  const files = replacing({ keys: { replaces: ['losses-show'], sets: undefined, removes: ['every-loss-is-a-gap'] } });
  const after = apply(files, take(files, 'gap-reworded'));
  const replaced = drop(after, 'losses-show');
  assert.equal(replaced.status, 'refused');
  assert.equal(replaced.message, 'losses-show is replaced by gap-reworded: it is no longer in force, and has nothing to drop');
  assert.equal(replaced.changed.size, 0);
  const replacing_ = drop(after, 'gap-reworded');
  assert.equal(replacing_.status, 'refused');
  assert.equal(replacing_.message, 'gap-reworded is taken and replaces losses-show: a decision that replaces gap-reworded comes first');
  assert.equal(replacing_.changed.size, 0);

  const proposal = replacing({ keys: { replaces: ['losses-show'] } });
  const dropped = drop(proposal, 'gap-reworded');
  assert.equal(dropped.status, 'done');
  const entry = parse(apply(proposal, dropped), 'record/decisions/gap-reworded.json');
  assert.equal(entry.verdict, 'drop');
  assert.deepEqual(entry.replaces, ['losses-show']);
  assert.deepEqual(checkFiles(apply(proposal, dropped)).errors, []);
});

test('drop refuses a taken decision that a decision with no verdict would replace, and names it; a dropped one holds nothing back', () => {
  const waiting = sample({
    'engine/decisions/engine-colour.json': { asks: 'Which colour is the engine?', answer: 'Blue.', verdict: 'take' },
    'decisions/nothing-without-a-reason.json': { ...basePlan()['decisions/nothing-without-a-reason.json'], replaces: ['engine-colour'] },
  });
  place(waiting, 'engine-colour');
  assert.deepEqual(checkFiles(waiting).errors, []);
  const refusedDrop = drop(waiting, 'engine-colour');
  assert.equal(refusedDrop.status, 'refused');
  assert.equal(refusedDrop.message, 'engine-colour is named in the replaces of nothing-without-a-reason, not decided: take or drop it first, or remove engine-colour from its replaces');
  assert.equal(refusedDrop.changed.size, 0);

  const skipped = apply(waiting, drop(waiting, 'nothing-without-a-reason'));
  const dropped = drop(skipped, 'engine-colour');
  assert.equal(dropped.status, 'done', dropped.message);
  assert.deepEqual(checkFiles(apply(skipped, dropped)).errors, []);
});

test('drop writes the verdict and keeps sets; it refuses a taken entry that brought statements', () => {
  const files = sample();
  const outcome = drop(files, 'losses-show');
  assert.equal(outcome.status, 'done');
  assert.deepEqual([...outcome.changed.keys()], ['record/decisions/losses-show.json']);
  const after = apply(files, outcome);
  const entry = parse(after, 'record/decisions/losses-show.json');
  assert.equal(entry.verdict, 'drop');
  assert.deepEqual(entry.sets, parse(files, 'record/decisions/losses-show.json').sets);
  assert.deepEqual(checkFiles(after).errors, []);
  assert.equal(drop(after, 'losses-show').status, 'already');

  const taken = drop(files, 'the-design');
  assert.equal(taken.status, 'refused');
  assert.equal(taken.changed.size, 0);
});

test('drop on a feature being built removes started with the verdict, says so, and the plan passes the check', () => {
  const base = basePlan();
  base['engine/decisions/linux-vm-engine.json'] = { ...base['engine/decisions/linux-vm-engine.json'], verdict: 'take' };
  base['engine/features/start-computer.json'] = { ...base['engine/features/start-computer.json'], verdict: 'take', started: '2026-11-18' };
  const files = filesOf(base);
  assert.deepEqual(checkFiles(files).errors, []);
  assert.equal(entryStatus(parsePlan(files).plan.entries['start-computer'], parsePlan(files).plan.entries), 'building');
  const outcome = drop(files, 'start-computer');
  assert.equal(outcome.status, 'done', outcome.message);
  assert.equal(outcome.message, 'dropped start-computer, which was being built: its started date is removed');
  const after = apply(files, outcome);
  const entry = parse(after, 'engine/features/start-computer.json');
  assert.equal(entry.verdict, 'drop');
  assert.equal('started' in entry, false);
  assert.deepEqual(checkFiles(after).errors, []);
  // A feature that was not being built keeps the plain message.
  const waiting = drop(sample(), 'start-computer');
  assert.equal(waiting.message, 'dropped start-computer');
});

/** A small seeded generator, so a failing sequence can be replayed. @param {number} seed */
function random(seed) {
  let state = seed >>> 0;
  const next = () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
  return { int: (n) => Math.floor(next() * n), pick: (list) => list[Math.floor(next() * list.length)] };
}

const PROPERTY_LISTS = ['properties', 'engine/properties', 'record/properties'];

test('model: after any sequence of takes and drops the plan passes the check, no file is deleted, the lists equal a reference, and every statement is in the adds of exactly one taken entry that is not replaced', () => {
  const total = { owned: 0, confirmed: 0 };
  for (let seed = 1; seed <= 60; seed += 1) {
    const rng = random(seed);
    const files = sample();
    // The reference: each statement this test made, by id, with its list, its text and the entry that brought it.
    /** @type {Map<string, { list: string, value: any, by: string }>} */
    const model = new Map();
    /** @type {Map<string, { verdict: 'take' | 'drop', by?: 'agents', answered: boolean, replaces: string[], needs: string[] }>} the decisions this test made, and who ruled on each */
    const decisions = new Map();
    /** The decisions the owner ever ruled on: the history's answer, as the command would give it. */
    const ownerRuled = new Set();
    const agents = (by) => (by ? { by, ownerRuled: (other) => ownerRuled.has(other) } : {});
    /** A proposal its author withdraws, as an edit of the plan, not by the tool. */
    const withdraw = (id) => {
      files.delete(`decisions/${id}.json`);
      const order = parse(files, 'order.json');
      for (const stage of order.stages) stage.entries = stage.entries.filter((other) => other !== id);
      files.set('order.json', canonicalText(order));
    };
    /** @type {Map<string, string>} each replaced decision, and the decision that replaced it */
    const replaced = new Map();
    const statementsOf = (owner) => [...model].filter(([, statement]) => statement.by === owner).map(([sid]) => sid);
    const taken = () => [...decisions].filter(([, d]) => d.verdict === 'take').map(([owner]) => owner);
    const counts = { done: 0, refused: 0, already: 0 };
    const seen = { owned: 0, confirmed: 0 };

    /** Applies an outcome, checking that a verdict deletes no file and writes only text. */
    const commit = (outcome, label) => {
      const before = new Set(files.keys());
      for (const [name, text] of outcome.changed) {
        assert.equal(typeof text, 'string', `${label}: ${name} is written as text`);
        files.set(name, text);
      }
      for (const name of before) assert.ok(files.has(name), `${label}: ${name} was deleted`);
    };
    /** A new decision: statements set again or new, removes, and the decisions it replaces with most of their statements covered. */
    const generate = (id, step, { replacing = true, removing = true, plain = false } = {}) => {
      const sets = [];
      const used = new Set();
      const removes = [];
      const setStatement = (sid, reuse) => {
        if (used.has(sid) || removes.includes(sid)) return;
        used.add(sid);
        // A property set again may land in another list of properties: it moves there.
        const before = reuse ? model.get(sid).list : rng.pick(['words', 'principles', 'property']);
        const list = before === 'property' || PROPERTY_LISTS.includes(before) ? rng.pick(PROPERTY_LISTS) : before;
        const value = list === 'words'
          ? { word: sid.replaceAll('-', ' '), means: `Meaning of step ${step}.` }
          : list === 'principles'
            ? { id: sid, says: `Says, at step ${step}.`, asks: 'Asks what?', why: 'Because.' }
            : { id: sid, says: `Holds, at step ${step}.`, kind: 'safety', from: 'less-sure-less-power', check: ['test'] };
        sets.push({ in: list, ...value });
      };
      for (let i = plain ? -1 : rng.int(3); i >= 0; i -= 1) {
        const existing = [...model.keys()].filter((sid) => !used.has(sid));
        const reuse = existing.length > 0 && rng.int(2) === 0;
        setStatement(reuse ? rng.pick(existing) : `gen-${seed}-${step}-${i}`, reuse);
      }
      const remove = (sid) => { if (!used.has(sid) && !removes.includes(sid)) removes.push(sid); };
      const candidates = [...model.keys()].filter((sid) => !used.has(sid));
      if (!plain && removing && candidates.length && rng.int(3) === 0) remove(rng.pick(candidates));
      // The decisions it replaces: mostly in force, now and then one already replaced.
      const replaces = [];
      if (replacing && !plain) {
        for (let n = rng.int(3) === 0 ? 1 + rng.int(2) : 0; n > 0; n -= 1) {
          const inForce = taken().filter((owner) => !replaced.has(owner) && !replaces.includes(owner));
          const pool = rng.int(6) === 0 ? taken().filter((owner) => replaced.has(owner) && !replaces.includes(owner)) : inForce;
          if (!pool.length) continue;
          const target = rng.pick(pool);
          replaces.push(target);
          // Each of its statements is set again or removed, but now and then one is forgotten.
          for (const sid of statementsOf(target)) {
            if (rng.int(8) === 0) continue;
            if (removing && rng.int(2) === 0) remove(sid);
            else setStatement(sid, true);
          }
        }
      }
      const answered = !sets.length || rng.int(3) !== 0;
      const needs = taken().length && rng.int(3) === 0 ? [rng.pick(taken())] : [];
      const decision = { asks: `Step ${step}?`, ...(answered ? { answer: 'Yes.' } : {}), ...(needs.length ? { needs } : {}), ...(replaces.length ? { replaces } : {}), ...(sets.length ? { sets } : {}), ...(removes.length ? { removes } : {}) };
      return { decision, sets, used, removes, replaces, answered, needs };
    };

    for (let step = 0; step < 16; step += 1) {
      const id = `step-${seed}-${step}`;
      const label = `seed ${seed} step ${step}`;
      const op = rng.int(10);
      if (op < 6) {
        // A new decision, and the owner's yes.
        // The agents' proposals are often bare: nothing they say sets a statement.
        const by = rng.int(3) === 0 ? 'agents' : undefined;
        const { decision, sets, used, removes, replaces, answered, needs } = generate(id, step, { plain: by && rng.int(3) > 0 });
        files.set(`decisions/${id}.json`, canonicalText(canonicalDecision(decision)));
        place(files, id);
        assert.deepEqual(checkFiles(files).errors, [], `${label}: the proposal passes the check`);
        const gone = new Set([...used, ...removes]);
        const already = replaces.filter((target) => replaced.has(target));
        const kept = replaces.filter((target) => statementsOf(target).some((sid) => !gone.has(sid)));
        const emptied = taken().filter((owner) => !replaced.has(owner) && !decisions.get(owner).answered && !replaces.includes(owner) && statementsOf(owner).every((sid) => gone.has(sid)));
        const outcome = take(files, id, agents(by));
        // The agents never write the owner's statements, whatever else holds.
        const touches = sets.length > 0 || removes.length > 0;
        const expected = by && touches ? /those are the owner's/ : already.length ? /already replaced/ : kept.length ? /would still add/ : emptied.length ? /would be left with nothing in force/ : null;
        if (expected) {
          assert.equal(outcome.status, 'refused', `${label}: expected ${expected}`);
          assert.match(outcome.message, expected, label);
          assert.equal(outcome.changed.size, 0, label);
          counts.refused += 1;
          withdraw(id);
        } else {
          assert.equal(outcome.status, 'done', `${label}: ${outcome.message} ${JSON.stringify(outcome.errors ?? [])}`);
          counts.done += 1;
          commit(outcome, label);
          for (const { in: list, ...value } of sets) model.set(list === 'words' ? wordId(value.word) : value.id, { list, value, by: id });
          for (const sid of removes) model.delete(sid);
          for (const target of replaces) replaced.set(target, id);
          decisions.set(id, { verdict: 'take', by, answered, replaces, needs });
          if (!by) ownerRuled.add(id);
        }
      } else if (op < 8) {
        // A new decision, and the owner's no: it keeps its sets and replaces.
        const by = rng.int(3) === 0 ? 'agents' : undefined;
        const { decision, replaces, needs } = generate(id, step, { removing: false, plain: by && rng.int(3) > 0 });
        files.set(`decisions/${id}.json`, canonicalText(canonicalDecision({ ...decision, answer: 'Yes.' })));
        place(files, id);
        const outcome = drop(files, id, agents(by));
        if (by && Array.isArray(decision.sets) && decision.sets.length) {
          assert.equal(outcome.status, 'refused', `${label}: the agents do not drop what sets statements`);
          assert.match(outcome.message, /those are the owner's/, label);
          assert.equal(outcome.changed.size, 0, label);
          counts.refused += 1;
          withdraw(id);
        } else {
          assert.equal(outcome.status, 'done', `${label}: ${outcome.message} ${JSON.stringify(outcome.errors ?? [])}`);
          counts.done += 1;
          commit(outcome, label);
          assert.deepEqual(parse(files, `decisions/${id}.json`).replaces, replaces.length ? replaces : undefined, label);
          decisions.set(id, { verdict: 'drop', by, answered: true, replaces, needs });
          if (!by) ownerRuled.add(id);
        }
      } else if (decisions.size && rng.int(3) === 0) {
        // The agents try a verdict on an entry the owner ruled on, even one the owner's verdict was cleared from by an edit: refused, whatever it holds.
        const [owned] = [...ownerRuled].filter((other) => decisions.has(other)).sort().slice(rng.int(3));
        if (owned) {
          const edited = new Map(files);
          const name = `decisions/${owned}.json`;
          if (rng.int(2) === 0) {
            const { verdict: _verdict, by: _by, ...cleared } = parse(files, name);
            edited.set(name, canonicalText(canonicalDecision(cleared)));
            // Only an edit that leaves a valid plan is an attack the tool can see: the rest fails the check first.
            if (checkFiles(edited).errors.length) edited.set(name, /** @type {string} */ (files.get(name)));
          }
          for (const operation of [take, drop]) {
            const outcome = operation(edited, owned, agents('agents'));
            assert.equal(outcome.status, 'refused', `${label}: ${owned} was ruled on by the owner`);
            assert.match(outcome.message, /ruled on by the owner/, label);
            assert.match(outcome.message, new RegExp(`names ${owned} in its replaces`), label);
            assert.equal(outcome.changed.size, 0, label);
            counts.refused += 1;
          }
          seen.owned += 1;
        }
      } else if (decisions.size && rng.int(2) === 0) {
        // The owner's own take or drop of a decision this test made: on one the agents ruled it is the owner's confirmation, on the owner's it says so.
        const target = rng.pick([...decisions.keys()]);
        const d = decisions.get(target);
        const outcome = (d.verdict === 'take' ? take : drop)(files, target);
        const confirms = d.by === 'agents' && !replaced.has(target);
        assert.equal(outcome.status, confirms ? 'done' : 'already', `${label}: owner ${d.verdict} ${target}: ${outcome.message}`);
        counts[outcome.status] += 1;
        commit(outcome, label);
        if (confirms) {
          assert.deepEqual([...outcome.changed.keys()], [`decisions/${target}.json`], `${label}: only the entry's file changes`);
          Object.assign(d, { by: undefined });
          ownerRuled.add(target);
          seen.confirmed += 1;
        }
      } else if (decisions.size) {
        // The owner's no on a decision this test made.
        const target = rng.pick([...decisions.keys()]);
        const d = decisions.get(target);
        // A dropped decision that names it holds nothing back.
        const named = [...decisions].some(([other, od]) => other !== target && od.verdict !== 'drop' && od.replaces.includes(target));
        const needed = [...decisions].some(([, od]) => od.verdict !== 'drop' && od.needs.includes(target));
        // The agents never overrule the owner: their drop of an entry the owner ruled on is refused, whatever else holds.
        const by = rng.int(3) === 0 ? 'agents' : undefined;
        const outcome = drop(files, target, agents(by));
        const expected = by && ownerRuled.has(target) ? /ruled on by the owner/ : d.verdict === 'drop' ? (!by && d.by === 'agents' ? 'done' : 'already')
          : replaced.has(target) ? /is replaced by/
            : statementsOf(target).length ? /is taken and brought/
              : d.replaces.length ? /a decision that replaces .* comes first/
                : named || needed ? /would fail the check/ : 'done';
        if (expected === 'already' || expected === 'done') assert.equal(outcome.status, expected, `${label}: drop ${target}: ${outcome.message}`);
        else {
          assert.equal(outcome.status, 'refused', `${label}: drop ${target}: expected ${expected}`);
          assert.match(outcome.message, expected, label);
          assert.equal(outcome.changed.size, 0, label);
        }
        counts[outcome.status] += 1;
        commit(outcome, label);
        if (outcome.status === 'done') {
          Object.assign(d, { verdict: 'drop', by });
          if (!by) ownerRuled.add(target);
        }
      }

      // The lists hold exactly the reference's statements, with the sample's own around them.
      const generated = (list, items) => items.filter((item) => (list === 'words' ? wordId(item.word) : item.id).startsWith('gen-'));
      const expectedIn = (list) => [...model.entries()].filter(([, s]) => s.list === list).map(([, s]) => s.value);
      for (const list of ['words', 'principles', ...PROPERTY_LISTS]) {
        const items = files.has(`${list}.json`) ? parse(files, `${list}.json`) : [];
        assert.deepEqual(new Set(generated(list, items).map((w) => JSON.stringify(w))), new Set(expectedIn(list).map((w) => JSON.stringify(w))), `${label}: ${list}`);
      }
      // The replaced decisions are the reference's, and each statement is in the adds of exactly one
      // entry, the one that set it last, which is taken and not replaced.
      // A decision has replaced_by exactly when a taken decision names it in replaces.
      const { plan } = parsePlan(files);
      const fromFiles = new Map(Object.values(plan.entries).map((entry) => [entry.id, replacedBy(entry)]).filter(([, by]) => by));
      assert.deepEqual(fromFiles, replaced, `${label}: replaced`);
      const named = new Map(Object.values(plan.entries).filter((entry) => entry.verdict === 'take').flatMap((entry) => (entry.replaces ?? []).map((target) => [target, entry.id])));
      assert.deepEqual(named, fromFiles, `${label}: replaced_by agrees with replaces`);
      for (const [sid, statement] of model) {
        const owners = Object.values(plan.entries).filter((entry) => (entry.adds ?? []).includes(sid)).map((entry) => entry.id);
        assert.deepEqual(owners, [statement.by], `${label}: ${sid}`);
        assert.equal(entryStatus(plan.entries[statement.by], plan.entries), 'taken', `${label}: ${statement.by} is in force`);
      }
      // Each file says who ruled, as the test did, and `by` follows the verdict.
      for (const [made, d] of decisions) {
        const file = parse(files, `decisions/${made}.json`);
        assert.equal(file.verdict, d.verdict, `${label}: ${made} verdict`);
        assert.equal(file.by, d.by, `${label}: ${made} by`);
        if (d.by) assert.equal(Object.keys(file).at(Object.keys(file).indexOf('verdict') + 1), 'by', `${label}: ${made} key order`);
      }
      assert.deepEqual(checkFiles(files).errors, [], `${label}: check`);
    }
    assert.ok(counts.done > 0, `seed ${seed}: something was done`);
    total.owned += seen.owned;
    total.confirmed += seen.confirmed;
  }
  assert.ok(total.owned > 10 && total.confirmed > 3, `the owner-ruled and confirm paths ran: ${JSON.stringify(total)}`);
});

test('take with by agents writes "by": "agents" right after the verdict, in the canonical form, and the result passes the check', () => {
  const files = sample();
  const outcome = take(files, 'linux-vm-engine', AG);
  assert.equal(outcome.status, 'done');
  const name = 'engine/decisions/linux-vm-engine.json';
  const written = parse(apply(files, outcome), name);
  assert.equal(written.verdict, 'take');
  assert.equal(written.by, 'agents');
  const keys = Object.keys(written);
  assert.equal(keys[keys.indexOf('verdict') + 1], 'by');
  assert.equal(outcome.changed.get(name), canonicalText(canonicalDecision(written)));
  assert.deepEqual(checkFiles(apply(files, outcome)).errors, []);
  // Without by, the file has none.
  assert.equal(Object.hasOwn(parse(apply(files, take(files, 'linux-vm-engine')), name), 'by'), false);
});

/** The sample with an answer on `no-detour`, which nothing needs: an entry either command may rule on. */
const answered = () => sample({ 'record/decisions/no-detour.json': { ...basePlan()['record/decisions/no-detour.json'], answer: 'It is not.' } });

test('drop with by agents writes the verdict and "by": "agents"; it is refused on a verdict the owner wrote, allowed on none or on the agents\' own', () => {
  const files = answered();
  const name = 'record/decisions/no-detour.json';
  const dropped = drop(files, 'no-detour', AG);
  assert.equal(dropped.status, 'done');
  const written = parse(apply(files, dropped), name);
  assert.deepEqual(Object.keys(written).slice(-2), ['verdict', 'by']);
  assert.deepEqual([written.verdict, written.by], ['drop', 'agents']);
  assert.deepEqual(checkFiles(apply(files, dropped)).errors, []);
  // Dropping again: already.
  assert.equal(drop(apply(files, dropped), 'no-detour', AG).status, 'already');

  // An entry the owner took: refused, and nothing is written.
  const refused = drop(files, 'engine-language', AG);
  assert.equal(refused.status, 'refused');
  assert.match(refused.message, /taken by the owner/);
  assert.equal(refused.changed.size, 0);

  // An entry the agents took: the agents may drop it, and the owner's drop removes by.
  const taken = apply(files, take(files, 'no-detour', AG));
  const again = drop(taken, 'no-detour', AG);
  assert.equal(again.status, 'done');
  assert.equal(parse(apply(taken, again), name).by, 'agents');
  const owner = drop(taken, 'no-detour');
  assert.equal(owner.status, 'done');
  assert.equal(Object.hasOwn(parse(apply(taken, owner), name), 'by'), false);
});

test('take with by agents does not turn the owner\'s drop into a yes; an owner take over an agents drop removes by', () => {
  const files = answered();
  const ownerDropped = apply(files, drop(files, 'no-detour'));
  const refused = take(ownerDropped, 'no-detour', AG);
  assert.equal(refused.status, 'refused');
  assert.match(refused.message, /dropped by the owner/);
  const agentsDropped = apply(files, drop(files, 'no-detour', AG));
  assert.equal(take(agentsDropped, 'no-detour', AG).status, 'done');
  const owner = take(agentsDropped, 'no-detour');
  assert.equal(Object.hasOwn(parse(apply(agentsDropped, owner), 'record/decisions/no-detour.json'), 'by'), false);
});

test('agents never write a verdict on an entry the owner ruled on: refused with the way, whether the owner\'s verdict stands or an edit cleared it', () => {
  const files = answered();
  const name = 'record/decisions/no-detour.json';
  const ruled = (ids) => ({ by: 'agents', ownerRuled: (id) => ids.includes(id) });
  // The owner took then dropped nothing here: the history alone says no-detour was ruled on.
  for (const operation of [take, drop]) {
    const refused = operation(files, 'no-detour', ruled(['no-detour']));
    assert.equal(refused.status, 'refused');
    assert.match(refused.message, /no-detour was ruled on by the owner/);
    assert.match(refused.message, /a new decision that names no-detour in its replaces/);
    assert.equal(refused.changed.size, 0);
    // Another entry is not touched by it.
    assert.equal(operation(files, 'no-detour', ruled(['engine-language'])).status, 'done');
  }
  // An edit that cleared the owner's verdict from the file changes nothing: the history still says it.
  const cleared = apply(files, drop(files, 'no-detour'));
  const { verdict: _verdict, ...bare } = parse(cleared, name);
  cleared.set(name, canonicalText(canonicalDecision(bare)));
  assert.equal(take(cleared, 'no-detour', ruled(['no-detour'])).status, 'refused');
  assert.equal(take(cleared, 'no-detour', ruled([])).status, 'done');
  // A feature: the way is not a decision.
  assert.match(take(sample(), 'start-computer', ruled(['start-computer'])).message, /the owner deciding it/);
  // Without the history, the agents cannot tell, and do not write.
  const blind = take(files, 'no-detour', { by: 'agents' });
  assert.equal(blind.status, 'refused');
  assert.match(blind.message, /history is needed/);
});

test('agents never write a verdict on an entry that sets, removes or adds a principle, a property or a word; the owner may', () => {
  const files = sample();
  for (const [operation, id] of [[take, 'losses-show'], [drop, 'losses-show'], [take, 'nothing-without-a-reason'], [drop, 'nothing-without-a-reason']]) {
    const refused = operation(files, id, AG);
    assert.equal(refused.status, 'refused', `${operation.name} ${id}`);
    assert.match(refused.message, /principles, properties or words: those are the owner's/);
    assert.equal(refused.changed.size, 0);
    assert.equal(operation(files, id).status, 'done', `the owner ${operation.name} ${id}`);
  }
  // An entry that removes one.
  const removing = sample({ 'engine/decisions/model-tool.json': { ...basePlan()['engine/decisions/model-tool.json'], answer: 'Yes.', removes: ['rule-reads-only-facts'] } });
  assert.match(take(removing, 'model-tool', AG).message, /removes principles, properties or words/);
  // Dropping an entry that adds statements (taken by the owner) stays refused by its own rule; by the agents the first is the statements one.
  assert.match(drop(files, 'the-design', AG).message, /adds principles, properties or words|taken by the owner/);
});

test('the owner\'s plain take confirms an entry the agents took, and the plain drop one the agents dropped: by is removed, nothing else changes', () => {
  const files = answered();
  const taken = apply(files, take(files, 'linux-vm-engine', AG));
  const confirmed = take(taken, 'linux-vm-engine');
  assert.equal(confirmed.status, 'done');
  assert.deepEqual([...confirmed.changed.keys()], ['engine/decisions/linux-vm-engine.json']);
  const after = apply(taken, confirmed);
  const written = parse(after, 'engine/decisions/linux-vm-engine.json');
  assert.equal(written.verdict, 'take');
  assert.equal(Object.hasOwn(written, 'by'), false);
  assert.deepEqual(checkFiles(after).errors, []);
  // Confirmed, it is the owner's: taking it again says so.
  assert.equal(take(after, 'linux-vm-engine').status, 'already');

  const dropped = apply(files, drop(files, 'no-detour', AG));
  const unchanged = drop(dropped, 'no-detour');
  assert.equal(unchanged.status, 'done');
  assert.equal(Object.hasOwn(parse(apply(dropped, unchanged), 'record/decisions/no-detour.json'), 'by'), false);
  assert.equal(parse(apply(dropped, unchanged), 'record/decisions/no-detour.json').verdict, 'drop');
  assert.equal(drop(apply(dropped, unchanged), 'no-detour').status, 'already');
  // The owner's own verdict is already what it is.
  assert.equal(take(files, 'engine-language').status, 'already');
});
