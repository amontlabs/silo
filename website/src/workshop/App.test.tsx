import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { App } from './App';
import { buildData, FEATURES, issue, liveState } from './fixtures';
import type { LiveState } from './plan';
import {
  areaTitle, detail, itemOf, mockApi, ok, owner, part, refused, renderApp, rowButton, tile,
} from './test-support';
import type { Route } from './test-support';

// Engine and calls and `computers` are built, as their files say; `exec` is being built, as its issue says.
const builtData = () => buildData({ features: FEATURES.map((feature) => (['engine', 'calls', 'computers'].includes(feature.id) ? { ...feature, verdict: 'take' as const, built: '2026-11-16' } : feature)) });
const later = (): LiveState => liveState({
  13: issue({ entry: 'exec', verdict: 'take', building: true, up: 4 }),
  14: issue({ entry: 'fork', verdict: 'take' }),
});

describe('the map', () => {
  it('shows the title, one line, every part with its state and only the states present', async () => {
    mockApi();
    await renderApp();
    expect(screen.getByRole('heading', { level: 1, name: 'Workshop' })).toBeInTheDocument();
    expect(screen.getByText('Starting with the Engine.')).toBeInTheDocument();
    expect(document.querySelectorAll('.ws-part')).toHaveLength(16);
    expect(part('engine')).toHaveClass('s-next');
    expect(part('engine')).toHaveTextContent('next');
    expect(part('setup')).toHaveClass('s-planned');
    expect(part('setup')).toHaveTextContent('1 planned');
    expect(part('computers')).toHaveClass('s-open');
    expect(part('computers')).toHaveAccessibleName('Computers, not decided yet, 2 features. A whole virtual machine for your agent.');
    const key = screen.getByRole('list', { name: 'Key' });
    expect(within(key).getAllByRole('listitem').map((li) => li.textContent)).toEqual(['next', 'planned', 'not decided yet']);
  });

  it('shows built, building and next areas, on the map and the tiles alike', async () => {
    mockApi({ state: () => ok(later()) });
    await renderApp(builtData());
    expect(screen.getByText('Building Computers. Forks is next.')).toBeInTheDocument();
    expect(part('engine')).toHaveClass('s-done');
    expect(part('computers')).toHaveClass('s-now');
    expect(part('computers')).toHaveTextContent('now · 1/2');
    expect(part('forks')).toHaveClass('s-next');
    for (const [id, state] of [['engine', 's-done'], ['computers', 's-now'], ['forks', 's-next'], ['setup', 's-planned'], ['trust', 's-open']]) {
      expect(tile(id).parentElement).toHaveClass(state);
    }
    expect(tile('computers')).toHaveTextContent('now · 1/2');
    expect(within(screen.getByRole('list', { name: 'Key' })).getAllByRole('listitem').map((li) => li.textContent)).toEqual(['done', 'building now', 'next', 'planned', 'not decided yet']);
  });

  it('switches to the tiles in build order and back', async () => {
    mockApi();
    await renderApp();
    expect(screen.getByRole('button', { name: 'Map' })).toHaveAttribute('aria-pressed', 'true');
    await userEvent.click(screen.getByRole('button', { name: 'Order' }));
    expect(screen.getByRole('button', { name: 'Order' })).toHaveAttribute('aria-pressed', 'true');
    expect(location.search).toBe('?view=order');
    const order = [...document.querySelectorAll<HTMLElement>('.ws-tile')].map((t) => t.dataset.area);
    expect(order.slice(0, 3)).toEqual(['engine', 'computers', 'app']);
    expect(order.at(-1)).toBe('suggested');
    expect(screen.getByText('After that')).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: 'Map' }));
    expect(location.search).toBe('');
  });

  it('links to what is left out, the latest and the suggestions, in that order', async () => {
    mockApi();
    await renderApp();
    expect(screen.getByRole('link', { name: '2 suggested' })).toHaveAttribute('href', '#suggested');
    expect(screen.getByRole('link', { name: 'Left out · 2' })).toHaveAttribute('href', '#left');
    expect([...document.querySelectorAll('.ws-fold-link')].map((a) => a.textContent)).toEqual(['Left out · 2', 'Latest', '2 suggested']);
  });
});

