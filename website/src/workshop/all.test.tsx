// All: the order and the folding of features and their decisions, each control and what it leaves in the address, the
// old addresses, the relations on the strip and in the list, the films (their line, their marks, their place, their
// frame), search from the map, and what the page remembers.
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { allSections, formatQuery, graphOf, kidOrder, locate, NO_QUERY, parseQuery, partSections, relationsOf, underKey } from './all';
import type { Query } from './all';
import { App } from './App';
import { buildDataFrom } from './data';
import { buildData, DECISIONS, FEATURES, liveState, planFiles } from './fixtures';
import { NO_MAP } from './map';
import { buildPlan } from './plan';
import type { BuildData, Film } from './plan';
import { POINT_MS } from './relation';
import { itemOf, mockApi, ok, part, renderApp, rowButton } from './test-support';

/** The fixture with a feature that needs three decisions and a feature, and two films. */
function withNeeds(films: Partial<Film>[] = []): BuildData {
  const data = buildData({
    features: [...FEATURES, { id: 'fork-keeps', stage: 'fourth', part: 'part-d', name: 'Keep a fork apart', what: 'Nothing shared.', needs: ['one-way', 'security-properly', 'tamper-proof-logs', 'fork'] }],
    decisions: DECISIONS.map((decision) => (decision.id === 'security-properly' ? decision : decision)),
  });
  return { ...data, films: films.map((film) => ({ id: 'one', title: 'One film', line: 'A line.', shows: [], behind: [], playable: true, ...film })) };
}
const FILMS: Partial<Film>[] = [
  { id: 'across', title: 'Across the parts', shows: ['core', 'one-way', 'create'], behind: ['create'] },
  { id: 'engine-only', title: 'The engine alone', shows: ['core', 'calls'] },
];
const listNames = () => [...document.querySelectorAll('.wa-list .wa-name > span:first-child')].map((name) => name.textContent);
const query = (fields: Partial<Query> = {}): Query => ({ ...NO_QUERY, ...fields });

afterEach(() => {
  vi.useRealTimers();
  localStorage.clear();
});

describe('the list in build order', () => {
  const plan = buildPlan(withNeeds(), null);
  const graph = graphOf(plan);

  it('numbers every feature by its place in the plan and starts each stage with its decisions no feature rests on, one row per part', () => {
    const sections = allSections(plan, graph, withNeeds().stages, query());
    expect(sections.map((section) => section.title)).toEqual(['First', 'Second', 'Third', 'Fourth', 'Fifth', 'Sixth', 'Seventh', 'Eighth', 'Ninth', 'Tenth', 'Eleventh', 'Twelfth', 'Gone']);
    const steps = sections.flatMap((section) => section.items).flatMap((item) => (item.type === 'feature' ? [item.step] : []));
    expect(steps).toEqual(steps.map((_, index) => index + 1));
    const loose = sections.flatMap((section) => section.items.flatMap((item) => (item.type === 'bundle' ? [[section.title, item.title, item.kids.map((kid) => kid.key)]] : [])));
    // one-way, security-properly and tamper-proof-logs are under the feature that rests on them, not here.
    expect(loose).toEqual([
      ['First', 'Part A', ['entry-core-in-zig']],
      ['First', 'The whole product', ['entry-one-host', 'entry-plain-logs']],
      ['Eighth', 'Part E', ['entry-who-signs', 'entry-signed-logs']],
      ['Gone', 'Part E', ['entry-old-format']],
    ]);
    for (const section of sections) {
      const types = section.items.map((item) => item.type);
      expect(types, section.title).toEqual([...types.filter((type) => type === 'bundle'), ...types.filter((type) => type === 'feature')]);
    }
  });

  it('folds under a feature the decisions it needs: those not decided first, then the others, each in the plan\'s order', () => {
    const kids = kidOrder(graph.decisions.get('entry-fork-keeps')!).map((row) => [row.key, row.status]);
    expect(kids).toEqual([['entry-one-way', 'undecided'], ['entry-tamper-proof-logs', 'undecided'], ['entry-security-properly', 'decided']]);
    const item = allSections(plan, graph, withNeeds().stages, query()).flatMap((section) => section.items).find((one) => one.key === 'entry-fork-keeps');
    expect(item).toMatchObject({ type: 'feature', decided: 1, foldable: true, passing: null });
  });

  it('keeps under a filter a feature that passes or one of whose decisions passes, with the passing decisions to unfold', () => {
    const sections = allSections(plan, graph, withNeeds().stages, query({ states: ['decided'], parts: ['part-d'] }));
    const items = sections.flatMap((section) => section.items);
    const keeps = items.find((one) => one.key === 'entry-fork-keeps');
    // fork-keeps is not decided, but one of its decisions of Part D... is not: security-properly is the product's. So it goes.
    expect(keeps).toBeUndefined();
    const undecided = allSections(plan, graph, withNeeds().stages, query({ states: ['undecided'] })).flatMap((section) => section.items);
    const shown = undecided.find((one) => one.key === 'entry-fork-keeps');
    expect(shown).toMatchObject({ type: 'feature' });
    expect(shown?.type === 'feature' && shown.passing?.map((row) => row.key)).toEqual(['entry-one-way', 'entry-tamper-proof-logs']);
  });

  it('lists a search and Latest with every match on its own line', () => {
    const found = allSections(plan, graph, withNeeds().stages, query({ q: 'fork' })).flatMap((section) => section.items);
    expect(found.map((item) => [item.type, item.key])).toEqual([['feature', 'entry-fork'], ['feature', 'entry-diff'], ['feature', 'entry-fork-keeps']]);
    expect(found.every((item) => item.type !== 'feature' || !item.foldable)).toBe(true);
  });

  it('finds where an entry stands: on its own line, or under the first feature that holds it, past its first six when need be', () => {
    const sections = allSections(plan, graph, withNeeds().stages, query());
    expect(locate(sections, 'entry-fork', new Set())).toEqual({ instance: 'entry-fork', holder: null, more: false });
    expect(locate(sections, 'entry-one-way', new Set())).toEqual({ instance: 'entry-one-way@entry-fork-keeps', holder: 'entry-fork-keeps', more: false });
    expect(locate(sections, 'entry-who-signs', new Set())).toEqual({ instance: 'entry-who-signs@bundle-eighth.part-e', holder: 'bundle-eighth.part-e', more: false });
  });
});

