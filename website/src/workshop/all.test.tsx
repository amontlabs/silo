// All: the order and the folding of features and their decisions, each control and what it leaves in the address, the
// old addresses, the relations on the strip and in the list, the films (their line, their marks, their place, their
// frame), search from the map, and what the page remembers.
import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { allSections, formatQuery, graphOf, kidOrder, locate, NO_QUERY, parseQuery } from './all';
import type { Query } from './all';
import { buildData, DECISIONS, FEATURES } from './fixtures';
import { buildPlan } from './plan';
import type { BuildData, Film } from './plan';
import { POINT_MS } from './relation';
import { itemOf, mockApi, part, renderApp, rowButton } from './test-support';

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

  it('numbers every feature by its place in the plan, groups them by stage, and ends with the decisions no feature needs, one row per part', () => {
    const sections = allSections(plan, graph, withNeeds().stages, query());
    expect(sections.map((section) => section.title)).toEqual(['First', 'Second', 'Third', 'Fourth', 'Fifth', 'Sixth', 'Seventh', 'Eighth', 'Ninth', 'Tenth', 'Eleventh', 'Twelfth', 'Gone', 'Other decisions']);
    const steps = sections.flatMap((section) => section.items).flatMap((item) => (item.type === 'feature' ? [item.step] : []));
    expect(steps).toEqual(steps.map((_, index) => index + 1));
    const loose = sections.at(-1)!.items.map((item) => (item.type === 'bundle' ? [item.title, item.kids.map((kid) => kid.key)] : null));
    // one-way, security-properly and tamper-proof-logs are under the feature that needs them, not here.
    expect(loose).toEqual([
      ['Part A', ['entry-core-in-zig']],
      ['Part E', ['entry-who-signs', 'entry-signed-logs', 'entry-old-format']],
      ['The whole product', ['entry-one-host', 'entry-plain-logs']],
    ]);
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
    expect(locate(sections, 'entry-who-signs', new Set())).toEqual({ instance: 'entry-who-signs@bundle-part-e', holder: 'bundle-part-e', more: false });
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

  it('unfolds by the caret and the arrows, shows six then "N more", and opening a row moves none above it', async () => {
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

describe('an open entry', () => {
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

  it('show a film the site cannot play as its title and line, with no frame', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#films/across');
    await renderApp(withNeeds(FILMS.map((film) => ({ ...film, playable: false }))));
    expect(screen.getByRole('heading', { level: 2, name: 'Across the parts' })).toBeInTheDocument();
    expect(document.querySelector('iframe, .wa-player')).toBeNull();
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
