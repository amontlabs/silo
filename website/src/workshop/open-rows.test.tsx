// A row that is open when its state changes: it keeps its place and stays listed in All, under a fold and in a place,
// until it closes or what the list shows is changed. The counts follow the state at once.
import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { allSections, countOf, formatQuery, graphOf, NO_QUERY, partSections, productSections, rowKeyOf, underKey } from './all';
import type { Item, Query } from './all';
import { buildDataFrom } from './data';
import { CONFIRM_AFTER_MS } from './Entry';
import { planFiles } from './fixtures';
import { NO_MAP } from './map';
import { buildPlan, withVerdicts } from './plan';
import type { Plan, Verdict } from './plan';
import { clock, mockApi, owner, renderApp } from './test-support';

/** "top" folds eight decisions, the first two of them under "next" too; four are under no feature, one of them after "top". */
const DATA = {
  ...buildDataFrom(planFiles({
    features: [
      { id: 'top', stage: 'second', part: 'part-a', name: 'A top', what: 'x', needs: ['q0', 'q1', 'q2', 'q3', 'q4', 'q5', 'q6', 'q7'] },
      { id: 'next', stage: 'third', part: 'part-c', name: 'A next', what: 'x', needs: ['q0', 'q1'] },
    ],
    decisions: [
      { id: 'one', stage: 'first', part: null, asks: 'One?' },
      { id: 'two', stage: 'first', part: null, asks: 'Two?', answer: 'Yes.', verdict: 'take' },
      { id: 'alone', stage: 'first', part: 'part-a', asks: 'Alone?' },
      ...Array.from({ length: 8 }, (_, i) => ({
        id: `q${i}`, stage: 'second', part: i < 5 ? 'part-a' : 'part-c', asks: `Question ${i}?`,
        ...(i === 6 ? { answer: 'Yes.', verdict: 'take' as const } : {}),
      })),
      { id: 'after', stage: 'second', part: 'part-a', asks: 'After that question?', needs: ['q0'] },
    ],
  }), { map: NO_MAP }),
  builtFrom: 'c'.repeat(40),
};

afterEach(() => localStorage.clear());

async function openPage(at: string) {
  const time = clock();
  mockApi({ me: { ...owner, owner: true }, routes: { 'POST verdict': () => ({ status: 200, body: { ok: true, commit: 'e'.repeat(40) } }) } });
  history.replaceState(null, '', `/workshop${at}`);
  await renderApp(DATA);
  return time;
}

const itemAt = (id: string) => document.getElementById(id) as HTMLElement;
const hitOf = (id: string) => itemAt(id).querySelector(':scope > .wa-row > .wa-hit') as HTMLElement;
const press = (id: string) => act(async () => { fireEvent.click(hitOf(id)); });
const kidsOf = (id: string) => [...itemAt(id).querySelectorAll('.wa-kids > .wa-item')].map((item) => rowKeyOf(item.id));
const drawn = () => [...document.querySelectorAll('.wa-list .wa-item')].map((item) => item.id);
const meter = (id: string) => itemAt(id).querySelector(':scope > .wa-row .wa-meter')?.textContent;

/** Take or Drop on an open row, both presses, until the page shows the new state. */
async function rule(id: string, verdict: Verdict, time: ReturnType<typeof clock>) {
  const panel = () => within(document.getElementById(`${id}-more`) as HTMLElement);
  fireEvent.click(await panel().findByRole('button', { name: verdict === 'take' ? 'Take' : 'Drop' }));
  time.advance(CONFIRM_AFTER_MS);
  fireEvent.click(panel().getByRole('button', { name: `Confirm ${verdict}` }));
  // The row may be drawn in another place by then: the plan kept by the page says when the verdict is in.
  await waitFor(() => expect(localStorage.length && Object.values({ ...localStorage }).join()).toContain(`"entry":"${rowKeyOf(id).slice('entry-'.length)}"`));
  await act(async () => {});
}

