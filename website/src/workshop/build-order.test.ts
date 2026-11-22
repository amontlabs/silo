// All read from the top in Build order is an order to decide in: no decision still open shows above an open one it
// needs, on fixtures, on plans made at random and on the plan of this repository.
import { join } from 'node:path';
import { describe, expect, it } from 'vitest';
import { loadDir } from '../../../tools/plan/read/index.js';
import { allSections, graphOf, locate, NO_QUERY } from './all';
import type { Item, Query, Section } from './all';
import { buildDataFrom } from './data';
import { planFiles, STAGES } from './fixtures';
import type { FixtureDecision } from './fixtures';
import { NO_MAP } from './map';
import { buildPlan, withVerdicts } from './plan';
import type { BuildData, Verdict } from './plan';

const REPOSITORY = join(import.meta.dirname, '../../..');
const UNDECIDED: Query = { ...NO_QUERY, states: ['undecided'] };

/** The entries of a list as they read from the top, with every fold and every "N more" open. */
const readFromTheTop = (sections: Section[]): string[] => sections.flatMap((section) => section.items.flatMap((item: Item) => {
  if (item.type === 'bundle') return (item.passing?.length ? item.passing : item.kids).map((kid) => kid.key);
  if (item.type === 'feature' && item.foldable) return [item.key, ...(item.passing?.length ? item.passing : item.kids).map((kid) => kid.key)];
  return [item.key];
}));

/** What breaks the order: an open decision missing, one no feature rests on listed twice, or one above what it needs. */
function wrongIn(data: BuildData, query: Query): string[] {
  const plan = buildPlan(data, null);
  const graph = graphOf(plan);
  const read = readFromTheTop(allSections(plan, graph, data.stages, query));
  const at = new Map<string, number>();
  read.forEach((key, index) => { if (!at.has(key)) at.set(key, index); });
  const held = new Set([...graph.decisions.values()].flat().map((row) => row.key));
  const wrong: string[] = [];
  for (const row of graph.rows.values()) {
    if (row.of !== 'decision' || row.status !== 'undecided') continue;
    if (!at.has(row.key)) wrong.push(`${row.key} is not listed`);
    else if (!held.has(row.key) && read.lastIndexOf(row.key) !== at.get(row.key)) wrong.push(`${row.key} is listed twice`);
    for (const need of graph.needs.get(row.key)!) {
      const other = graph.rows.get(need)!;
      if (other.of === 'decision' && other.status === 'undecided' && !(at.get(need)! < at.get(row.key)!)) wrong.push(`${row.key} is above ${need}`);
    }
  }
  return wrong;
}

