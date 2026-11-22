import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { App } from './App';
import { buildData, FEATURES, issue, liveState } from './fixtures';
import type { LiveState } from './plan';
import {
  partTitle, detail, itemOf, mockApi, ok, owner, part, refused, renderApp, rowButton, tile,
} from './test-support';
import type { Route } from './test-support';

// `core`, `calls` and `create` are built and `fork` is taken, as their files say; `shells` is being built, or with
// `started` false only taken.
const builtData = (started = true) => buildData({ features: FEATURES.map((feature) => (['core', 'calls', 'create'].includes(feature.id) ? { ...feature, verdict: 'take' as const, built: '2026-11-16' } : feature.id === 'fork' ? { ...feature, verdict: 'take' as const } : feature.id === 'shells' && !started ? { ...feature, started: undefined } : feature)) });

describe('the map', () => {
  it('shows the title, one line, every part with its state and only the states present', async () => {
    mockApi();
    await renderApp();
    expect(screen.getByRole('heading', { level: 1, name: 'Workshop' })).toBeInTheDocument();
    expect(screen.getByText('Part C is being built.')).toBeInTheDocument();
    expect(document.querySelectorAll('.ws-part')).toHaveLength(5);
    expect(part('part-a')).toHaveClass('s-next');
    expect(part('part-a')).toHaveTextContent('next');
    expect(part('part-c')).toHaveClass('s-building');
    expect(part('part-c')).toHaveTextContent('being built');
    expect(part('part-b')).toHaveClass('s-open');
    expect(part('part-a')).toHaveAccessibleName('Part A, next, 2 features, 1 decision. A tall part with a glyph.');
    expect(part('part-b')).toHaveAccessibleName('Part B, not decided, 2 features. A part that holds another.');
    expect(part('part-c')).toHaveAccessibleName('Part C, being built, 1 feature. A part inside Part B.');
    expect(part('part-d')).toHaveAccessibleName('Part D, not decided, 7 features, 2 decisions. A part with copies behind it.');
    expect(tile('part-c').parentElement).toHaveClass('s-building');
    expect(tile('part-c')).toHaveTextContent('being built');
    expect(tile('part-c')).toHaveAccessibleName('Part C, being built, 1 feature. A part inside Part B.');
    // The feature being built has its own mark, in its part's dots.
    expect([...tile('part-c').querySelectorAll('.ws-pips i')].map((pip) => pip.className)).toEqual(['ws-pip building']);
  });

  it('shows built and next parts, on the map and the tiles alike', async () => {
    mockApi();
    await renderApp(builtData());
    expect(screen.getByText('Part C is being built.')).toBeInTheDocument();
    expect(part('part-a')).toHaveClass('s-built');
    expect(part('part-a')).toHaveTextContent('built');
    expect(part('part-b')).toHaveClass('s-built');
    expect(part('part-d')).toHaveClass('s-next');
    for (const [id, state] of [['part-a', 's-built'], ['part-b', 's-built'], ['part-d', 's-next'], ['part-c', 's-building'], ['part-e', 's-open']]) {
      expect(tile(id).parentElement).toHaveClass(state);
    }
    expect(tile('part-a')).toHaveTextContent('built');
  });

  it('shows a decided part, and the line says what is next, when no part is being built', async () => {
    mockApi();
    await renderApp(builtData(false));
    expect(screen.getByText('Part D is next.')).toBeInTheDocument();
    expect(part('part-c')).toHaveClass('s-decided');
    expect(part('part-c')).toHaveTextContent('1 decided');
    expect(part('part-c')).toHaveAccessibleName('Part C, 1 decided, 1 feature. A part inside Part B.');
    expect(tile('part-c').parentElement).toHaveClass('s-decided');
    expect(tile('part-c')).toHaveTextContent('1 decided');
  });

  it('switches to All and back, a move Back undoes, and keeps the tiles for narrow windows', async () => {
    mockApi();
    await renderApp();
    expect(screen.getByRole('button', { name: /^Map/ })).toHaveAttribute('aria-pressed', 'true');
    await userEvent.click(screen.getByRole('button', { name: 'All' }));
    expect(screen.getByRole('button', { name: 'All' })).toHaveAttribute('aria-pressed', 'true');
    expect(location.search).toBe('?view=all');
    expect(document.querySelector('.ws')).toHaveClass('v-all', 'is-all');
    expect(document.querySelector('.ws-stage, .wa-films')).toBeNull();
    expect(screen.getByRole('toolbar', { name: 'What the list shows' })).toBeInTheDocument();
    act(() => history.back());
    await waitFor(() => expect(screen.getByRole('button', { name: /^Map/ })).toHaveAttribute('aria-pressed', 'true'));
    expect(location.search).toBe('');
    // The tiles stand in the map's place under 1200 px, and have no view of their own.
    const order = [...document.querySelectorAll<HTMLElement>('.ws-tile')].map((t) => t.dataset.part);
    expect(order.slice(0, 4)).toEqual(['part-a', 'part-b', 'part-c', 'part-d']);
    expect(screen.queryByRole('button', { name: 'Order' })).toBeNull();
    // The old address of the tiles shows the map.
    history.replaceState(null, '', '/workshop?view=order');
    act(() => { dispatchEvent(new PopStateEvent('popstate')); });
    expect(screen.getByRole('button', { name: /^Map/ })).toHaveAttribute('aria-pressed', 'true');
  });

  it('holds Product, Search and the suggestions in the title row, then the switch, on every place', async () => {
    mockApi();
    await renderApp();
    const row = () => screen.getByRole('navigation', { name: 'Places' });
    const links = () => within(row()).getAllByRole('link').map((a) => [a.textContent, a.getAttribute('href'), a.getAttribute('aria-current')]);
    expect(links()).toEqual([['Product', '#product', null], ['Suggest a feature', '#suggested', null]]);
    expect(within(row()).getByRole('button', { name: 'Search' })).toBeInTheDocument();
    expect(row().closest('.ws-tools')?.lastElementChild).toHaveAttribute('aria-label', 'View');
    await userEvent.click(part('part-b'));
    expect(links().map(([name]) => name)).toEqual(['Product', 'Suggest a feature']);
    await userEvent.click(screen.getByRole('link', { name: 'Product' }));
    expect(location.hash).toBe('#product');
    expect(links()).toEqual([['Product', '#product', 'true'], ['Suggest a feature', '#suggested', null]]);
    // The open one, pressed again, goes back to the plain view.
    await userEvent.click(screen.getByRole('link', { name: 'Product' }));
    await waitFor(() => expect(location.hash).toBe(''));
    expect(screen.getByRole('link', { name: 'Product' })).not.toHaveAttribute('aria-current');
  });
});