describe('a row that is open when its state changes', () => {
  it('stays in All under "not decided", in its fold, and leaves once closed; the counts follow at once', async () => {
    const time = await openPage('?view=all&state=undecided');
    const at = 'entry-q1@entry-top';
    await press(at);
    const before = drawn();
    await rule(at, 'take', time);
    expect(drawn()).toEqual(before);
    expect(hitOf(at)).toHaveAttribute('aria-expanded', 'true');
    expect(meter('entry-top')).toBe('2/8');
    expect(document.querySelector('.wa-count')).toHaveTextContent('11 of 14');

    hitOf(at).focus();
    await press(at);
    expect(drawn().filter((id) => rowKeyOf(id) === 'entry-q1')).toEqual([]);
    // The list goes on from the row that took its place.
    expect(document.activeElement).toBe(hitOf('entry-q2@entry-top'));
  });

  it('stays in All when it is a stage\'s last decision that no feature rests on, with the row that folds it', async () => {
    const time = await openPage('?view=all&state=undecided');
    const at = 'entry-alone@bundle-first.part-a';
    await press(at);
    await rule(at, 'drop', time);
    expect(hitOf(at)).toHaveAttribute('aria-expanded', 'true');
    await press(at);
    expect(itemAt('bundle-first.part-a')).toBeNull();
  });

  it('keeps its place in a fold, then sorts after the others once closed', async () => {
    const time = await openPage('?view=all');
    fireEvent.click(within(itemAt('entry-top')).getByRole('button', { name: /^Its decisions/ }));
    const at = 'entry-q1@entry-top';
    await press(at);
    const before = kidsOf('entry-top');
    await rule(at, 'take', time);
    expect(kidsOf('entry-top')).toEqual(before);
    await press(at);
    expect(kidsOf('entry-top')).toEqual(['q0', 'q2', 'q3', 'q4', 'q5', 'q7'].map((id) => `entry-${id}`));
  });

  it('is not moved behind "N more" in a part\'s place', async () => {
    const time = await openPage('#part-a');
    fireEvent.click(within(itemAt('entry-top')).getByRole('button', { name: /^Its decisions/ }));
    const at = 'entry-q0@entry-top';
    await press(at);
    await rule(at, 'take', time);
    expect(kidsOf('entry-top')[0]).toBe('entry-q0');
    expect(hitOf(at)).toHaveAttribute('aria-expanded', 'true');
    expect(meter('entry-top')).toBe('2/8');
    expect(document.querySelector('.ws-block')).toHaveTextContent('Decisions: 1 decided');
    await press(at);
    expect(itemAt(at)).toBeNull();
  });

  it('shows the new state where Take was pressed, though what was over it has gone', async () => {
    const time = await openPage('?view=all&state=undecided');
    const at = 'entry-q1@entry-top';
    await press(at);
    // The line of the state is 700 px down while it offers Take, and 100 once the choice over it has gone.
    vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function (this: Element) {
      const top = !this.classList.contains('ws-state') ? 0 : this.querySelector('.ws-verdict') ? 700 : 100;
      return { top, y: top, left: 0, x: 0, right: 0, bottom: top, width: 0, height: 0, toJSON: () => '' };
    });
    const scrolls = vi.fn();
    vi.stubGlobal('scrollBy', scrolls);
    await rule(at, 'take', time);
    // Once, though the decision is drawn open under two features.
    expect(scrolls.mock.calls).toEqual([[{ top: -600, behavior: 'instant' }]]);
  });

  it('goes when the filter changes, though it is open', async () => {
    const time = await openPage('?view=all&state=undecided');
    const at = 'entry-q1@entry-top';
    await press(at);
    await rule(at, 'take', time);
    fireEvent.click(screen.getByRole('button', { name: 'Filter' }));
    fireEvent.click(screen.getByRole('menuitem', { name: 'Part' }));
    fireEvent.click(within(screen.getByRole('menu', { name: 'Part' })).getByRole('menuitemcheckbox', { name: /^Part A/ }));
    await act(async () => {});
    expect(itemAt(at)).toBeNull();
    expect(location.hash).toBe('');
  });
});