/** A small plan of its own: the example map names entries it does not hold, so it has no map. */
const drawn = (options: Parameters<typeof planFiles>[0]) => buildDataFrom(planFiles(options), { map: NO_MAP });

describe('what a feature rests on', () => {
  // top needs end and base-feature; end needs middle, which needs deep, which needs settled. hidden is needed by
  // base-feature and by side, a decision that nothing rests on; other needs side-feature, a feature.
  const data = drawn({
    features: [
      { id: 'base-feature', stage: 'second', part: 'part-b', name: 'A base', what: 'x', needs: ['hidden'] },
      { id: 'top', stage: 'third', part: 'part-a', name: 'A top', what: 'x', needs: ['end', 'base-feature', 'through'] },
    ],
    decisions: [
      { id: 'settled', stage: 'first', part: null, asks: 'Settled?', answer: 'Yes.', verdict: 'take' },
      { id: 'deep', stage: 'first', part: 'part-c', asks: 'Deep?', needs: ['settled'] },
      { id: 'hidden', stage: 'first', part: 'part-a', asks: 'Hidden?' },
      { id: 'early', stage: 'first', part: 'part-c', asks: 'Early?' },
      { id: 'middle', stage: 'second', part: 'part-a', asks: 'Middle?', needs: ['deep'] },
      { id: 'end', stage: 'second', part: 'part-a', asks: 'End?', needs: ['middle', 'deep'] },
      { id: 'side', stage: 'second', part: 'part-c', asks: 'Side?', needs: ['hidden'] },
      { id: 'through', stage: 'third', part: 'part-a', asks: 'Through?' },
    ],
  });
  const plan = buildPlan(data, null);
  const graph = graphOf(plan);
  const keys = (ids: string[]) => ids.map((id) => `entry-${id}`);
  const under = (id: string) => kidOrder(graph.decisions.get(`entry-${id}`)!).map((row) => row.key);

  it('holds a decision reached only through another decision, and none reached only through a needed feature', () => {
    expect(under('top')).toEqual(keys(['deep', 'middle', 'end', 'through', 'settled']));
    expect(under('top')).not.toContain('entry-hidden');
    expect(under('base-feature')).toEqual(keys(['hidden']));
  });

  it('follows no feature that a decision needs', () => {
    const crossed = drawn({
      features: [{ id: 'low', stage: 'first', part: 'part-a', name: 'Low', what: 'x', needs: ['under-low'] }, { id: 'high', stage: 'third', part: 'part-a', name: 'High', what: 'x', needs: ['joins'] }],
      decisions: [{ id: 'under-low', stage: 'first', part: 'part-a', asks: 'Under low?' }, { id: 'joins', stage: 'second', part: 'part-a', asks: 'Joins?', needs: ['low'] }],
    });
    expect(graphOf(buildPlan(crossed, null)).decisions.get('entry-high')!.map((row) => row.key)).toEqual(['entry-joins']);
  });

  it('puts in a fold each decision still to decide after those of them it needs', () => {
    for (const feature of graph.steps) {
      const open = kidOrder(graph.decisions.get(feature.key)!).filter((row) => row.status === 'undecided').map((row) => row.key);
      open.forEach((key, at) => {
        for (const need of graph.needs.get(key)!) {
          if (graph.rows.get(need)!.status === 'undecided') expect(open.slice(0, at), `${key} needs ${need}`).toContain(need);
        }
      });
    }
  });

  it('counts the same set in the meter, the filters and where an entry stands', () => {
    const sections = allSections(plan, graph, data.stages, query({ parts: ['part-c'] }));
    const top = sections.flatMap((section) => section.items).find((item) => item.key === 'entry-top');
    // Only a decision two needs away is of Part C.
    expect(top?.type === 'feature' && [top.decided, top.kids.length, top.passing?.map((row) => row.key)]).toEqual([1, 5, ['entry-deep']]);
    expect(locate(allSections(plan, graph, data.stages, query()), 'entry-deep', new Set())).toEqual({ instance: underKey('entry-deep', 'entry-top'), holder: 'entry-top', more: false });
  });

  it('lists a stage\'s decisions no feature rests on before its features, and gives no section to a stage with neither', () => {
    const listed = allSections(plan, graph, data.stages, query()).map((section) => [section.title, section.items.map((item) => item.key)]);
    expect(listed).toEqual([
      ['First', ['bundle-first.part-c']],
      ['Second', ['bundle-second.part-c', 'entry-base-feature']],
      ['Third', ['entry-top']],
    ]);
  });

  it('is the set a part\'s place lists under its features, and leaves the relations to the direct needs', () => {
    const rows = plan.parts.find((group) => group.part.id === 'part-a')!.rows;
    const sections = partSections(graph, rows);
    expect(sections[0].items[0]).toMatchObject({ key: 'entry-top', decided: 1 });
    expect(sections[1].items.map((item) => item.key)).toEqual(['entry-hidden']);
    expect(relationsOf(graph, 'entry-top').needs).toEqual(keys(['end', 'base-feature', 'through']));
    expect(relationsOf(graph, 'entry-deep').waits).toEqual([]);
  });
});