describe('a part', () => {
  it('opens from its part: the map folds, the part and its entries show, the address and focus follow', async () => {
    mockApi();
    await renderApp();
    await userEvent.click(part('part-b'));
    expect(location.hash).toBe('#part-b');
    expect(document.querySelector('.ws-stage')).toHaveClass('fold');
    expect(part('part-b')).toHaveAttribute('aria-current', 'true');
    expect(screen.getByRole('heading', { level: 2, name: 'Part B' })).toHaveFocus();
    expect(within(document.querySelector('.ws-detail') as HTMLElement).getByText('A part that holds another.')).toBeInTheDocument();
    expect(screen.getByText('2 not decided')).toBeInTheDocument();
    expect(rowButton('Create and delete a computer')).toBeInTheDocument();
    expect(rowButton('Run a command')).toBeInTheDocument();
    expect(screen.getByText('Part B, open')).toBeInTheDocument();
  });

  it('switches to another part, goes back with the button, Escape and Back, and returns focus', async () => {
    mockApi();
    await renderApp();
    await userEvent.click(part('part-b'));
    await userEvent.click(part('part-d'));
    expect(location.hash).toBe('#part-d');
    expect(screen.getByRole('heading', { level: 2, name: 'Part D' })).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: /Full map/ }));
    expect(location.hash).toBe('');
    expect(document.querySelector('.ws-stage')).not.toHaveClass('fold');
    expect(part('part-d')).toHaveFocus();

    await userEvent.click(part('part-e'));
    fireEvent.keyDown(document.activeElement as Element, { key: 'Escape' });
    expect(partTitle()).toBeNull();
    expect(part('part-e')).toHaveFocus();

    await userEvent.click(part('part-a'));
    act(() => history.back());
    await waitFor(() => expect(partTitle()).toBeNull());
    act(() => history.forward());
    await waitFor(() => expect(screen.getByRole('heading', { level: 2, name: 'Part A' })).toBeInTheDocument());
  });

  it('opens from a tile too, and the same tile closes it', async () => {
    mockApi();
    await renderApp();
    await userEvent.click(tile('part-e'));
    expect(location.hash).toBe('#part-e');
    expect(tile('part-e')).toHaveAttribute('aria-current', 'true');
    await userEvent.click(tile('part-e'));
    expect(location.hash).toBe('');
  });

  it('shows each entry\'s state by its pip and its votes as a count; the thumb and the comments are in the opened row', async () => {
    history.replaceState(null, '', '/workshop#part-a');
    mockApi();
    await renderApp();
    expect(within(itemOf('The engine runs on its own')).getByRole('img', { name: 'decided' })).toBeInTheDocument();
    expect(itemOf('The engine runs on its own').querySelector(':scope > .wa-row > .wa-side')).toHaveTextContent('👍 8');
    expect(within(itemOf('The engine runs on its own')).queryByRole('link', { name: /votes/ })).toBeNull();
    act(() => { location.hash = '#part-b/create'; });
    await waitFor(() => expect(within(itemOf('Create and delete a computer')).getByText('💬 2')).toBeInTheDocument());
    expect(within(itemOf('Create and delete a computer')).getByRole('link', { name: '12 votes, sign in to vote' })).toHaveTextContent('👍12');
    expect(itemOf('Run a command').querySelector(':scope > .wa-row > .wa-side')).toHaveTextContent('👍 4');
  });
});

