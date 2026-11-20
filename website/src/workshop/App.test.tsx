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

// `core`, `calls` and `create` are built and `fork` is taken, as their files say.
const builtData = () => buildData({ features: FEATURES.map((feature) => (['core', 'calls', 'create'].includes(feature.id) ? { ...feature, verdict: 'take' as const, built: '2026-11-16' } : feature.id === 'fork' ? { ...feature, verdict: 'take' as const } : feature)) });

describe('the map', () => {
  it('shows the title, one line, every part with its state and only the states present', async () => {
    mockApi();
    await renderApp();
    expect(screen.getByRole('heading', { level: 1, name: 'Workshop' })).toBeInTheDocument();
    expect(screen.getByText('Starting with Part A.')).toBeInTheDocument();
    expect(document.querySelectorAll('.ws-part')).toHaveLength(5);
    expect(part('part-a')).toHaveClass('s-next');
    expect(part('part-a')).toHaveTextContent('next');
    expect(part('part-c')).toHaveClass('s-planned');
    expect(part('part-c')).toHaveTextContent('1 planned');
    expect(part('part-b')).toHaveClass('s-open');
    expect(part('part-b')).toHaveAccessibleName('Part B, not decided yet, 2 features. A part that holds another.');
    expect(part('part-c')).toHaveAccessibleName('Part C, 1 planned, 1 feature. A part inside Part B.');
    expect(part('part-d')).toHaveAccessibleName('Part D, not decided yet, 7 features, 2 decisions. A part with copies behind it.');
    const key = screen.getByRole('list', { name: 'Key' });
    expect(within(key).getAllByRole('listitem').map((li) => li.textContent)).toEqual(['next', 'planned', 'not decided yet']);
  });

  it('shows built and next parts, on the map and the tiles alike', async () => {
    mockApi();
    await renderApp(builtData());
    expect(screen.getByText('Part D is next.')).toBeInTheDocument();
    expect(part('part-a')).toHaveClass('s-done');
    expect(part('part-b')).toHaveClass('s-done');
    expect(part('part-d')).toHaveClass('s-next');
    for (const [id, state] of [['part-a', 's-done'], ['part-b', 's-done'], ['part-d', 's-next'], ['part-c', 's-planned'], ['part-e', 's-open']]) {
      expect(tile(id).parentElement).toHaveClass(state);
    }
    expect(tile('part-c')).toHaveTextContent('1 planned');
    expect(within(screen.getByRole('list', { name: 'Key' })).getAllByRole('listitem').map((li) => li.textContent)).toEqual(['done', 'next', 'planned', 'not decided yet']);
  });

  it('switches to the tiles in build order and back', async () => {
    mockApi();
    await renderApp();
    expect(screen.getByRole('button', { name: 'Map' })).toHaveAttribute('aria-pressed', 'true');
    await userEvent.click(screen.getByRole('button', { name: 'Order' }));
    expect(screen.getByRole('button', { name: 'Order' })).toHaveAttribute('aria-pressed', 'true');
    expect(location.search).toBe('?view=order');
    const order = [...document.querySelectorAll<HTMLElement>('.ws-tile')].map((t) => t.dataset.part);
    expect(order.slice(0, 4)).toEqual(['part-a', 'part-b', 'part-c', 'part-d']);
    expect(order.at(-1)).toBe('suggested');
    expect(screen.getByText('After that')).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Map' }));
    expect(location.search).toBe('');
  });

  it('links to what is left out, the whole product, the latest and the suggestions, in that order', async () => {
    mockApi();
    await renderApp();
    expect(screen.getByRole('link', { name: '2 suggested' })).toHaveAttribute('href', '#suggested');
    expect(screen.getByRole('link', { name: 'Left out · 3' })).toHaveAttribute('href', '#left');
    expect(screen.getByRole('link', { name: 'The whole product' })).toHaveAttribute('href', '#product');
    expect([...document.querySelectorAll('.ws-fold-link')].map((a) => a.textContent)).toEqual(['Left out · 3', 'The whole product', 'Latest', '2 suggested']);
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

  it('shows each entry\'s state word, votes and comment counts', async () => {
    history.replaceState(null, '', '/workshop#part-a');
    mockApi();
    await renderApp();
    expect(within(itemOf('The engine runs on its own')).getByText('planned')).toBeInTheDocument();
    expect(within(itemOf('The engine runs on its own')).getByText('👍 8')).toBeInTheDocument();
    act(() => { location.hash = '#part-b'; });
    await waitFor(() => expect(within(itemOf('Create and delete a computer')).getByText('💬 2')).toBeInTheDocument());
    expect(within(itemOf('Create and delete a computer')).getByText('👍 12')).toBeInTheDocument();
    expect(within(itemOf('Run a command')).queryByText(/💬/)).not.toBeInTheDocument();
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
    expect(screen.getByText('Not decided yet.')).toBeInTheDocument();
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

  it('says planned, decided and not decided in a few words', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#part-c/shells');
    await renderApp();
    expect(await screen.findByText('Planned.')).toBeInTheDocument();
    expect(within(itemOf('Your shell inside')).getByText('planned')).toBeInTheDocument();
    act(() => { location.hash = '#part-a/core-in-zig'; });
    await waitFor(() => expect(screen.getByText('Decided.')).toBeInTheDocument());
    expect(within(itemOf('Which language is the core written in?')).getByText('decided')).toBeInTheDocument();
    expect(screen.getByText('Zig. It is the core piece.')).toBeInTheDocument();
    act(() => { location.hash = '#part-d/one-way'; });
    await waitFor(() => expect(screen.getByText('Not decided yet.')).toBeInTheDocument());
    expect(itemOf('Which way do changes go?')).toHaveClass('st-undecided');
  });

  it('shows a verdict written in the file as planned, with nothing to confirm', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#part-a/calls');
    await renderApp();
    expect(await screen.findByText('Planned.')).toBeInTheDocument();
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
    act(() => { location.hash = '#entry-reviewer'; });
    await waitFor(() => expect(rowButton('A hosted reviewer')).toHaveAttribute('aria-expanded', 'true'));
    expect(location.hash).toBe('#part-e/reviewer');
    expect(screen.getByRole('heading', { level: 2, name: 'Part E' })).toBeInTheDocument();
    // A decision of the whole product lives in its own place.
    act(() => { location.hash = '#entry-one-host'; });
    await waitFor(() => expect(rowButton('Which host runs Silo first?')).toHaveAttribute('aria-expanded', 'true'));
    expect(location.hash).toBe('#product/one-host');
    expect(screen.getByRole('heading', { level: 2, name: 'The whole product' })).toBeInTheDocument();
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

  it('open a dropped entry in the left-out list where it was opened', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#left');
    await renderApp(buildData({ features: FEATURES.map((feature) => (feature.id === 'exec' ? { ...feature, verdict: 'drop' as const } : feature)) }));
    await userEvent.click(rowButton('Run a command'));
    expect(rowButton('Run a command')).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByRole('heading', { level: 2, name: 'Left out' })).toBeInTheDocument();
    expect(location.hash).toBe('#left/exec');
    act(() => { history.replaceState(null, '', '/workshop#left/exec'); dispatchEvent(new PopStateEvent('popstate')); });
    expect(screen.getByRole('heading', { level: 2, name: 'Left out' })).toBeInTheDocument();
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

describe('the bottom strip', () => {
  it('lists every dropped entry of the parts, decisions and features', async () => {
    mockApi();
    await renderApp();
    await userEvent.click(screen.getByRole('link', { name: 'Left out · 3' }));
    expect(location.hash).toBe('#left');
    expect([...document.querySelectorAll('.ws-detail .ws-name')].map((name) => name.textContent)).toEqual(['Which format does an export use?', 'A hosted reviewer', 'An earlier desktop stack']);
    expect(within(itemOf('A hosted reviewer')).getByText('dropped')).toBeInTheDocument();
    expect(within(itemOf('Which format does an export use?')).getByText('dropped')).toBeInTheDocument();
    expect([...document.querySelectorAll('.ws-detail .ws-tally')].map((tally) => tally.textContent?.replace(/\u00a0/g, ' '))).toEqual(['2 dropped', 'Decisions: 1 dropped']);
    await userEvent.click(rowButton('An earlier desktop stack'));
    expect(screen.getByText('Dropped.')).toBeInTheDocument();
  });

  it('opens the decisions of the whole product, with their tally', async () => {
    mockApi();
    await renderApp();
    await userEvent.click(screen.getByRole('link', { name: 'The whole product' }));
    expect(location.hash).toBe('#product');
    expect(screen.getByRole('heading', { level: 2, name: 'The whole product' })).toHaveFocus();
    expect([...document.querySelectorAll('.ws-detail .ws-name')].map((name) => name.textContent)).toEqual(['How is security built?', 'Which host runs Silo first?', 'Which logs does Silo keep?']);
    expect(document.querySelector('.ws-detail .ws-tally')).toHaveTextContent('Decisions: 1 decided · 1 not decided · 1 replaced');
  });

  it('opens the suggestions by votes, with a way to sign in for a visitor', async () => {
    mockApi();
    await renderApp();
    await userEvent.click(screen.getByRole('link', { name: '2 suggested' }));
    expect(location.hash).toBe('#suggested');
    const group = screen.getByRole('region', { name: 'Suggested by people' });
    expect(within(group).getAllByRole('button', { name: /^(Share|Start)/ }).map((b) => b.textContent)).toEqual(['Share a base computer', 'Start a task from Slack']);
    expect(screen.getByRole('link', { name: 'Sign in to suggest' })).toHaveAttribute('href', '/api/workshop/login?return=%2Fworkshop%23suggested');
  });

  it('returns to the link that opened the left-out list, the whole product, the latest or the suggestions', async () => {
    mockApi({ me: owner });
    await renderApp();
    for (const [name, title] of [['Left out · 3', 'Left out'], ['The whole product', 'The whole product'], ['Latest', 'Latest'], ['2 suggested', 'Suggest a feature']]) {
      await userEvent.click(screen.getByRole('link', { name }));
      expect(screen.getByRole('heading', { level: 2, name: title })).toHaveFocus();
      fireEvent.keyDown(document.activeElement as Element, { key: 'Escape' });
      await waitFor(() => expect(screen.getByRole('link', { name })).toHaveFocus());
    }
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
      expect(screen.getAllByText('Votes and comments are offline.').length).toBeGreaterThan(0);
      expect(part('part-a')).toHaveClass('s-next');
      // Who is signed in does not depend on the live state.
      expect(screen.getByText('polarzero')).toBeInTheDocument();
      expect(screen.queryByText(/^Decisions/)).not.toBeInTheDocument();
      expect(screen.getByRole('link', { name: 'Suggest a feature' })).toBeInTheDocument();
      await userEvent.click(part('part-b'));
      expect(screen.queryByText(/^👍/)).not.toBeInTheDocument();
      await userEvent.click(rowButton('Create and delete a computer'));
      expect(screen.getByText('Not decided yet.')).toBeInTheDocument();
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
    expect(screen.queryByText(/^👍/)).not.toBeInTheDocument();
    await act(async () => release(ok(liveState())));
    expect(await screen.findByText('👍 12')).toBeInTheDocument();
  });
});