describe('the address', () => {
  it('holds the view, the order, the filters, what is displayed and the search, and leaves out every default and other parameter alone', () => {
    const asked = query({ order: 'latest', states: ['undecided', 'decided'], parts: ['part-a', 'product'], films: ['across'], show: ['decision', 'film'], q: 'a fork' });
    const text = formatQuery('?signin=failed&view=order', 'all', asked);
    expect(text).toBe('?signin=failed&view=all&order=latest&state=undecided,decided&part=part-a,product&film=across&show=decision,film&q=a+fork');
    expect(parseQuery(text)).toEqual({ view: 'all', order: 'latest', query: asked });
    expect(formatQuery('?view=all&state=dropped', 'map', asked)).toBe('');
    expect(formatQuery('', 'all', NO_QUERY)).toBe('?view=all');
    // The old view of the tiles names no view: the map.
    expect(parseQuery('?view=order').view).toBeNull();
    expect(parseQuery('?view=all&state=nothing,undecided').query.states).toEqual(['undecided']);
  });

  it('follows each control of All: the order, a filter and its chip, Clear, Display, with the count while a filter applies', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop?view=all');
    await renderApp(withNeeds());
    await userEvent.click(screen.getByRole('button', { name: 'Latest' }));
    expect(location.search).toBe('?view=all&order=latest');
    await userEvent.click(screen.getByRole('button', { name: 'Build order' }));
    expect(location.search).toBe('?view=all');
    expect(document.querySelector('.wa-count')).toBeNull();
    await userEvent.click(screen.getByRole('button', { name: 'Filter' }));
    await userEvent.click(screen.getByRole('menuitem', { name: 'State' }));
    await userEvent.click(within(screen.getByRole('menu', { name: 'State' })).getByRole('menuitemcheckbox', { name: /^not decided/ }));
    expect(location.search).toBe('?view=all&state=undecided');
    expect(document.querySelector('.wa-count')).toHaveTextContent(/^\d+ of 31$/);
    // The menu's field lists the values of every kind that match.
    fireEvent.keyDown(document.body, { key: 'Escape' });
    await userEvent.click(screen.getByRole('button', { name: 'Filter' }));
    await userEvent.type(screen.getByRole('textbox', { name: 'Filter' }), 'part d');
    await userEvent.click(screen.getByRole('menuitemcheckbox', { name: /^Part D/ }));
    expect(location.search).toBe('?view=all&state=undecided&part=part-d');
    fireEvent.keyDown(document.body, { key: 'Escape' });
    expect(screen.queryByRole('menu')).toBeNull();
    // A chip per value; its remove button takes it away; with two, Clear takes them all.
    expect(screen.getByRole('button', { name: 'Remove Part D' })).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Clear' }));
    expect(location.search).toBe('?view=all');
    await userEvent.click(screen.getByRole('button', { name: 'Display' }));
    await userEvent.click(screen.getByRole('menuitemcheckbox', { name: 'Features' }));
    expect(location.search).toBe('?view=all&show=decision');
    expect(document.querySelector('.wa-item.f:not(.bundle)')).toBeNull();
    expect(screen.getByRole('button', { name: 'Display' }).querySelector('.wa-dot')).not.toBeNull();
  });

  it('opens the owner\'s address with every feature unfolded to its decisions not decided', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop?view=all&state=undecided');
    await renderApp(withNeeds());
    const feature = itemOf('Keep a fork apart');
    expect(within(feature).getByRole('button', { name: /^Its decisions/ })).toHaveAttribute('aria-expanded', 'true');
    expect([...feature.querySelectorAll('.wa-kids .wa-name > span:first-child')].map((name) => name.textContent)).toEqual(['Which way do changes go?', 'Can a log be changed after the fact?']);
  });

  it('unfolds by the caret and the arrows, shows six then "N more", which gives the focus to the first it reveals, and keeps the order of the rows when one opens', async () => {
    const many = withNeeds();
    const decisions = Array.from({ length: 8 }, (_, i) => ({ id: `q${i}`, stage: 'first', part: 'part-a', asks: `Question ${i}?` }));
    const data = { ...buildData({ features: [...FEATURES, { id: 'asks-much', stage: 'second', part: 'part-a', name: 'Asks much', what: 'x', needs: decisions.map((d) => d.id) }], decisions: [...DECISIONS, ...decisions] }), films: many.films };
    mockApi();
    history.replaceState(null, '', '/workshop?view=all');
    await renderApp(data);
    const feature = itemOf('Asks much');
    const caret = within(feature).getByRole('button', { name: /^Its decisions/ });
    expect(caret).toHaveAttribute('aria-expanded', 'false');
    rowButton('Asks much').focus();
    fireEvent.keyDown(rowButton('Asks much'), { key: 'ArrowRight' });
    expect(caret).toHaveAttribute('aria-expanded', 'true');
    expect(feature.querySelectorAll('.wa-kids > .wa-item')).toHaveLength(6);
    await userEvent.click(within(feature).getByRole('button', { name: '2 more' }));
    expect(feature.querySelectorAll('.wa-kids > .wa-item')).toHaveLength(8);
    expect(document.activeElement).toBe(feature.querySelectorAll('.wa-kids > .wa-item')[6].querySelector('.wa-hit'));
    const ids = () => [...document.querySelectorAll('.wa-list .wa-item')].map((item) => item.id);
    const before = ids();
    await userEvent.click(rowButton('Question 3\\?'));
    expect(rowButton('Question 3\\?')).toHaveAttribute('aria-expanded', 'true');
    expect(ids()).toEqual(before);
    expect(location.hash).toBe('#part-a/q3');
    fireEvent.keyDown(rowButton('Asks much'), { key: 'ArrowLeft' });
    expect(caret).toHaveAttribute('aria-expanded', 'false');
  });

  it.each([
    ['#latest', '?view=all&order=latest', ''],
    ['#left', '?view=all&state=dropped', ''],
    ['#product', '', '#product'],
    ['#part-b', '', '#part-b'],
    ['#part-b/create', '', '#part-b/create'],
  ])('keeps the old address %s working', async (hash, search, kept) => {
    mockApi();
    history.replaceState(null, '', `/workshop${hash}`);
    await renderApp(withNeeds());
    expect(location.search).toBe(search);
    expect(location.hash).toBe(kept);
    if (kept === '#part-b/create') await waitFor(() => expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'true'));
  });

  it('opens a row in All where it stands, from its address with the view', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop?view=all#part-d/one-way');
    await renderApp(withNeeds());
    await waitFor(() => expect(rowButton('Which way do changes go?')).toHaveAttribute('aria-expanded', 'true'));
    expect(rowButton('Which way do changes go?').closest('.wa-item')).toHaveAttribute('id', 'entry-one-way@entry-fork-keeps');
    expect(document.querySelector('.ws-detail')).toBeNull();
  });
});