describe('an entry', () => {
  it('opens in place, loads its comments and is linked', async () => {
    const calls = mockApi({ routes: { 'GET issue': (url) => ok(detail(Number(url.searchParams.get('n')), { comments: [{ id: 1, author: 'mara-k', avatar: '', bot: false, createdAt: '2026-11-14T10:00:00Z', text: 'Keep stop separate from pause.' }] })) } });
    history.replaceState(null, '', '/workshop#part-b');
    await renderApp();
    await userEvent.click(rowButton('Create and delete a computer'));
    expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByText('Make a Linux computer.')).toBeInTheDocument();
    expect(screen.getByText('Not decided.')).toBeInTheDocument();
    expect(await screen.findByText('Keep stop separate from pause.')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'or comment directly on GitHub' })).toHaveAttribute('href', 'https://github.com/amontlabs/silo/issues/12');
    expect(calls.some((call) => call.path === 'issue' && call.method === 'GET')).toBe(true);
    expect(location.hash).toBe('#part-b/create');
    expect(document.querySelector('video')).toBeNull();
    expect(screen.getByText('Create and delete a computer, open')).toBeInTheDocument();

    // A second entry opens beside the first; each row closes only itself.
    await userEvent.click(rowButton('Run a command'));
    expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'true');
    expect(rowButton('Run a command')).toHaveAttribute('aria-expanded', 'true');
    expect(location.hash).toBe('#part-b/exec');
    await userEvent.click(rowButton('Create and delete a computer'));
    expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'false');
    await waitFor(() => expect(screen.queryByText('Make a Linux computer.')).not.toBeInTheDocument());
    expect(rowButton('Run a command')).toHaveAttribute('aria-expanded', 'true');
    expect(location.hash).toBe('#part-b/exec');
    await userEvent.click(rowButton('Run a command'));
    expect(location.hash).toBe('#part-b');
  });

  it('says decided, being built and not decided in a few words', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#part-c/shells');
    await renderApp();
    expect(await screen.findByText('Being built.')).toBeInTheDocument();
    expect(within(itemOf('Your shell inside')).getByRole('img', { name: 'being built' })).toBeInTheDocument();
    expect(itemOf('Your shell inside').querySelector('.ws-pip')).toHaveClass('building');
    act(() => { location.hash = '#part-a/calls'; });
    await waitFor(() => expect(screen.getByText('Decided.')).toBeInTheDocument());
    expect(within(itemOf('Every call defined once')).getByRole('img', { name: 'decided' })).toBeInTheDocument();
    expect(itemOf('Every call defined once').querySelector('.ws-pip')).toHaveClass('decided');
    act(() => { location.hash = '#part-a/core-in-zig'; });
    await waitFor(() => expect(within(itemOf('Which language is the core written in?')).getByText('Decided.')).toBeInTheDocument());
    expect(within(itemOf('Which language is the core written in?')).getByRole('img', { name: 'decided' })).toBeInTheDocument();
    expect(screen.getByText('Zig. It is the core piece.')).toBeInTheDocument();
    act(() => { location.hash = '#part-d/one-way'; });
    await waitFor(() => expect(screen.getByText('Not decided.')).toBeInTheDocument());
    expect(within(itemOf('Which way do changes go?')).getByRole('img', { name: 'not decided' })).toHaveClass('undecided');
  });

  it('shows a verdict written in the file as decided, with nothing to confirm', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#part-a/calls');
    await renderApp();
    expect(await screen.findByText('Decided.')).toBeInTheDocument();
    expect(screen.queryByText(/carried over/)).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /^Confirm/ })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /^Refine/ })).not.toBeInTheDocument();
  });
});