describe('a change in Display', () => {
  it('lets a row that is open go, as a change of filter does', async () => {
    const time = await openPage('?view=all&state=undecided');
    const at = 'entry-q1@entry-top';
    await press(at);
    await rule(at, 'take', time);
    fireEvent.click(screen.getByRole('button', { name: 'Display' }));
    fireEvent.click(screen.getByRole('menuitemcheckbox', { name: 'Features' }));
    await act(async () => {});
    expect(drawn().filter((id) => rowKeyOf(id) === 'entry-q1')).toEqual([]);
    expect(location.hash).toBe('');
  });

  it('keeps in sight an open row that it lets move behind "N more", and the folds made by hand', async () => {
    const time = await openPage('?view=all');
    fireEvent.click(within(itemAt('entry-top')).getByRole('button', { name: /^Its decisions/ }));
    fireEvent.click(within(itemAt('entry-next')).getByRole('button', { name: /^Its decisions/ }));
    const at = 'entry-q0@entry-top';
    await press(at);
    await rule(at, 'take', time);
    fireEvent.click(screen.getByRole('button', { name: 'Display' }));
    fireEvent.click(screen.getByRole('menuitemcheckbox', { name: 'Films' }));
    await act(async () => {});
    expect(kidsOf('entry-top').indexOf('entry-q0')).toBe(6);
    expect(hitOf(at)).toHaveAttribute('aria-expanded', 'true');
    expect(kidsOf('entry-next')).toEqual(['entry-q1', 'entry-q0']);
  });
});

/** A number from 0 to 1 at each call, the same ones for the same seed. */
function randomOf(seed: number) {
  let n = seed;
  return () => ((n = (n * 1664525 + 1013904223) % 4294967296) / 4294967296);
}

/**
 * The rule against a reference: random openings, closings, verdicts and changes of what the list shows. The reference
 * never holds a row: it lists, by the rules alone, a plan in which each open row has the verdict it had when it opened.
 */