describe('a link to an entry', () => {
  it.each([
    ['decisions not displayed', '?view=all&show=feature#product/one-host', 'Product', 'Which host runs Silo first?', '#product/one-host'],
    ['a search that leaves it out', '?view=all&q=nothing-like-it#part-b/create', 'Part B', 'Create and delete a computer', '#part-b/create'],
    ['Latest, for an entry with no history', '?view=all&order=latest#part-a/core', 'Part A', 'The engine runs on its own', '#part-a/core'],
    ['a filter that leaves it out', '?view=all&state=dropped#part-d/fork', 'Part D', 'Fork a computer', '#part-d/fork'],
  ])('opens it in its own place when All does not hold it: %s', async (_, link, place, name, hash) => {
    mockApi();
    history.replaceState(null, '', `/workshop${link}`);
    await renderApp(withNeeds());
    expect(screen.getByRole('heading', { level: 2, name: place })).toBeInTheDocument();
    await waitFor(() => expect(rowButton(name.replace('?', '\\?'))).toHaveAttribute('aria-expanded', 'true'));
    expect(location.search).toBe('');
    expect(location.hash).toBe(hash);
  });

  it('opens a suggestion named in All where it lives', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop?view=all#issue-30');
    await renderApp(withNeeds());
    await waitFor(() => expect(rowButton('Start a task from Slack')).toHaveAttribute('aria-expanded', 'true'));
    expect(location.hash).toBe('#suggested/issue-30');
  });
});

describe('the address is the truth', () => {
  it('shows Build order for an address with no order, read from the history, whatever the page last showed or remembers', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop?view=all');
    await renderApp(withNeeds());
    await userEvent.click(screen.getByRole('button', { name: 'Latest' }));
    expect(location.search).toBe('?view=all&order=latest');
    act(() => { history.replaceState(null, '', '/workshop?view=all'); dispatchEvent(new PopStateEvent('popstate')); });
    expect(screen.getByRole('button', { name: 'Build order' })).toHaveAttribute('aria-pressed', 'true');
    expect(location.search).toBe('?view=all');
    // Pressing All shows the order used last, and writes it.
    await userEvent.click(screen.getByRole('button', { name: /^Map/ }));
    await userEvent.click(screen.getByRole('button', { name: 'All' }));
    expect(screen.getByRole('button', { name: 'Latest' })).toHaveAttribute('aria-pressed', 'true');
    expect(location.search).toBe('?view=all&order=latest');
  });
});

describe('an address from the history', () => {
  it('is never rewritten when Back or Forward across fragments fires popstate, then hashchange', async () => {
    mockApi();
    await renderApp(withNeeds());
    act(() => {
      history.replaceState(null, '', '/workshop#left/exec');
      dispatchEvent(new PopStateEvent('popstate'));
      dispatchEvent(new HashChangeEvent('hashchange'));
    });
    await waitFor(() => expect(rowButton('Run a command')).toHaveAttribute('aria-expanded', 'true'));
    expect(location.hash).toBe('#left/exec');
    // The same old address followed by hand is written in its own form.
    act(() => {
      history.pushState(null, '', '/workshop#left/create');
      dispatchEvent(new HashChangeEvent('hashchange'));
    });
    expect(location.hash).toBe('#part-b/create');
  });

  it('is never rewritten when what it names arrives late', async () => {
    let release: (answer: { status: number; body: unknown }) => void = () => {};
    mockApi({ state: () => new Promise((resolve) => { release = resolve; }) });
    history.replaceState(null, '', '/workshop');
    render(<App data={withNeeds()} />);
    act(() => {
      history.replaceState(null, '', '/workshop#issue-30');
      dispatchEvent(new PopStateEvent('popstate'));
    });
    await act(async () => release(ok(liveState())));
    await waitFor(() => expect(rowButton('Start a task from Slack')).toHaveAttribute('aria-expanded', 'true'));
    expect(location.hash).toBe('#issue-30');
  });
});