describe('a decision no feature rests on', () => {
  const data = buildDataFrom(planFiles({
    features: [
      { id: 'near', stage: 'second', part: 'part-a', name: 'Near', what: 'x', needs: ['held-near'] },
      { id: 'far', stage: 'third', part: 'part-b', name: 'Far', what: 'x', needs: ['held-far', 'held-near'] },
      { id: 'last', stage: 'fourth', part: 'part-a', name: 'Last', what: 'x' },
    ],
    decisions: [
      { id: 'early', stage: 'first', part: 'part-c', asks: 'Early?' },
      { id: 'early-next', stage: 'first', part: 'part-a', asks: 'Early, next?', needs: ['early'] },
      { id: 'held-near', stage: 'first', part: 'part-a', asks: 'Held near?' },
      { id: 'held-far', stage: 'first', part: 'part-b', asks: 'Held far?' },
      { id: 'both', stage: 'first', part: 'part-a', asks: 'Both?', needs: ['held-far', 'held-near'] },
      { id: 'one', stage: 'first', part: 'part-c', asks: 'One?', needs: ['held-near', 'early'] },
      { id: 'through', stage: 'first', part: 'part-a', asks: 'Through one?', needs: ['one'] },
      { id: 'later-early', stage: 'fourth', part: 'part-e', asks: 'Early, later?' },
      { id: 'later', stage: 'fourth', part: 'part-a', asks: 'Later?', needs: ['held-near'] },
    ],
  }), { map: NO_MAP });
  const listOf = (of: BuildData, query: Query = NO_QUERY) => {
    const plan = buildPlan(of, null);
    return allSections(plan, graphOf(plan), of.stages, query);
  };
  const keysIn = (sections: Section[]) => sections.map((section) => [section.title, section.items.map((item) => item.key.replace('entry-', ''))]);

  it('follows the first feature by which all it needs is listed, or starts its own stage after the rows of early ones', () => {
    const sections = listOf(data);
    expect(keysIn(sections)).toEqual([
      ['First', ['bundle-first.part-c', 'bundle-first.part-a']],
      ['Second', ['near', 'one', 'through']],
      ['Third', ['far', 'both']],
      ['Fourth', ['bundle-fourth.part-e', 'later', 'last']],
    ]);
    expect(locate(sections, 'entry-through', new Set())).toEqual({ instance: 'entry-through', holder: null, more: false });
  });

  it('keeps that place under a filter that takes its feature out of the list', () => {
    const taken = withVerdicts(data, new Map(['near', 'held-near', 'early'].map((id) => [id, 'take' as Verdict])));
    expect(keysIn(listOf(taken, UNDECIDED))[1]).toEqual(['Second', ['one', 'through']]);
    // A verdict on a row's first decision moves no row of early ones.
    expect(keysIn(listOf(taken))[0]).toEqual(keysIn(listOf(data))[0]);
  });

  it('is never above an open decision it needs, in this plan', () => {
    for (const query of [NO_QUERY, UNDECIDED]) expect(wrongIn(data, query)).toEqual([]);
  });
});

/** A number from 0 to 1 at each call, the same ones for the same seed. */
function randomOf(seed: number) {
  let n = seed;
  return () => ((n = (n * 1664525 + 1013904223) % 4294967296) / 4294967296);
}

/** A plan at random. The decisions of a part come together, so its row of early ones can stand in the plan's order. */
function randomPlan(seed: number): BuildData {
  const random = randomOf(seed * 7919);
  const stages = STAGES.slice(0, 4).map((stage) => stage.id);
  const decisions: FixtureDecision[] = [];
  const parts = ['part-d', 'part-b', null, 'part-a', 'part-c'];
  for (let i = 0; i < 30; i += 1) {
    const needs = decisions.filter(() => random() < 0.06).map((one) => one.id);
    const taken = random() < 0.25;
    decisions.push({ id: `q${i}`, stage: stages[Math.floor(i / 8)], part: parts[Math.floor(i / 6)], asks: `Question ${i}?`, ...(needs.length ? { needs } : {}), ...(taken ? { answer: 'Yes.' } : {}) });
  }
  const features = Array.from({ length: 6 }, (_, i) => {
    const stage = Math.floor(i / 2) + 1;
    const needs = decisions.filter((one) => stages.indexOf(one.stage) <= stage && random() < 0.1).map((one) => one.id);
    return { id: `f${i}`, stage: stages[stage], part: 'part-e', name: `Feature ${i}`, what: 'x', ...(needs.length ? { needs } : {}) };
  });
  const data = buildDataFrom(planFiles({ features, decisions }), { map: NO_MAP });
  return withVerdicts(data, new Map(decisions.filter((one) => one.answer).map((one) => [one.id, 'take' as Verdict])));
}

describe('going down Build order', () => {
  it('meets no open decision before an open one it needs, whatever the plan', () => {
    for (let seed = 1; seed <= 300; seed += 1) {
      const data = randomPlan(seed);
      for (const query of [NO_QUERY, UNDECIDED]) expect(wrongIn(data, query), `seed ${seed}`).toEqual([]);
    }
  });

  it('meets none in the plan of this repository', () => {
    const data = buildDataFrom(loadDir(join(REPOSITORY, 'plan')), { map: NO_MAP });
    for (const query of [NO_QUERY, UNDECIDED]) expect(wrongIn(data, query)).toEqual([]);
  });
});