describe('addresses', () => {
  it('open a part and an entry, and old entry links open the entry where it now lives', async () => {
    history.replaceState(null, '', '/workshop#entry-exec');
    mockApi();
    await renderApp();
    await waitFor(() => expect(rowButton('Run a command')).toHaveAttribute('aria-expanded', 'true'));
    expect(location.hash).toBe('#part-b/exec');
    // A link followed: the browser pushes the address and says so (hashchange; the Navigation API tells a push from a
    // traverse, which jsdom lacks).
    act(() => { history.pushState(null, '', '/workshop#entry-reviewer'); dispatchEvent(new HashChangeEvent('hashchange')); });
    await waitFor(() => expect(rowButton('A hosted reviewer')).toHaveAttribute('aria-expanded', 'true'));
    expect(location.hash).toBe('#part-e/reviewer');
    expect(screen.getByRole('heading', { level: 2, name: 'Part E' })).toBeInTheDocument();
    // A decision of the whole product lives in its own place.
    act(() => { history.pushState(null, '', '/workshop#entry-one-host'); dispatchEvent(new HashChangeEvent('hashchange')); });
    await waitFor(() => expect(rowButton('Which host runs Silo first?')).toHaveAttribute('aria-expanded', 'true'));
    expect(location.hash).toBe('#product/one-host');
    expect(screen.getByRole('heading', { level: 2, name: 'Product' })).toBeInTheDocument();
  });

  it('open a suggestion once the live state arrives', async () => {
    history.replaceState(null, '', '/workshop#issue-31');
    mockApi();
    await renderApp();
    await waitFor(() => expect(rowButton('Share a base computer')).toHaveAttribute('aria-expanded', 'true'));
    expect(location.hash).toBe('#suggested/issue-31');
    expect(await screen.findByText('Body of 31.')).toBeInTheDocument();
  });

  it('fall back to the map when they name nothing, or cannot be decoded', async () => {
    history.replaceState(null, '', '/workshop#entry-nothing');
    mockApi();
    const first = await renderApp();
    await waitFor(() => expect(location.hash).toBe(''));
    expect(partTitle()).toBeNull();
    first.unmount();
    history.replaceState(null, '', '/workshop#entry-%E0%A4%A');
    await renderApp();
    expect(screen.getByRole('heading', { level: 1, name: 'Workshop' })).toBeInTheDocument();
    expect(document.querySelector('.ws-item.open')).toBeNull();
  });

  it('open what was left out in All, with the dropped entries, and an old left-out row where it lives', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#left');
    await renderApp(buildData({ features: FEATURES.map((feature) => (feature.id === 'exec' ? { ...feature, verdict: 'drop' as const } : feature)) }));
    expect(location.search).toBe('?view=all&state=dropped');
    expect(location.hash).toBe('');
    await userEvent.click(rowButton('Run a command'));
    expect(rowButton('Run a command')).toHaveAttribute('aria-expanded', 'true');
    expect(location.hash).toBe('#part-b/exec');
    // Followed by hand, the old address is written in its own form; read from the history, it is shown as it is.
    act(() => { history.pushState(null, '', '/workshop#left/exec'); dispatchEvent(new HashChangeEvent('hashchange')); });
    await waitFor(() => expect(screen.getByRole('heading', { level: 2, name: 'Part B' })).toBeInTheDocument());
    expect(location.hash).toBe('#part-b/exec');
    act(() => { history.replaceState(null, '', '/workshop#left/exec'); dispatchEvent(new PopStateEvent('popstate')); });
    expect(screen.getByRole('heading', { level: 2, name: 'Part B' })).toBeInTheDocument();
    expect(location.hash).toBe('#left/exec');
  });

  it('open the part when its entry no longer exists, and find an entry under another part', async () => {
    history.replaceState(null, '', '/workshop#part-a/gone-away');
    mockApi();
    const first = await renderApp();
    expect(screen.getByRole('heading', { level: 2, name: 'Part A' })).toBeInTheDocument();
    expect(location.hash).toBe('#part-a');
    first.unmount();
    history.replaceState(null, '', '/workshop#part-d/exec');
    await renderApp();
    await waitFor(() => expect(rowButton('Run a command')).toHaveAttribute('aria-expanded', 'true'));
    expect(location.hash).toBe('#part-b/exec');
  });
});