describe('an open entry', () => {
  it('stays open and named when a search typed in its part takes the person to All and the results hold it', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#part-d/fork');
    await renderApp(withNeeds());
    await waitFor(() => expect(rowButton('Fork a computer')).toHaveAttribute('aria-expanded', 'true'));
    await userEvent.click(screen.getByRole('button', { name: 'Search' }));
    await userEvent.type(screen.getByRole('searchbox', { name: 'Search' }), 'fork');
    expect(document.querySelector('.ws')).toHaveClass('v-all');
    expect(rowButton('Fork a computer')).toHaveAttribute('aria-expanded', 'true');
    expect(location.search).toBe('?view=all&q=fork');
    expect(location.hash).toBe('#part-d/fork');
  });

  it('is not named when a search typed in its part takes the person to results that do not hold it', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#part-d/fork');
    await renderApp(withNeeds());
    await waitFor(() => expect(rowButton('Fork a computer')).toHaveAttribute('aria-expanded', 'true'));
    await userEvent.click(screen.getByRole('button', { name: 'Search' }));
    await userEvent.type(screen.getByRole('searchbox', { name: 'Search' }), 'z');
    expect(location.hash).toBe('');
  });

  it('stays drawn, its feature unfolded and past the first six, through a change of filter, and is named until the list no longer holds it', async () => {
    const decisions = Array.from({ length: 8 }, (_, i) => ({ id: `q${i}`, stage: 'first', part: 'part-a', asks: `Question ${i}?` }));
    const data = { ...buildData({ features: [...FEATURES, { id: 'asks-much', stage: 'second', part: 'part-a', name: 'Asks much', what: 'x', needs: decisions.map((d) => d.id) }], decisions: [...DECISIONS, ...decisions] }), films: [] };
    mockApi();
    history.replaceState(null, '', '/workshop?view=all#part-a/q7');
    await renderApp(data);
    await waitFor(() => expect(rowButton('Question 7\\?')).toHaveAttribute('aria-expanded', 'true'));
    await userEvent.click(screen.getByRole('button', { name: 'Filter' }));
    await userEvent.click(screen.getByRole('menuitem', { name: 'Part' }));
    await userEvent.click(within(screen.getByRole('menu', { name: 'Part' })).getByRole('menuitemcheckbox', { name: /^Part A/ }));
    expect(location.search).toBe('?view=all&part=part-a');
    expect(rowButton('Question 7\\?')).toHaveAttribute('aria-expanded', 'true');
    expect(rowButton('Question 7\\?').closest('.wa-item')).toHaveAttribute('id', 'entry-q7@entry-asks-much');
    expect(location.hash).toBe('#part-a/q7');
    // A filter that leaves it out: the address stops naming it.
    await userEvent.click(within(screen.getByRole('menu', { name: 'Part' })).getByRole('menuitemcheckbox', { name: /^Part A/ }));
    await userEvent.click(within(screen.getByRole('menu', { name: 'Part' })).getByRole('menuitemcheckbox', { name: /^Part B/ }));
    expect(screen.queryByRole('button', { name: /^Question 7/ })).toBeNull();
    expect(location.hash).toBe('');
  });

  it('stays open when a search shows it on a line of its own, and the address stops naming it when a filter removes it', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop?view=all#part-d/one-way');
    await renderApp(withNeeds());
    await waitFor(() => expect(rowButton('Which way do changes go\\?')).toHaveAttribute('aria-expanded', 'true'));
    expect(rowButton('Which way do changes go\\?').closest('.wa-item')).toHaveAttribute('id', 'entry-one-way@entry-fork-keeps');
    await userEvent.click(screen.getByRole('button', { name: 'Search' }));
    await userEvent.type(screen.getByRole('searchbox', { name: 'Search' }), 'way do');
    expect(rowButton('Which way do changes go\\?').closest('.wa-item')).toHaveAttribute('id', 'entry-one-way');
    expect(rowButton('Which way do changes go\\?')).toHaveAttribute('aria-expanded', 'true');
    expect(location.hash).toBe('#part-d/one-way');
    await userEvent.type(screen.getByRole('searchbox', { name: 'Search' }), 'zz');
    expect(screen.queryByRole('button', { name: /^Which way do changes go/ })).toBeNull();
    expect(location.hash).toBe('');
  });
});

