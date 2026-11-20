import { test } from 'node:test';
import assert from 'node:assert/strict';
import { checkFiles } from '../check/index.js';
import { canonicalDecision, canonicalText } from '../model/canonical.js';
import { wordId } from '../model/names.js';
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

test('take writes the sentences, adds and the verdict, and the result passes the check', () => {
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

test('take accepts a decision that puts forward only sentences, and refuses one with neither an answer nor a sets', () => {
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

test('replacing a sentence moves its id out of the earlier entry\'s adds', () => {
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

test('a sets item with the id of a sentence of another list of its kind moves it there, and the result passes the check', () => {
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

test('removes deletes the sentence and takes it out of the adds that brought it; an entry that sets nothing gets no adds', () => {
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

test('drop writes the verdict and keeps sets; it refuses a taken entry that brought sentences', () => {
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

test('model: any sequence of takes keeps the lists equal to a reference, every sentence in the adds of exactly one taken entry, and every taken decision with an answer or an adds', () => {
  for (let seed = 1; seed <= 25; seed += 1) {
    const rng = random(seed);
    const files = sample();
    // The reference: each sentence this test made, by id, with its list, its text and the entry that brought it.
    /** @type {Map<string, { list: string, value: any, by: string }>} */
    const model = new Map();
    // The taken decisions that had no answer: their adds stands for it.
    const onlySets = new Set();
    for (let step = 0; step < 12; step += 1) {
      const id = `step-${seed}-${step}`;
      const sets = [];
      const used = new Set();
      for (let i = rng.int(3); i >= 0; i -= 1) {
        const existing = [...model.keys()].filter((sid) => !used.has(sid));
        const reuse = existing.length && rng.int(2) === 0;
        const sid = reuse ? rng.pick(existing) : `gen-${seed}-${step}-${i}`;
        if (used.has(sid)) continue;
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
      }
      const candidates = [...model.keys()].filter((sid) => !used.has(sid));
      const removes = candidates.length && rng.int(3) === 0 ? [rng.pick(candidates)] : [];
      // A decision that sets sentences may have no answer.
      const answered = !sets.length || rng.int(3) !== 0;
      files.set(`decisions/${id}.json`, canonicalText(canonicalDecision({ asks: `Step ${step}?`, ...(answered ? { answer: 'Yes.' } : {}), ...(sets.length ? { sets } : {}), ...(removes.length ? { removes } : {}) })));
      place(files, id);

      // A take that would leave a decision without an answer or an adds is refused and writes nothing;
      // with its own answer written, the same take is then done.
      const gone = new Set([...used, ...removes]);
      const emptied = [...onlySets].filter((owner) => [...model].filter(([, sentence]) => sentence.by === owner).every(([sid]) => gone.has(sid)));
      let outcome = take(files, id);
      if (emptied.length) {
        assert.equal(outcome.status, 'refused', `seed ${seed} step ${step}: would empty the adds of ${emptied.join(', ')}`);
        assert.equal(outcome.changed.size, 0);
        assert.match(JSON.stringify(outcome.errors ?? []), /without an answer or adds/);
        for (const owner of emptied) {
          files.set(`decisions/${owner}.json`, canonicalText(canonicalDecision({ ...parse(files, `decisions/${owner}.json`), answer: 'Yes.' })));
          onlySets.delete(owner);
        }
        outcome = take(files, id);
      }
      assert.equal(outcome.status, 'done', `seed ${seed} step ${step}: ${outcome.message} ${JSON.stringify(outcome.errors ?? [])}`);
      if (!answered) onlySets.add(id);
      for (const [name, text] of outcome.changed) files.set(name, text);
      for (const { in: list, ...value } of sets) model.set(list === 'words' ? wordId(value.word) : value.id, { list, value, by: id });
      for (const sid of removes) model.delete(sid);

      // The lists hold exactly the reference's sentences, with the sample's own around them.
      const generated = (list, items) => items.filter((item) => (list === 'words' ? wordId(item.word) : item.id).startsWith('gen-'));
      const expected = (list) => [...model.entries()].filter(([, s]) => s.list === list).map(([, s]) => s.value);
      for (const list of ['words', 'principles', ...PROPERTY_LISTS]) {
        const items = files.has(`${list}.json`) ? parse(files, `${list}.json`) : [];
        assert.deepEqual(new Set(generated(list, items).map((w) => JSON.stringify(w))), new Set(expected(list).map((w) => JSON.stringify(w))), `seed ${seed} step ${step}: ${list}`);
      }
      // Each generated sentence is in the adds of exactly one entry, the one that set it last.
      for (const [sid, sentence] of model) {
        const owners = [...files.keys()].filter((name) => /decisions\/[^/]+\.json$/.test(name)).filter((name) => (parse(files, name).adds ?? []).includes(sid));
        assert.deepEqual(owners, [`decisions/${sentence.by}.json`], `seed ${seed} step ${step}: ${sid}`);
      }
      assert.deepEqual(checkFiles(files).errors, [], `seed ${seed} step ${step}: check`);
    }
  }
});