describe('the places of the title row', () => {
  it('gives what was left out as a filter of All: every dropped entry of the parts, decisions and features', async () => {
    mockApi();
    await renderApp();
    expect(screen.queryByRole('link', { name: /Left out/ })).toBeNull();
    history.replaceState(null, '', '/workshop?view=all&state=dropped');
    act(() => { dispatchEvent(new PopStateEvent('popstate')); });
    await waitFor(() => expect(document.querySelector('.wa-list')).not.toBeNull());
    expect([...document.querySelectorAll('.wa-list .wa-name > span:first-child')].map((name) => name.textContent)).toEqual(['Part E', 'Which format does an export use?', 'A hosted reviewer', 'An earlier desktop stack']);
    // The dropped decision no feature rests on stands, unfolded, under its part's row, before its stage's features.
    expect(within(itemOf('A hosted reviewer')).getByRole('img', { name: 'dropped' })).toBeInTheDocument();
    expect(itemOf('A hosted reviewer')).toHaveClass('gone');
    await userEvent.click(rowButton('An earlier desktop stack'));
    expect(screen.getByText('Dropped.')).toBeInTheDocument();
  });

  it('counts a part\'s features as built, being built, decided, not decided and dropped, in that order, each with its mark', async () => {
    const states: Record<string, object> = { triggers: { verdict: 'take', built: '2026-11-16' }, linux: { verdict: 'take', started: '2026-11-15' }, release: { verdict: 'take' } };
    mockApi();
    history.replaceState(null, '', '/workshop#part-e');
    await renderApp(buildData({ features: FEATURES.map((feature) => ({ ...feature, ...states[feature.id] })) }));
    expect(document.querySelector('.ws-detail .ws-tally')?.textContent?.replace(/\u00a0/g, ' ')).toBe('1 built · 1 being built · 1 decided · 4 not decided · 1 dropped');
    expect(within(itemOf('A Linux server')).getByRole('img', { name: 'being built' })).toBeInTheDocument();
    expect(within(itemOf('Signed releases')).getByRole('img', { name: 'decided' })).toBeInTheDocument();
    expect(within(itemOf('Tasks from GitHub events')).getByRole('img', { name: 'built' })).toBeInTheDocument();
    expect([...tile('part-e').querySelectorAll('.ws-pips i')].map((pip) => pip.className).slice(0, 3)).toEqual(['ws-pip built', 'ws-pip building', 'ws-pip decided']);
    expect(part('part-e')).toHaveClass('s-building');
    expect(part('part-e')).toHaveAccessibleName('Part E, being built, 7 features, 2 decisions. A wide bar under the drawing.');
  });

  it('opens the decisions of the whole product, with their tally', async () => {
    mockApi();
    await renderApp();
    await userEvent.click(screen.getByRole('link', { name: 'Product' }));
    expect(location.hash).toBe('#product');
    expect(screen.getByRole('heading', { level: 2, name: 'Product' })).toHaveFocus();
    expect([...document.querySelectorAll('.ws-detail .wa-name > span:first-child')].map((name) => name.textContent)).toEqual(['How is security built?', 'Which host runs Silo first?', 'Which logs does Silo keep?']);
    expect(document.querySelector('.ws-detail .ws-tally')).toHaveTextContent('Decisions: 1 decided · 1 not decided · 1 replaced');
  });

  it('opens the suggestions by votes, with a way to sign in for a visitor', async () => {
    mockApi();
    await renderApp();
    await userEvent.click(screen.getByRole('link', { name: 'Suggest a feature' }));
    expect(location.hash).toBe('#suggested');
    const group = screen.getByRole('region', { name: 'Suggested by people' });
    expect(within(group).getAllByRole('button', { name: /^(Share|Start)/ }).map((b) => b.textContent)).toEqual(['Share a base computer', 'Start a task from Slack']);
    expect(screen.getByRole('link', { name: 'Sign in to suggest' })).toHaveAttribute('href', '/api/workshop/login?return=%2Fworkshop%23suggested');
  });

  it('returns to the link that opened Product or the suggestions', async () => {
    mockApi({ me: owner });
    await renderApp();
    for (const [name, title] of [['Product', 'Product'], ['Suggest a feature', 'Suggest a feature']]) {
      await userEvent.click(screen.getByRole('link', { name }));
      expect(screen.getByRole('heading', { level: 2, name: title })).toHaveFocus();
      fireEvent.keyDown(document.activeElement as Element, { key: 'Escape' });
      await waitFor(() => expect(screen.getByRole('link', { name })).toHaveFocus());
    }
  });

  it('returns to the link of the place being closed, not to the one that opened an earlier place', async () => {
    mockApi({ me: owner });
    await renderApp();
    await userEvent.click(screen.getByRole('link', { name: 'Product' }));
    // The address changed by hand: the suggestions are reached by their address, with no opener of their own.
    act(() => { location.hash = '#suggested'; });
    await waitFor(() => expect(screen.getByRole('heading', { level: 2, name: 'Suggest a feature' })).toBeInTheDocument());
    await userEvent.click(screen.getByRole('button', { name: /← Full map/ }));
    await waitFor(() => expect(location.hash).toBe(''));
    await waitFor(() => expect(screen.getByRole('link', { name: 'Suggest a feature' })).toHaveFocus());
  });
});