describe('relations', () => {
  it('ring a row\'s pip on the strip, ink what it needs and what waits for it, and light those rows in the list, after a rest of the pointer', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop?view=all');
    await renderApp(withNeeds());
    vi.useFakeTimers();
    const row = rowButton('Fork a computer').closest('.wa-row') as HTMLElement;
    fireEvent.pointerOver(row);
    expect(document.querySelector('.wa-sp.cur')).toBeNull();
    await act(async () => { await vi.advanceTimersByTimeAsync(POINT_MS + 10); });
    expect(document.querySelector('.wa-sp.cur')).toHaveAttribute('data-row', 'entry-fork');
    // fork-keeps needs fork: it waits for it.
    expect([...document.querySelectorAll('.wa-sp.rel')].map((pip) => pip.getAttribute('data-row'))).toEqual(['entry-fork-keeps']);
    expect(document.querySelector('.wa-strip')).toHaveClass('relating');
    expect([...document.querySelectorAll('.wa-row.lit')].map((lit) => lit.getAttribute('data-row'))).toEqual(['entry-fork-keeps']);
    expect(document.querySelectorAll('.wa-rail path, .wa-rail circle').length).toBeGreaterThan(0);
    fireEvent.pointerLeave(row.closest('.wa-list') as HTMLElement);
    expect(document.querySelector('.wa-sp.cur, .wa-row.lit')).toBeNull();
    vi.useRealTimers();
  });

  it('for a decision, light the features that need it; an opened row keeps its marks while nothing is pointed at', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop?view=all#part-d/one-way');
    await renderApp(withNeeds());
    await waitFor(() => expect(rowButton('Which way do changes go?')).toHaveAttribute('aria-expanded', 'true'));
    await waitFor(() => expect([...document.querySelectorAll('.wa-sp.rel')].map((pip) => pip.getAttribute('data-row'))).toEqual(['entry-fork-keeps']));
    expect([...document.querySelectorAll('.wa-row.lit')].map((lit) => lit.getAttribute('data-row'))).toEqual(['entry-fork-keeps']);
  });

  it('let the keyboard go on from a row pressed on its name', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop?view=all');
    await renderApp(withNeeds());
    fireEvent.click(itemOf('Fork a computer').querySelector('.wa-name > span') as HTMLElement);
    expect(document.activeElement).toBe(rowButton('Fork a computer'));
    fireEvent.keyDown(document.activeElement as HTMLElement, { key: 'ArrowDown' });
    expect(document.activeElement).toBe(rowButton('Save a checkpoint'));
  });

  it('open the feature of a dot the list does not hold in its part\'s place', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop?view=all&part=part-a');
    await renderApp(withNeeds());
    const dot = document.querySelector<HTMLButtonElement>('.wa-sp[data-row="entry-fork"]') as HTMLButtonElement;
    expect(dot).toHaveClass('out');
    await userEvent.click(dot);
    expect(screen.getByRole('heading', { level: 2, name: 'Part D' })).toBeInTheDocument();
    await waitFor(() => expect(rowButton('Fork a computer')).toHaveAttribute('aria-expanded', 'true'));
    expect(location.hash).toBe('#part-d/fork');
  });

  it('name each dot of the relation drawn by its side of it, and light every drawing of a related decision', async () => {
    const shared = buildData({
      features: [...FEATURES, { id: 'fork-keeps', stage: 'fourth', part: 'part-d', name: 'Keep a fork apart', what: 'x', needs: ['one-way', 'fork'] }, { id: 'fork-later', stage: 'fifth', part: 'part-d', name: 'Fork later', what: 'y', needs: ['one-way', 'fork-keeps'] }],
    });
    mockApi();
    history.replaceState(null, '', '/workshop?view=all&state=undecided');
    await renderApp({ ...shared, films: [] });
    vi.useFakeTimers();
    // Both features show "Which way do changes go?" unfolded; the decision's relations reach both.
    const keeps = rowButton('Keep a fork apart').closest('.wa-row') as HTMLElement;
    fireEvent.pointerOver(keeps);
    await act(async () => { await vi.advanceTimersByTimeAsync(POINT_MS + 10); });
    expect(document.querySelector('.wa-sp[data-row="entry-fork"]')).toHaveAccessibleName(/, needed$/);
    expect(document.querySelector('.wa-sp[data-row="entry-fork-later"]')).toHaveAccessibleName(/, waiting$/);
    expect(document.querySelector('.wa-sp[data-row="entry-core"]')).toHaveAccessibleName(/not decided$|decided$/);
    expect([...document.querySelectorAll('.wa-row.lit[data-row="entry-one-way"]')]).toHaveLength(2);
    fireEvent.pointerLeave(keeps.closest('.wa-list') as HTMLElement);
    expect(document.querySelector('.wa-sp[data-row="entry-fork"]')).not.toHaveAccessibleName(/needed/);
    vi.useRealTimers();
  });

  it('give each pip of the strip a target and one stop for the keyboard, the arrows moving along it, a press bringing its row', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop?view=all');
    await renderApp(withNeeds());
    const pips = [...document.querySelectorAll<HTMLButtonElement>('.wa-sp')];
    expect(pips).toHaveLength(22);
    expect(pips.filter((pip) => pip.tabIndex === 0)).toHaveLength(1);
    expect(pips[0]).toHaveAccessibleName('1 How research enters the plan, not decided');
    pips[0].focus();
    fireEvent.keyDown(pips[0], { key: 'ArrowRight' });
    expect(document.activeElement).toBe(pips[1]);
    expect(pips[1].tabIndex).toBe(0);
    await userEvent.click(pips[1]);
    expect(document.activeElement).toBe(rowButton('The engine runs on its own'));
  });

  it('are not written in an opened row', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop?view=all#part-d/fork-keeps');
    await renderApp(withNeeds());
    await waitFor(() => expect(rowButton('Keep a fork apart')).toHaveAttribute('aria-expanded', 'true'));
    expect(within(itemOf('Keep a fork apart')).queryByText('Needs')).toBeNull();
  });
});

