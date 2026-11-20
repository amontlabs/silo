import { describe, expect, it } from 'vitest';
import { buildDataFrom, PlanFileError } from './data';
import { buildData, DECISIONS, FEATURES, issue, liveState, PARTS, planFiles } from './fixtures';
import { allRows, buildPlan } from './plan';
import { formatHash, RESERVED_ADDRESSES, resolveHash } from './routes';

/** The fixture plan with one more feature in part-a, whose id looks like a suggestion's key, and a suggestion that has that key. */
function withIssueLikeEntry() {
  const data = buildData({ features: [...FEATURES, { id: 'issue-42', stage: 'second', part: 'part-a', name: 'An entry named like an issue', what: 'Its id begins with issue-.' }] });
  const plan = buildPlan(data, liveState({ 42: issue({ kind: 'suggestion', title: 'A suggestion numbered 42', author: 'mara-k', bot: false }) }));
  const rows = new Map(allRows(plan).map((row) => [row.key, row]));
  return { plan, rows };
}

describe('an address that names an entry whose id begins with issue-', () => {
  it('opens the entry, not the suggestion that has the same key, whatever place the address names', () => {
    const { plan, rows } = withIssueLikeEntry();
    expect(rows.get('issue-42')?.kind).toBe('suggestion');
    for (const hash of ['#part-a/issue-42', '#part-b/issue-42', '#product/issue-42', '#left/issue-42', '#suggested/issue-42']) {
      expect([hash, resolveHash(hash, plan, rows)]).toEqual([hash, { place: { kind: 'part', part: 'part-a' }, key: 'entry-issue-42' }]);
    }
    expect(formatHash({ kind: 'part', part: 'part-a' }, 'entry-issue-42')).toBe('#part-a/issue-42');
  });

  it('still opens a suggestion by its key when no entry has that id', () => {
    const { plan, rows } = withIssueLikeEntry();
    expect(resolveHash('#suggested/issue-31', plan, rows)).toEqual({ place: { kind: 'suggested' }, key: 'issue-31' });
    expect(resolveHash('#issue-31', plan, rows)).toEqual({ place: { kind: 'suggested' }, key: 'issue-31' });
  });
});

describe('a part whose id is an address the page keeps', () => {
  for (const id of RESERVED_ADDRESSES) {
    it(`stops the build on a part named "${id}", with the part and the fix`, () => {
      const parts = PARTS.map((part) => (part.id === 'part-e' ? { ...part, id } : part));
      const features = FEATURES.map((feature) => (feature.part === 'part-e' ? { ...feature, part: id } : feature));
      const decisions = DECISIONS.map((decision) => (decision.part === 'part-e' ? { ...decision, part: id } : decision));
      const files = planFiles({ parts, features, decisions });
      expect(() => buildDataFrom(files)).toThrow(PlanFileError);
      expect(() => buildDataFrom(files)).toThrow(`plan/parts.json: the part "${id}" takes an address the Workshop keeps for its own page (#${id}). Fix: give the part another id`);
    });
  }
});