describe('text written on GitHub', () => {
  it('never becomes HTML', async () => {
    const hostile = 'Look <img src=x onerror="window.hacked=1"> <script>window.hacked=1</script>\n\n[click](javascript:alert(1)) javascript:alert(1) and `<b>code</b>` and https://example.com/ok.';
    mockApi({ routes: { 'GET issue': (url) => ok(detail(Number(url.searchParams.get('n')), { body: hostile, comments: [{ id: 1, author: 'eve', avatar: 'javascript:alert(1)', bot: false, createdAt: '2026-11-14T10:00:00Z', text: hostile }] })) } });
    history.replaceState(null, '', '/workshop#suggested/issue-30');
    await renderApp();
    await screen.findAllByText(/onerror/);
    const more = document.getElementById('issue-30-more') as HTMLElement;
    expect(more.querySelector('img, script, p b')).toBeNull();
    expect(more.textContent).toContain('<img src=x onerror="window.hacked=1">');
    expect(more.textContent).toContain('[click](javascript:alert(1))');
    const hrefs = [...more.querySelectorAll('a')].map((a) => a.getAttribute('href'));
    expect(hrefs.filter((href) => href?.startsWith('javascript'))).toEqual([]);
    expect(hrefs).toContain('https://example.com/ok');
    expect(more.querySelectorAll('code')[0].textContent).toBe('<b>code</b>');
    expect((window as unknown as { hacked?: number }).hacked).toBeUndefined();
  });
});

describe('when the API is missing or slow', () => {
  for (const [label, state] of [
    ['not configured', () => refused(503, 'not configured')],
    ['GitHub failing', () => refused(502, 'GitHub failed')],
    ['unreachable', () => { throw new TypeError('offline'); }],
  ] as [string, Route][]) {
    it(`shows the whole plan without live parts when ${label}`, async () => {
      mockApi({ state, me: owner });
      await renderApp();
      // On the plain view, on the line under the title.
      expect(screen.getByText('Votes and comments are offline.').closest('.ws-status')).not.toBeNull();
      expect(part('part-a')).toHaveClass('s-next');
      // Who is signed in does not depend on the live state.
      expect(screen.getByText('polarzero')).toBeInTheDocument();
      expect(screen.queryByText(/^Decisions/)).not.toBeInTheDocument();
      expect(screen.getByRole('link', { name: 'Suggest a feature' })).toBeInTheDocument();
      await userEvent.click(part('part-b'));
      expect(document.querySelector('.ws-vote')).toBeNull();
      await userEvent.click(rowButton('Create and delete a computer'));
      expect(screen.getByText('Not decided.')).toBeInTheDocument();
      expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
    });
  }

  it('shows the plan from the file while the state is still loading', async () => {
    let release: (value: { status: number; body: unknown }) => void = () => {};
    mockApi({ state: () => new Promise((resolve) => { release = resolve; }) });
    history.replaceState(null, '', '/workshop#part-b');
    render(<App data={buildData()} />);
    expect(part('part-a')).toHaveClass('s-next');
    expect(rowButton('Create and delete a computer')).toBeInTheDocument();
    expect(screen.queryByText('Live comments are unavailable.')).not.toBeInTheDocument();
    expect(document.querySelector('.ws-vote')).toBeNull();
    expect(itemOf('Create and delete a computer').querySelector('.wa-side .wa-ph-side')).not.toBeNull();
    await act(async () => release(ok(liveState())));
    await waitFor(() => expect(itemOf('Create and delete a computer').querySelector('.wa-side')).toHaveTextContent('👍 12'));
  });
});