describe('the films', () => {
  it('stand on their line above the map, in their order; pointing at one marks its parts on the map', async () => {
    mockApi();
    await renderApp(withNeeds(FILMS));
    const line = screen.getByRole('toolbar', { name: 'Films' });
    expect(within(line).getAllByRole('button').map((button) => button.textContent)).toEqual(['Across the parts', 'The engine alone']);
    fireEvent.pointerEnter(within(line).getByRole('button', { name: /^The engine alone/ }));
    expect(part('part-a')).toHaveClass('marked');
    expect(part('part-b')).toHaveClass('faint');
    fireEvent.pointerLeave(within(line).getByRole('button', { name: /^The engine alone/ }));
    expect(part('part-a')).not.toHaveClass('marked');
  });

  it('dash the frame of a film behind the plan, on its control and on the mark of the entry it is behind on', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#part-b');
    await renderApp(withNeeds(FILMS));
    expect(screen.getByRole('button', { name: /^Across the parts/ })).toHaveClass('behind');
    expect(screen.getByRole('button', { name: /^The engine alone/ })).not.toHaveClass('behind');
    expect(within(itemOf('Create and delete a computer')).getByRole('img', { name: 'Film is behind the plan' })).toHaveClass('wa-film', 'behind');
    expect(within(itemOf('Run a command')).queryByRole('img', { name: /film/i })).toBeNull();
  });

  it('open as a place: pressed, the map folds with the film\'s parts marked, the frame loads, and its entries follow', async () => {
    mockApi();
    await renderApp(withNeeds(FILMS));
    await userEvent.click(screen.getByRole('button', { name: /^Across the parts/ }));
    expect(location.hash).toBe('#films/across');
    expect(screen.getByRole('button', { name: /^Across the parts/ })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByRole('heading', { level: 2, name: 'Across the parts' })).toHaveFocus();
    expect(document.querySelector('.ws-stage')).toHaveClass('fold');
    expect(['part-a', 'part-b', 'part-d'].map((id) => part(id).classList.contains('marked'))).toEqual([true, true, true]);
    expect(part('part-e')).toHaveClass('faint');
    const frame = document.querySelector<HTMLIFrameElement>('.wa-player iframe');
    expect(frame).toHaveAttribute('src', '/films/across/?in=workshop');
    expect(frame?.closest('.wa-poster')).toHaveAttribute('aria-busy', 'true');
    expect(document.querySelector('.wa-player .wa-ph-film')).not.toBeNull();
    fireEvent.load(frame as HTMLIFrameElement);
    expect(document.querySelector('.wa-player .wa-ph-film')).toBeNull();
    expect(listNames()).toEqual(['The engine runs on its own', 'Create and delete a computer', 'Which way do changes go?']);
    // Back gives the focus to its control on the line.
    await userEvent.click(screen.getByRole('button', { name: /← Full map/ }));
    await waitFor(() => expect(location.hash).toBe(''));
    await waitFor(() => expect(screen.getByRole('button', { name: /^Across the parts/ })).toHaveFocus());
  });

  it('take a height only from the frame they made, on the site\'s origin, for their own film', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#films/across');
    await renderApp(withNeeds(FILMS));
    const frame = document.querySelector<HTMLIFrameElement>('.wa-player iframe') as HTMLIFrameElement;
    const box = frame.closest('.wa-poster') as HTMLElement;
    const tell = (data: unknown, source: MessageEventSource | null = frame.contentWindow, origin = location.origin) => act(() => { dispatchEvent(new MessageEvent('message', { data, source, origin })); });
    tell({ film: '/films/across/', height: 500 }, window);
    tell({ film: '/films/across/', height: 500 }, frame.contentWindow, 'https://elsewhere.example');
    tell({ film: '/films/engine-only/', height: 500 });
    expect(box.style.height).toBe('');
    tell({ film: '/films/across/', height: 500 });
    expect(box.style.height).toBe('500px');
  });

  it('keep the control of a film of one part in that part\'s place', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#part-a');
    await renderApp(withNeeds(FILMS));
    const block = document.querySelector('.ws-block') as HTMLElement;
    await userEvent.click(within(block).getByRole('button', { name: /^The engine alone/ }));
    expect(location.hash).toBe('#films/engine-only');
  });

  it('show first in an opened row, with a frame that loads only once the row is open', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#part-a');
    await renderApp(withNeeds(FILMS));
    expect(document.querySelector('iframe')).toBeNull();
    expect(within(itemOf('The engine runs on its own')).getByRole('img', { name: 'Has a film' })).toBeInTheDocument();
    await userEvent.click(rowButton('The engine runs on its own'));
    const box = itemOf('The engine runs on its own').querySelector('.wa-filmbox') as HTMLElement;
    expect(box.querySelector('iframe')).toHaveAttribute('src', '/films/across/?in=workshop');
    expect(within(box).getByText('Across the parts')).toBeInTheDocument();
    // Its count of entries lists them in All.
    await userEvent.click(within(box).getByRole('button', { name: '3 entries' }));
    expect(location.search).toBe('?view=all&film=across');
    // Its entries, and the feature that holds one of them, unfolded to it.
    expect(listNames()).toEqual(['The engine runs on its own', 'Create and delete a computer', 'Keep a fork apart', 'Which way do changes go?']);
  });

  it('take their frame away once the row is closed', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#part-a/core');
    await renderApp(withNeeds(FILMS));
    await waitFor(() => expect(itemOf('The engine runs on its own').querySelector('.wa-filmbox iframe')).not.toBeNull());
    await userEvent.click(rowButton('The engine runs on its own'));
    expect(rowButton('The engine runs on its own')).toHaveAttribute('aria-expanded', 'false');
    expect(document.querySelector('iframe')).toBeNull();
  });

  it('show a film the site cannot play as its title and line, with no frame', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#films/across');
    await renderApp(withNeeds(FILMS.map((film) => ({ ...film, playable: false }))));
    expect(screen.getByRole('heading', { level: 2, name: 'Across the parts' })).toBeInTheDocument();
    expect(document.querySelector('iframe, .wa-player')).toBeNull();
  });
});

