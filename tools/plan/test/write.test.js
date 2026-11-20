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
  entry.sets[0] = { in: 'engine/properties', id: 'classifier-never-acts', says: 'A classifier never acts on the engine.', kind: 'safety', from: 'less-sure-less-power', check: ['model'] };
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
  assert.equal(refusedDrop.message, 'engine-colour is named in the replaces of nothing-without-a-reason, not decided yet: take or drop it first, or remove engine-colour from its replaces');
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
  for (let seed = 1; seed <= 60; seed += 1) {
    const rng = random(seed);
    const files = sample();
    // The reference: each statement this test made, by id, with its list, its text and the entry that brought it.
    /** @type {Map<string, { list: string, value: any, by: string }>} */
    const model = new Map();
    /** @type {Map<string, { verdict: 'take' | 'drop', answered: boolean, replaces: string[], needs: string[] }>} the decisions this test made */
    const decisions = new Map();
    /** @type {Map<string, string>} each replaced decision, and the decision that replaced it */
    const replaced = new Map();
    const statementsOf = (owner) => [...model].filter(([, statement]) => statement.by === owner).map(([sid]) => sid);
    const taken = () => [...decisions].filter(([, d]) => d.verdict === 'take').map(([owner]) => owner);
    const counts = { done: 0, refused: 0, already: 0 };

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
    const generate = (id, step, { replacing = true, removing = true } = {}) => {
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
      for (let i = rng.int(3); i >= 0; i -= 1) {
        const existing = [...model.keys()].filter((sid) => !used.has(sid));
        const reuse = existing.length > 0 && rng.int(2) === 0;
        setStatement(reuse ? rng.pick(existing) : `gen-${seed}-${step}-${i}`, reuse);
      }
      const remove = (sid) => { if (!used.has(sid) && !removes.includes(sid)) removes.push(sid); };
      const candidates = [...model.keys()].filter((sid) => !used.has(sid));
      if (removing && candidates.length && rng.int(3) === 0) remove(rng.pick(candidates));
      // The decisions it replaces: mostly in force, now and then one already replaced.
      const replaces = [];
      if (replacing) {
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
        const { decision, sets, used, removes, replaces, answered, needs } = generate(id, step);
        files.set(`decisions/${id}.json`, canonicalText(canonicalDecision(decision)));
        place(files, id);
        assert.deepEqual(checkFiles(files).errors, [], `${label}: the proposal passes the check`);
        const gone = new Set([...used, ...removes]);
        const already = replaces.filter((target) => replaced.has(target));
        const kept = replaces.filter((target) => statementsOf(target).some((sid) => !gone.has(sid)));
        const emptied = taken().filter((owner) => !replaced.has(owner) && !decisions.get(owner).answered && !replaces.includes(owner) && statementsOf(owner).every((sid) => gone.has(sid)));
        const outcome = take(files, id);
        const expected = already.length ? /already replaced/ : kept.length ? /would still add/ : emptied.length ? /would be left with nothing in force/ : null;
        if (expected) {
          assert.equal(outcome.status, 'refused', `${label}: expected ${expected}`);
          assert.match(outcome.message, expected, label);
          assert.equal(outcome.changed.size, 0, label);
          counts.refused += 1;
          // The proposal is withdrawn by its author, as an edit of the plan, not by the tool.
          files.delete(`decisions/${id}.json`);
          const order = parse(files, 'order.json');
          for (const stage of order.stages) stage.entries = stage.entries.filter((other) => other !== id);
          files.set('order.json', canonicalText(order));
        } else {
          assert.equal(outcome.status, 'done', `${label}: ${outcome.message} ${JSON.stringify(outcome.errors ?? [])}`);
          counts.done += 1;
          commit(outcome, label);
          for (const { in: list, ...value } of sets) model.set(list === 'words' ? wordId(value.word) : value.id, { list, value, by: id });
          for (const sid of removes) model.delete(sid);
          for (const target of replaces) replaced.set(target, id);
          decisions.set(id, { verdict: 'take', answered, replaces, needs });
        }
      } else if (op < 8) {
        // A new decision, and the owner's no: it keeps its sets and replaces.
        const { decision, replaces, needs } = generate(id, step, { removing: false });
        files.set(`decisions/${id}.json`, canonicalText(canonicalDecision({ ...decision, answer: 'Yes.' })));
        place(files, id);
        const outcome = drop(files, id);
        assert.equal(outcome.status, 'done', `${label}: ${outcome.message} ${JSON.stringify(outcome.errors ?? [])}`);
        counts.done += 1;
        commit(outcome, label);
        assert.deepEqual(parse(files, `decisions/${id}.json`).replaces, replaces.length ? replaces : undefined, label);
        decisions.set(id, { verdict: 'drop', answered: true, replaces, needs });
      } else if (decisions.size) {
        // The owner's no on a decision this test made.
        const target = rng.pick([...decisions.keys()]);
        const d = decisions.get(target);
        const named = [...decisions].some(([other, od]) => other !== target && od.replaces.includes(target));
        const needed = [...decisions].some(([, od]) => od.verdict !== 'drop' && od.needs.includes(target));
        const outcome = drop(files, target);
        const expected = d.verdict === 'drop' ? 'already'
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
        if (outcome.status === 'done') d.verdict = 'drop';
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
      assert.deepEqual(checkFiles(files).errors, [], `${label}: check`);
    }
    assert.ok(counts.done > 0, `seed ${seed}: something was done`);
  }
});