describe('an area', () => {
  it('opens from its part: the map folds, the area and its entries show, the address and focus follow', async () => {
    mockApi();
    await renderApp();
    await userEvent.click(part('computers'));
    expect(location.hash).toBe('#computers');
    expect(document.querySelector('.ws-stage')).toHaveClass('fold');
    expect(part('computers')).toHaveAttribute('aria-current', 'true');
    expect(screen.getByRole('heading', { level: 2, name: 'Computers' })).toHaveFocus();
    expect(within(document.querySelector('.ws-detail') as HTMLElement).getByText('A whole virtual machine for your agent.')).toBeInTheDocument();
    expect(screen.getByText('2 not decided')).toBeInTheDocument();
    expect(rowButton('Create and delete a computer')).toBeInTheDocument();
    expect(rowButton('Run a command')).toBeInTheDocument();
    expect(screen.getByText('Computers, open')).toBeInTheDocument();
  });

  it('switches to another part, goes back with the button, Escape and Back, and returns focus', async () => {
    mockApi();
    await renderApp();
    await userEvent.click(part('computers'));
    await userEvent.click(part('forks'));
    expect(location.hash).toBe('#forks');
    expect(screen.getByRole('heading', { level: 2, name: 'Forks' })).toBeInTheDocument();
    await userEvent.click(screen.getByRole('button', { name: /Full map/ }));
    expect(location.hash).toBe('');
    expect(document.querySelector('.ws-stage')).not.toHaveClass('fold');
    expect(part('forks')).toHaveFocus();

    await userEvent.click(part('devices'));
    fireEvent.keyDown(document.activeElement as Element, { key: 'Escape' });
    expect(areaTitle()).toBeNull();
    expect(part('devices')).toHaveFocus();

    await userEvent.click(part('engine'));
    act(() => history.back());
    await waitFor(() => expect(areaTitle()).toBeNull());
    act(() => history.forward());
    await waitFor(() => expect(screen.getByRole('heading', { level: 2, name: 'Engine' })).toBeInTheDocument());
  });

  it('opens from a tile too, and the same tile closes it', async () => {
    mockApi();
    await renderApp();
    await userEvent.click(tile('devices'));
    expect(location.hash).toBe('#devices');
    expect(tile('devices')).toHaveAttribute('aria-current', 'true');
    await userEvent.click(tile('devices'));
    expect(location.hash).toBe('');
  });

  it('shows each entry\'s state word, votes and comment counts', async () => {
    history.replaceState(null, '', '/workshop#engine');
    mockApi();
    await renderApp();
    expect(within(itemOf('The engine runs on its own')).getByText('planned')).toBeInTheDocument();
    expect(within(itemOf('The engine runs on its own')).getByText('👍 8')).toBeInTheDocument();
    act(() => { location.hash = '#computers'; });
    await waitFor(() => expect(within(itemOf('Create and delete a computer')).getByText('💬 2')).toBeInTheDocument());
    expect(within(itemOf('Create and delete a computer')).getByText('👍 12')).toBeInTheDocument();
    expect(within(itemOf('Run a command')).queryByText(/💬/)).not.toBeInTheDocument();
  });
});

describe('an entry', () => {
  it('opens in place, loads its comments and is linked', async () => {
    const calls = mockApi({ routes: { 'GET issue': (url) => ok(detail(Number(url.searchParams.get('n')), { comments: [{ id: 1, author: 'mara-k', avatar: '', bot: false, createdAt: '2026-11-14T10:00:00Z', text: 'Keep stop separate from pause.' }] })) } });
    history.replaceState(null, '', '/workshop#computers');
    await renderApp();
    await userEvent.click(rowButton('Create and delete a computer'));
    expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByText('Make a Linux computer.')).toBeInTheDocument();
    expect(screen.getByText('Not decided yet.')).toBeInTheDocument();
    expect(await screen.findByText('Keep stop separate from pause.')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'or comment directly on GitHub' })).toHaveAttribute('href', 'https://github.com/amontlabs/silo/issues/12');
    expect(calls.some((call) => call.path === 'issue' && call.method === 'GET')).toBe(true);
    expect(location.hash).toBe('#computers/computers');
    expect(document.querySelector('video')).toBeNull();
    expect(screen.getByText('Create and delete a computer, open')).toBeInTheDocument();

    // A second entry opens beside the first; each row closes only itself.
    await userEvent.click(rowButton('Run a command'));
    expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'true');
    expect(rowButton('Run a command')).toHaveAttribute('aria-expanded', 'true');
    expect(location.hash).toBe('#computers/exec');
    await userEvent.click(rowButton('Create and delete a computer'));
    expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'false');
    await waitFor(() => expect(screen.queryByText('Make a Linux computer.')).not.toBeInTheDocument());
    expect(rowButton('Run a command')).toHaveAttribute('aria-expanded', 'true');
    expect(location.hash).toBe('#computers/exec');
    await userEvent.click(rowButton('Run a command'));
    expect(location.hash).toBe('#computers');
  });

  it('says being built and planned in a few words', async () => {
    mockApi({ state: () => ok(later()) });
    history.replaceState(null, '', '/workshop#computers/exec');
    await renderApp();
    expect(await screen.findByText('Being built now.')).toBeInTheDocument();
    expect(within(itemOf('Run a command')).getByText('building')).toBeInTheDocument();
    act(() => { location.hash = '#setup/shells'; });
    await waitFor(() => expect(screen.getByText('Planned.')).toBeInTheDocument());
  });

  it('shows a verdict written in the file as planned, with nothing to confirm', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#engine/calls');
    await renderApp();
    expect(await screen.findByText('Planned.')).toBeInTheDocument();
    expect(screen.queryByText(/carried over/)).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /^Confirm/ })).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /^Refine/ })).not.toBeInTheDocument();
  });
});