describe('the keyboard\'s focus', () => {
  it('goes back to the search button on Escape, to a chip from its menu, to the next chip or Filter when one goes, and to Filter after Clear', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop?view=all&state=undecided,decided&part=part-a');
    await renderApp(withNeeds());
    await userEvent.click(screen.getByRole('button', { name: 'Search' }));
    await userEvent.keyboard('{Escape}');
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Search' }));
    const value = (text: string) => [...document.querySelectorAll<HTMLElement>('.wa-chip > button:first-child')].find((button) => button.textContent === text) as HTMLElement;
    await userEvent.click(value('decided'));
    expect(screen.getByRole('menu', { name: 'State' })).toBeInTheDocument();
    await userEvent.keyboard('{Escape}');
    expect(document.activeElement).toBe(value('decided'));
    await userEvent.click(screen.getByRole('button', { name: 'Remove decided' }));
    expect(document.activeElement).toBe(value('Part A'));
    await userEvent.click(screen.getByRole('button', { name: 'Remove Part A' }));
    expect(document.activeElement).toBe(value('not decided'));
    await userEvent.click(screen.getByRole('button', { name: 'Remove not decided' }));
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Filter' }));
    history.replaceState(null, '', '/workshop?view=all&state=undecided&part=part-a');
    act(() => { dispatchEvent(new PopStateEvent('popstate')); });
    await userEvent.click(document.querySelector('.wa-clear') as HTMLElement);
    expect(document.activeElement).toBe(screen.getByRole('button', { name: 'Filter' }));
  });
});

describe('a chip\'s menu', () => {
  it('remembers the chip that opened it through typing and another submenu, and gives it the focus back', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop?view=all&state=undecided&part=part-a');
    await renderApp(withNeeds());
    const value = (text: string) => [...document.querySelectorAll<HTMLElement>('.wa-chip > button:first-child')].find((button) => button.textContent === text) as HTMLElement;
    await userEvent.click(value('Part A'));
    await userEvent.type(screen.getByRole('textbox', { name: 'Filter' }), 'part');
    await userEvent.clear(screen.getByRole('textbox', { name: 'Filter' }));
    fireEvent.pointerEnter(screen.getByRole('menuitem', { name: 'Film' }));
    fireEvent.pointerEnter(screen.getByRole('menuitem', { name: 'State' }));
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('menu')).toBeNull();
    expect(document.activeElement).toBe(value('Part A'));
  });
});

describe('search', () => {
  it('opens from the map with its key, and typing shows All with the matches', async () => {
    mockApi();
    await renderApp(withNeeds());
    fireEvent.keyDown(document.body, { key: '/' });
    const field = await screen.findByRole('searchbox', { name: 'Search' });
    expect(field).toHaveFocus();
    await userEvent.type(field, 'fork');
    expect(location.search).toBe('?view=all&q=fork');
    expect(listNames()).toEqual(['Fork a computer', 'See what a fork changed', 'Keep a fork apart']);
    expect(document.querySelector('.wa-count')).toHaveTextContent('3 of 31');
    await userEvent.clear(field);
    await userEvent.type(field, 'telemetry');
    expect(screen.getByText('Nothing matches.')).toBeInTheDocument();
    await userEvent.click(within(document.querySelector('.wa-none') as HTMLElement).getByRole('button', { name: 'Clear' }));
    expect(location.search).toBe('?view=all');
  });
});

describe('what the page remembers', () => {
  it('shows a plain address in the view and the order used last, and an address that names a view as it says', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop?view=all');
    const first = await renderApp(withNeeds());
    await userEvent.click(screen.getByRole('button', { name: 'Latest' }));
    first.unmount();
    history.replaceState(null, '', '/workshop');
    const second = await renderApp(withNeeds());
    expect(screen.getByRole('button', { name: 'All' })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByRole('button', { name: 'Latest' })).toHaveAttribute('aria-pressed', 'true');
    expect(location.search).toBe('?view=all&order=latest');
    // The owner's address shows its own order.
    second.unmount();
    history.replaceState(null, '', '/workshop?view=all&state=undecided');
    const third = await renderApp(withNeeds());
    expect(screen.getByRole('button', { name: 'Build order' })).toHaveAttribute('aria-pressed', 'true');
    third.unmount();
    // A part's address shows the part, on the map's side.
    history.replaceState(null, '', '/workshop#part-b');
    const fourth = await renderApp(withNeeds());
    expect(screen.getByRole('heading', { level: 2, name: 'Part B' })).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: /^Map/ }));
    fourth.unmount();
    history.replaceState(null, '', '/workshop');
    await renderApp(withNeeds());
    expect(screen.getByRole('button', { name: /^Map/ })).toHaveAttribute('aria-pressed', 'true');
    expect(location.search).toBe('');
  });
});