describe('the lists, whatever is pressed in whatever order', () => {
  it.each([1, 2, 3, 4, 5, 6, 7, 8])('are those of the reference after every step (seed %i)', async (seed) => {
    const time = await openPage('?view=all&state=undecided');
    const random = randomOf(seed);
    const one = <T,>(list: T[]): T => list[Math.floor(random() * list.length)];
    const ruled = new Map<string, Verdict>();
    /** The rows open, each with the page's own verdict on it when it opened. */
    const open = new Map<string, Verdict | undefined>();
    const now = () => open.forEach((_, id) => open.set(id, ruled.get(id)));
    let query: Query = { ...NO_QUERY, states: ['undecided'] };
    let place: string | null = null;

    const planWith = (verdicts: ReadonlyMap<string, Verdict>): Plan => buildPlan(withVerdicts(DATA, verdicts), null);
    function expected(): string[] {
      const placed = new Map(ruled);
      for (const [id, was] of open) {
        if (was) placed.set(id, was);
        else placed.delete(id);
      }
      const plan = planWith(placed);
      const graph = graphOf(plan);
      const sections = place === 'product' ? productSections(plan.product)
        : place ? partSections(graph, plan.parts.find((group) => group.part.id === place)!.rows)
          : allSections(plan, graph, DATA.stages, query);
      const kids = (item: Item) => ((item.type === 'feature' && item.foldable) || item.type === 'bundle'
        ? (item.passing?.length ? item.passing : item.kids).map((kid) => underKey(kid.key, item.key)) : []);
      return sections.flatMap((section) => section.items.flatMap((item) => [item.key, ...kids(item)]));
    }
    /** Every fold unfolded and every "N more" pressed, so that the list is all in the document. */
    async function unfolded() {
      for (;;) {
        const next = document.querySelector('.wa-list .wa-caret[aria-expanded="false"], .wa-list .bundle > .wa-hit[aria-expanded="false"], .wa-list .wa-row.more');
        if (!next) return;
        await act(async () => { fireEvent.click(next); });
      }
    }

    const did: string[] = [];
    for (let step = 0; step < 30; step += 1) {
      await unfolded();
      const rows = drawn().filter((id) => id.startsWith('entry-'));
      const toRule = rows.filter((id) => {
        const key = rowKeyOf(id).slice('entry-'.length);
        return open.has(key) && !ruled.has(key) && DATA.entries.find((entry) => entry.id === key)!.verdict === null;
      });
      const roll = random();
      if (toRule.length && roll < 0.4) {
        const at = one(toRule);
        const verdict: Verdict = random() < 0.7 ? 'take' : 'drop';
        await rule(at, verdict, time);
        ruled.set(rowKeyOf(at).slice('entry-'.length), verdict);
        did.push(`${verdict} ${at}`);
      } else if (rows.length && roll < 0.82) {
        const at = one(rows);
        const key = rowKeyOf(at).slice('entry-'.length);
        await press(at);
        const home = DATA.entries.find((entry) => entry.id === key)!.part ?? 'product';
        if (open.has(key)) open.delete(key);
        else {
          // In a part's place, a decision of another part opens where it lives: a move, with that row alone open.
          if (place && home !== place) {
            place = home;
            open.clear();
          }
          open.set(key, ruled.get(key));
        }
        did.push(`press ${at}`);
      } else if (!place && roll < 0.88) {
        fireEvent.click(screen.getByRole('button', { name: 'Filter' }));
        fireEvent.click(screen.getByRole('menuitem', { name: 'State' }));
        fireEvent.click(within(screen.getByRole('menu', { name: 'State' })).getByRole('menuitemcheckbox', { name: /^not decided/ }));
        await act(async () => { fireEvent.keyDown(document.body, { key: 'Escape' }); });
        query = { ...query, states: query.states.length ? [] : ['undecided'] };
        now();
        did.push('filter');
      } else if (!place && roll < 0.91) {
        fireEvent.click(screen.getByRole('button', { name: 'Display' }));
        fireEvent.click(screen.getByRole('menuitemcheckbox', { name: 'Features' }));
        await act(async () => { fireEvent.keyDown(document.body, { key: 'Escape' }); });
        query = { ...query, show: query.show.includes('feature') ? ['decision'] : ['feature', 'decision'] };
        now();
        did.push('display');
      } else if (!place && roll < 0.94) {
        if (!screen.queryByRole('searchbox', { name: 'Search' })) fireEvent.click(screen.getByRole('button', { name: 'Search' }));
        query = { ...query, q: query.q ? '' : 'question' };
        await act(async () => { fireEvent.change(screen.getByRole('searchbox', { name: 'Search' }), { target: { value: query.q } }); });
        now();
        did.push('search');
      } else {
        place = one([null, 'part-a', 'part-c', 'product']);
        query = { ...NO_QUERY, states: random() < 0.6 ? ['undecided'] : [] };
        await act(async () => {
          history.replaceState(null, '', place ? `/workshop#${place}` : `/workshop${formatQuery('', 'all', query)}`);
          dispatchEvent(new PopStateEvent('popstate'));
        });
        open.clear();
        did.push(`go ${place ?? formatQuery('', 'all', query)}`);
      }
      await unfolded();
      const trace = did.join('; ');
      expect(drawn(), trace).toEqual(expected());
      const plan = planWith(ruled);
      const graph = graphOf(plan);
      for (const row of graph.steps) {
        const line = itemAt(row.key);
        if (!line?.querySelector(':scope > .wa-row > .wa-caret')) continue;
        const kids = graph.decisions.get(row.key)!;
        expect(meter(row.key), trace).toBe(`${kids.filter((kid) => kid.status === 'decided').length}/${kids.length}`);
      }
      if (!place && (query.states.length || query.q)) expect(document.querySelector('.wa-count'), trace).toHaveTextContent(`${countOf(graph, query).shown} of ${graph.rows.size}`);
    }
  }, 30_000);
});