describe('addresses', () => {
  it('open an area and an entry, and old entry links open the entry where it now lives', async () => {
    history.replaceState(null, '', '/workshop#entry-exec');
    mockApi();
    await renderApp();
    await waitFor(() => expect(rowButton('Run a command')).toHaveAttribute('aria-expanded', 'true'));
    expect(location.hash).toBe('#computers/exec');
    act(() => { location.hash = '#entry-reviewer'; });
    await waitFor(() => expect(rowButton('A hosted reviewer')).toHaveAttribute('aria-expanded', 'true'));
    expect(location.hash).toBe('#left/reviewer');
    expect(screen.getByRole('heading', { level: 2, name: 'Left out' })).toBeInTheDocument();
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
    expect(areaTitle()).toBeNull();
    first.unmount();
    history.replaceState(null, '', '/workshop#entry-%E0%A4%A');
    await renderApp();
    expect(screen.getByRole('heading', { level: 1, name: 'Workshop' })).toBeInTheDocument();
    expect(document.querySelector('.ws-item.open')).toBeNull();
  });
});

describe('the bottom strip', () => {
  it('lists what is left out with each one\'s real state', async () => {
    mockApi();
    await renderApp();
    await userEvent.click(screen.getByRole('link', { name: 'Left out · 2' }));
    expect(location.hash).toBe('#left');
    expect(within(itemOf('A hosted reviewer')).getByText('dropped')).toBeInTheDocument();
    expect(itemOf('An earlier desktop stack')).toHaveClass('st-undecided');
    await userEvent.click(rowButton('An earlier desktop stack'));
    expect(screen.getByText('Not decided yet.')).toBeInTheDocument();
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
      expect(part('engine')).toHaveClass('s-next');
      // Who is signed in does not depend on the live state; deciding does.
      expect(screen.getByText('polarzero')).toBeInTheDocument();
      expect(screen.queryByText(/^Decisions/)).not.toBeInTheDocument();
      expect(screen.queryByText(/to decide/)).not.toBeInTheDocument();
      expect(screen.getByRole('link', { name: 'Suggest a feature' })).toBeInTheDocument();
      await userEvent.click(part('computers'));
      expect(screen.queryByText(/^👍/)).not.toBeInTheDocument();
      await userEvent.click(rowButton('Create and delete a computer'));
      expect(screen.getByText('Not decided yet.')).toBeInTheDocument();
      expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: /^Take/ })).not.toBeInTheDocument();
    });
  }

  it('shows the plan from the file while the state is still loading', async () => {
    let release: (value: { status: number; body: unknown }) => void = () => {};
    mockApi({ state: () => new Promise((resolve) => { release = resolve; }) });
    history.replaceState(null, '', '/workshop#computers');
    render(<App data={buildData()} />);
    expect(part('engine')).toHaveClass('s-next');
    expect(rowButton('Create and delete a computer')).toBeInTheDocument();
    expect(screen.queryByText('Live comments are unavailable.')).not.toBeInTheDocument();
    expect(screen.queryByText(/^👍/)).not.toBeInTheDocument();
    await act(async () => release(ok(liveState())));
    expect(await screen.findByText('👍 12')).toBeInTheDocument();
  });
});
