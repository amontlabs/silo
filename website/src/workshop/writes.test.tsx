import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it } from 'vitest';
import { App, OPEN_GUARD_MS } from './App';
import { apiConfig } from './api';
import { buildData, issue, liveState } from './fixtures';
import {
  clock, detail, itemOf, mockApi, ok, owner, ownerOn, part, posts, refused, renderApp, rowButton, tile, visitor, wait,
} from './test-support';
import type { Route } from './test-support';
import type { Feature } from './plan';

const away = { top: -900, bottom: -860, left: 0, right: 800, width: 800, height: 40, x: 0, y: -900, toJSON() {} } as DOMRect;
const scrollIntoView = Element.prototype.scrollIntoView;
afterEach(() => { Element.prototype.scrollIntoView = scrollIntoView; });

// Signing in, writes, the owner's decisions and verdicts.
describe('a signed-in person', () => {
  it('sees their login, comments, votes and signs out', async () => {
    const calls = mockApi({ me: visitor });
    history.replaceState(null, '', '/workshop#computers');
    await renderApp();
    expect(screen.getByText('mara-k', { selector: '.ws-login' })).toBeInTheDocument();
    await userEvent.click(rowButton('Create and delete a computer'));
    const box = await screen.findByRole('textbox', { name: 'Comment' });
    await userEvent.type(box, 'Please keep the defaults small.');
    await userEvent.click(screen.getByRole('button', { name: 'Comment' }));
    expect(screen.getByText('Please keep the defaults small.')).toBeInTheDocument();
    expect(within(itemOf('Create and delete a computer')).getByText('💬 3')).toBeInTheDocument();
    expect(posts(calls, 'comment')[0]).toMatchObject({ body: { n: 12, text: 'Please keep the defaults small.' }, headers: { 'X-Workshop': '1' } });

    await userEvent.click(within(itemOf('Create and delete a computer')).getByRole('button', { name: /12 votes/ }));
    await waitFor(() => expect(posts(calls, 'vote')).toHaveLength(1));
    expect(posts(calls, 'vote')[0].body).toEqual({ n: 12, on: true });
    expect(within(itemOf('Create and delete a computer')).getByRole('button', { name: /13 votes/ })).toHaveAttribute('aria-pressed', 'true');

    await userEvent.click(screen.getByRole('button', { name: 'Sign out' }));
    expect(await screen.findByRole('link', { name: 'Sign in with GitHub' })).toBeInTheDocument();
  });

  it('is sent to sign in for votes, comments and the header, back to the same place', async () => {
    history.replaceState(null, '', '/workshop#computers/computers');
    mockApi();
    await renderApp();
    const login = '/api/workshop/login?return=%2Fworkshop%23computers%2Fcomputers';
    expect(screen.getByRole('link', { name: 'Sign in with GitHub' })).toHaveAttribute('href', login);
    expect(within(itemOf('Create and delete a computer')).getByRole('link', { name: /12 votes/ })).toHaveAttribute('href', login);
    expect(await screen.findByRole('link', { name: 'Sign in to comment' })).toHaveAttribute('href', login);
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
  });

  it('gets a refused comment back in the box with a plain message', async () => {
    mockApi({ me: visitor, routes: { 'POST comment': () => refused(429, 'GitHub is busy.') } });
    history.replaceState(null, '', '/workshop#computers/exec');
    await renderApp();
    await userEvent.type(await screen.findByRole('textbox', { name: 'Comment' }), 'Hello');
    await userEvent.click(screen.getByRole('button', { name: 'Comment' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Comment: not saved. GitHub is busy.');
    expect(screen.getByRole('textbox', { name: 'Comment' })).toHaveValue('Hello');
    expect(within(itemOf('Run a command')).queryByText(/💬/)).not.toBeInTheDocument();
  });

  it('rolls back a refused vote', async () => {
    mockApi({ me: visitor, routes: { 'POST vote': () => refused(502, 'GitHub failed.') } });
    history.replaceState(null, '', '/workshop#computers');
    await renderApp();
    await userEvent.click(within(itemOf('Run a command')).getByRole('button', { name: /4 votes/ }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Vote: not saved. GitHub failed.');
    expect(within(itemOf('Run a command')).getByRole('button', { name: /4 votes/ })).toHaveAttribute('aria-pressed', 'false');
  });

  it('suggests a feature that appears under Suggested by people, dimmed until it has its issue', async () => {
    let release: (value: { status: number; body: unknown }) => void = () => {};
    const calls = mockApi({ me: visitor, routes: { 'POST suggest': () => new Promise((resolve) => { release = resolve; }) } });
    await renderApp();
    await userEvent.click(screen.getByRole('link', { name: 'Suggest a feature' }));
    await userEvent.type(screen.getByRole('textbox', { name: 'Title' }), 'Pause a computer');
    await userEvent.type(screen.getByRole('textbox', { name: 'Why' }), 'Stop is not pause.');
    await userEvent.click(screen.getByRole('button', { name: 'Suggest' }));
    expect(posts(calls, 'suggest')[0].body).toEqual({ title: 'Pause a computer', text: 'Stop is not pause.' });
    const sending = screen.getByText('Pause a computer').closest('.ws-item') as HTMLElement;
    expect(sending).toHaveClass('ws-pending');
    expect(sending).toHaveAttribute('aria-busy', 'true');
    expect(within(sending).getByText('Sending…')).toBeInTheDocument();
    expect(within(sending).queryByRole('button')).toBeNull();
    await act(async () => release(ok({ number: 77 })));
    const group = screen.getByRole('region', { name: 'Suggested by people' });
    expect(within(group).getByRole('button', { name: /^Pause a computer/ }).closest('.ws-item')).toHaveAttribute('id', 'issue-77');
    expect(within(group).getAllByRole('button', { name: /votes/ }).map((b) => b.textContent)).toEqual(['👍 14', '👍 9', '👍 0']);
  });

  it('gets a refused suggestion back in the form', async () => {
    mockApi({ me: visitor, routes: { 'POST suggest': () => refused(400, 'The title is too long.') } });
    history.replaceState(null, '', '/workshop#suggested');
    await renderApp();
    await userEvent.type(screen.getByRole('textbox', { name: 'Title' }), 'Pause');
    await userEvent.click(screen.getByRole('button', { name: 'Suggest' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Suggestion: not saved. The title is too long.');
    expect(screen.getByRole('textbox', { name: 'Title' })).toHaveValue('Pause');
    expect(screen.queryByRole('button', { name: /^Pause/ })).not.toBeInTheDocument();
  });

  it('drops to signed out when a write answers 401', async () => {
    mockApi({ me: visitor, routes: { 'POST comment': () => refused(401, 'Not signed in.') } });
    history.replaceState(null, '', '/workshop#computers/exec');
    await renderApp();
    await userEvent.type(await screen.findByRole('textbox', { name: 'Comment' }), 'Hello');
    await userEvent.click(screen.getByRole('button', { name: 'Comment' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(/^Sign in again\.$/);
    expect(screen.getByRole('link', { name: 'Sign in with GitHub' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Sign in to comment' })).toBeInTheDocument();
  });

  it('says once that sign-in did not complete and cleans the address', async () => {
    history.replaceState(null, '', '/workshop?signin=failed#computers/exec');
    mockApi();
    await renderApp();
    expect(screen.getAllByText('Sign-in did not complete.').length).toBeGreaterThan(0);
    expect(location.search).toBe('');
    expect(location.hash).toBe('#computers/exec');
  });

  it('say that signing out failed', async () => {
    mockApi({ me: visitor, routes: { 'POST logout': () => refused(502, 'GitHub failed.') } });
    history.replaceState(null, '', '/workshop#computers/exec');
    await renderApp();
    await screen.findByRole('textbox', { name: 'Comment' });
    await userEvent.click(screen.getByRole('button', { name: 'Sign out' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(/^Could not sign out\.$/);
  });

  it('give a signed-out visitor a reason on the thumbs-up, and say offline plainly', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#computers');
    const first = await renderApp();
    expect(within(itemOf('Run a command')).getByRole('link', { name: /4 votes/ })).toHaveAttribute('title', 'Sign in to vote');
    first.unmount();
    // A first visit: nothing remembered.
    localStorage.clear();
    mockApi({ state: () => refused(503, 'not configured') });
    history.replaceState(null, '', '/workshop#suggested');
    await renderApp();
    expect(screen.getByText('Suggestions are offline.')).toBeInTheDocument();
    expect(screen.getAllByText('Votes and comments are offline.').length).toBeGreaterThan(0);
  });
});

describe('the decisions', () => {
  it('are never offered to a visitor: no link, the address shows the map, no key opens them', async () => {
    for (const me of [null, visitor]) {
      history.replaceState(null, '', '/workshop#decisions');
      mockApi({ me });
      const view = await renderApp();
      await waitFor(() => expect(location.hash).toBe(''));
      expect(screen.queryByText(/^Decisions/)).not.toBeInTheDocument();
      expect(screen.queryByText('Only you see this.')).not.toBeInTheDocument();
      expect(document.querySelector('a[href="#decisions"]')).toBeNull();
      expect(part('engine')).toBeInTheDocument();
      for (const key of ['d', 'D', 'Enter', 'Escape']) fireEvent.keyDown(document.body, { key });
      act(() => { location.hash = '#decisions/decided-engine-in-zig'; });
      await waitFor(() => expect(location.hash).toBe(''));
      expect(screen.queryByText('The engine is written in Zig')).not.toBeInTheDocument();
      view.unmount();
    }
  });

  it('open for the owner from the link, from the plan file, newest first', async () => {
    mockApi({ me: owner });
    await renderApp();
    await userEvent.click(screen.getByRole('link', { name: 'Decisions' }));
    expect(location.hash).toBe('#decisions');
    expect(screen.getByText('3 decided')).toBeInTheDocument();
    const rows = within(document.querySelector('.ws-list') as HTMLElement).getAllByRole('button');
    expect(within(document.querySelector('.ws-list') as HTMLElement).getAllByText(/^1[56] Nov$/).map((n) => n.textContent)).toEqual(['16 Nov', '16 Nov', '15 Nov']);
    expect(rows.map((b) => b.textContent)).toEqual(['The engine is written in Zig', 'Merging changes main only', 'Security is built properly']);
    await userEvent.click(rowButton('Merging changes main only'));
    expect(screen.getByText('Merging changes main only. Nothing else moves.')).toBeInTheDocument();
    expect(screen.getByText('16 Nov · Diff and merge, Forks')).toBeInTheDocument();
    expect(location.hash).toBe('#decisions/decided-merge-main-only');
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /^Take/ })).not.toBeInTheDocument();
  });
});

describe('the owner deciding entries', () => {
  it('sees how many wait on each part and tile, and the lowest one opens from the link', async () => {
    mockApi({ me: owner });
    await renderApp();
    expect(part('computers').querySelector('.ws-todo')).toHaveTextContent('2');
    // A verdict written in the file never waits for the owner.
    expect(part('engine').querySelector('.ws-todo')).toBeNull();
    expect(part('setup').querySelector('.ws-todo')).toBeNull();
    expect(tile('computers')).toHaveTextContent('2 to decide');
    expect(part('computers')).toHaveAccessibleName(/, 2 to decide\./);
    // The lowest entry waiting for the owner.
    await userEvent.click(screen.getByRole('link', { name: '4 to decide' }));
    await ownerOn('Create and delete a computer');
    expect(location.hash).toBe('#computers/computers');
    expect(within(itemOf('Create and delete a computer')).getByText('to decide')).toBeInTheDocument();
    expect(document.activeElement).toBe(rowButton('Create and delete a computer'));
    for (const name of [/^Take/, /^Later$/, /^Drop$/]) expect(screen.getByRole('button', { name })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /^(Refine|Confirm)/ })).not.toBeInTheDocument();
    expect(screen.getByText('Suggested: take. Key records by id.')).toBeInTheDocument();
  });

  it('takes with Enter after the guard, then the next in the area, then the lowest anywhere', async () => {
    const time = clock();
    const calls = mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#computers/computers');
    await renderApp();
    await ownerOn('Create and delete a computer');
    fireEvent.keyDown(document.activeElement as Element, { key: 'Enter' });
    expect(posts(calls, 'verdict')).toHaveLength(0);
    time.advance(OPEN_GUARD_MS);
    fireEvent.keyDown(document.activeElement as Element, { key: 'Enter', repeat: true });
    expect(posts(calls, 'verdict')).toHaveLength(0);
    fireEvent.keyDown(document.activeElement as Element, { key: 'Enter' });
    expect(posts(calls, 'verdict').map((call) => call.body)).toEqual([{ n: 12, verdict: 'take' }]);
    expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'false');
    expect(itemOf('Create and delete a computer').querySelector('.ws-word')).toHaveTextContent('Saving…');
    expect(rowButton('Run a command')).toHaveAttribute('aria-expanded', 'true');
    expect(await within(itemOf('Create and delete a computer')).findByText('planned')).toBeInTheDocument();
    expect(location.hash).toBe('#computers/exec');
    // A second fast Enter lands on the row that just opened and is refused.
    fireEvent.keyDown(document.activeElement as Element, { key: 'Enter' });
    expect(posts(calls, 'verdict')).toHaveLength(1);
    time.advance(OPEN_GUARD_MS);
    await userEvent.click(screen.getByRole('button', { name: 'Later' }));
    expect(posts(calls, 'verdict').map((call) => call.body)).toEqual([{ n: 12, verdict: 'take' }, { n: 13, verdict: 'later' }]);
    // Computers has none left: the lowest one anywhere opens, its area unfolds.
    await ownerOn('Fork a computer');
    expect(screen.getByRole('heading', { level: 2, name: 'Forks' })).toBeInTheDocument();
    expect(location.hash).toBe('#forks/fork');
    // The guard holds across the area change.
    fireEvent.keyDown(document.activeElement as Element, { key: 'Enter' });
    expect(posts(calls, 'verdict')).toHaveLength(2);
  });

  it('ignores Enter in a field and on other buttons', async () => {
    const time = clock();
    const calls = mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#computers/computers');
    await renderApp();
    await ownerOn('Create and delete a computer');
    time.advance(OPEN_GUARD_MS);
    fireEvent.keyDown(await screen.findByRole('textbox', { name: 'Comment' }), { key: 'Enter' });
    fireEvent.keyDown(screen.getByRole('button', { name: 'Drop' }), { key: 'Enter' });
    expect(posts(calls, 'verdict')).toHaveLength(0);
  });

  it('records the verdict for the row it was given, and rolls back a refused one', async () => {
    const time = clock();
    let release: (value: { status: number; body: unknown }) => void = () => {};
    const calls = mockApi({ me: owner, routes: { 'POST verdict': () => new Promise((resolve) => { release = resolve; }) } });
    history.replaceState(null, '', '/workshop#computers/computers');
    await renderApp();
    await ownerOn('Create and delete a computer');
    time.advance(OPEN_GUARD_MS);
    await userEvent.click(screen.getByRole('button', { name: 'Drop' }));
    expect(itemOf('Create and delete a computer').querySelector('.ws-word')).toHaveTextContent('Saving…');
    expect(rowButton('Run a command')).toHaveAttribute('aria-expanded', 'true');
    await act(async () => release(refused(403, 'Not allowed.')));
    expect(await screen.findByRole('alert')).toHaveTextContent('Create and delete a computer: not saved. Not allowed.');
    expect(within(itemOf('Create and delete a computer')).queryByText('dropped')).not.toBeInTheDocument();
    expect(posts(calls, 'verdict').map((call) => call.body)).toEqual([{ n: 12, verdict: 'drop' }]);
    expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'true');
    expect(document.getElementById('entry-computers')).toContainElement(screen.getByRole('alert'));
    expect(location.hash).toBe('#computers/computers');
    fireEvent.keyDown(document.activeElement as Element, { key: 'Enter' });
    expect(posts(calls, 'verdict')).toHaveLength(1);
  });

  it('each have their own guard: an earlier open row stays armed when another opens', async () => {
    const time = clock();
    const calls = mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#computers/computers');
    await renderApp();
    await ownerOn('Create and delete a computer');
    const take = (name: string) => within(itemOf(name)).getByRole('button', { name: /^Take/ });
    expect(take('Create and delete a computer')).toHaveClass('ws-unarmed');
    await act(async () => { time.advance(OPEN_GUARD_MS); await new Promise((resolve) => setTimeout(resolve, OPEN_GUARD_MS + 20)); });
    expect(take('Create and delete a computer')).not.toHaveClass('ws-unarmed');
    // Opening a second row starts that row's guard only.
    await userEvent.click(rowButton('Run a command'));
    expect(take('Run a command')).toHaveClass('ws-unarmed');
    expect(take('Create and delete a computer')).not.toHaveClass('ws-unarmed');
    await userEvent.click(take('Run a command'));
    expect(posts(calls, 'verdict')).toHaveLength(0);
    await userEvent.click(take('Create and delete a computer'));
    expect(posts(calls, 'verdict').map((call) => call.body)).toEqual([{ n: 12, verdict: 'take' }]);
  });

  it('take Enter on the open row that holds the focus, else on the one opened last', async () => {
    const time = clock();
    const calls = mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#computers/computers');
    await renderApp();
    await ownerOn('Create and delete a computer');
    await userEvent.click(rowButton('Run a command'));
    time.advance(OPEN_GUARD_MS);
    // The focus is on the earlier row: Enter is its verdict, and it does not close it.
    act(() => rowButton('Create and delete a computer').focus());
    fireEvent.keyDown(rowButton('Create and delete a computer'), { key: 'Enter' });
    expect(posts(calls, 'verdict').map((call) => call.body)).toEqual([{ n: 12, verdict: 'take' }]);
    // With the focus on the page, the row opened last; a held key does nothing.
    act(() => (document.activeElement as HTMLElement).blur());
    fireEvent.keyDown(document.body, { key: 'Enter', repeat: true });
    expect(posts(calls, 'verdict')).toHaveLength(1);
    await waitFor(() => expect(rowButton('Run a command')).toHaveAttribute('aria-expanded', 'true'));
    // The row that became current is armed again, like a freshly opened one.
    act(() => (document.activeElement as HTMLElement).blur());
    fireEvent.keyDown(document.body, { key: 'Enter' });
    expect(posts(calls, 'verdict')).toHaveLength(1);
    time.advance(OPEN_GUARD_MS);
    fireEvent.keyDown(document.body, { key: 'Enter' });
    expect(posts(calls, 'verdict').map((call) => call.body?.n)).toEqual([12, 13]);
  });

  it('never takes for a row whose verdict buttons are out of sight, and a row made current is brought into view and re-armed', async () => {
    const calls = mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#computers');
    await renderApp();
    const scrolled: string[] = [];
    Element.prototype.scrollIntoView = function scroll(this: Element) { scrolled.push(this.closest('.ws-item')?.id ?? ''); };
    await userEvent.click(rowButton('Create and delete a computer'));
    await wait(OPEN_GUARD_MS + 50);
    await userEvent.click(rowButton('Run a command'));
    await wait(OPEN_GUARD_MS + 50);
    // The first row's buttons are scrolled far above the window.
    const first = itemOf('Create and delete a computer').querySelector('.ws-btn.go')?.closest('.ws-state') as HTMLElement;
    first.getBoundingClientRect = () => away;
    // With the focus on its own row button, the out-of-sight row is not taken.
    act(() => rowButton('Create and delete a computer').focus());
    fireEvent.keyDown(rowButton('Create and delete a computer'), { key: 'Enter' });
    expect(posts(calls, 'verdict')).toHaveLength(0);
    // With the focus on the page, the row opened last is taken; the earlier one becomes current, is brought
    // into view and waits its guard again.
    act(() => (document.activeElement as HTMLElement).blur());
    scrolled.length = 0;
    fireEvent.keyDown(document.body, { key: 'Enter' });
    expect(posts(calls, 'verdict').map((call) => call.body?.n)).toEqual([13]);
    await wait(300);
    expect(scrolled).toContain('entry-computers');
    expect(location.hash).toBe('#computers/computers');
    fireEvent.keyDown(document.activeElement as HTMLElement, { key: 'Enter' });
    await wait(OPEN_GUARD_MS);
    // Armed again, but still out of sight here: nothing.
    fireEvent.keyDown(document.activeElement as HTMLElement, { key: 'Enter' });
    expect(posts(calls, 'verdict')).toHaveLength(1);
  });

  it('sends no verdict for the open row and leaves the header to open its own row', async () => {
    const calls = mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#computers');
    await renderApp();
    await userEvent.click(rowButton('Create and delete a computer'));
    await wait(OPEN_GUARD_MS + 50);
    const b = rowButton('Run a command');
    act(() => b.focus());
    // The keyboard path of a button: keydown, then the browser's click unless the keydown was prevented.
    const event = fireEvent.keyDown(b, { key: 'Enter' });
    expect(event).toBe(true);
    if (event) await userEvent.click(b);
    expect(posts(calls, 'verdict')).toHaveLength(0);
    expect(b).toHaveAttribute('aria-expanded', 'true');
    expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'true');
  });

  it('counts only undecided entries with an issue, whatever their file says elsewhere', async () => {
    const data = buildData();
    const base = data.features.find((feature) => feature.id === 'exec') as Feature;
    // Entries whose file already holds a verdict never wait for the owner; undecided ones with an issue do.
    const decided = (['take', 'later', 'drop'] as const).map((verdict, i): Feature => ({ ...base, position: 30 + i, id: `c-${verdict}`, name: `Decided ${verdict}`, verdict }));
    const open = { ...base, position: 40, id: 'c-open', name: 'Open one', verdict: null } as Feature;
    data.features = [...data.features, ...decided, open];
    mockApi({ me: owner, state: () => ok(liveState(Object.fromEntries([...decided, open].map((feature, i) => [50 + i, issue({ entry: feature.id, title: feature.name })])))) });
    history.replaceState(null, '', '/workshop#computers');
    render(<App data={data} />);
    await waitFor(() => expect(document.querySelector('.ws-detail .ws-tally')).toHaveTextContent(/to decide/));
    // `computers`, `exec` and the new open one wait; the decided ones count as what their file says.
    expect(document.querySelector('.ws-detail .ws-tally')).toHaveTextContent('1 planned · 3 to decide · 1 later · 1 dropped');
  });
});

describe('writes the server has not confirmed', () => {
  function deferred() {
    let release: (value: { status: number; body: unknown }) => void = () => {};
    const route: Route = () => new Promise((resolve) => { release = resolve; });
    return { route, release: (value: { status: number; body: unknown }) => release(value) };
  }
  const announced = () => document.querySelector('.sr-only[aria-live="polite"]')?.textContent;

  it('show a comment being sent as pending, then as an ordinary comment', async () => {
    const answer = deferred();
    mockApi({ me: visitor, routes: { 'POST comment': answer.route } });
    history.replaceState(null, '', '/workshop#computers/exec');
    await renderApp();
    await userEvent.type(await screen.findByRole('textbox', { name: 'Comment' }), 'On its way.');
    await userEvent.click(screen.getByRole('button', { name: 'Comment' }));
    const pending = screen.getByText('On its way.').closest('.ws-c') as HTMLElement;
    expect(pending).toHaveClass('ws-pending');
    expect(within(pending).getByText('Sending…')).toBeInTheDocument();
    expect(within(itemOf('Run a command')).queryByText(/💬/)).not.toBeInTheDocument();
    expect(announced()).toBe('Sending comment');
    expect(localStorage.getItem('silo-workshop-writes-2')).toBeNull();
    await act(async () => answer.release(ok()));
    expect(screen.getByText('On its way.').closest('.ws-c')).not.toHaveClass('ws-pending');
    expect(within(itemOf('Run a command')).getByText('💬 1')).toBeInTheDocument();
    expect(localStorage.getItem('silo-workshop-writes-2')).toContain('On its way.');
  });

  it('take back a comment that times out', async () => {
    apiConfig.writeTimeoutMs = 30;
    try {
      mockApi({ me: visitor, routes: { 'POST comment': () => new Promise(() => {}) } });
      history.replaceState(null, '', '/workshop#computers/exec');
      await renderApp();
      await userEvent.type(await screen.findByRole('textbox', { name: 'Comment' }), 'Lost.');
      await userEvent.click(screen.getByRole('button', { name: 'Comment' }));
      expect(screen.getByText('Sending…')).toBeInTheDocument();
      expect(await screen.findByRole('alert')).toHaveTextContent('Comment: not saved. No answer.');
      expect(screen.getByRole('textbox', { name: 'Comment' })).toHaveValue('Lost.');
      expect(localStorage.getItem('silo-workshop-writes-2')).toBeNull();
    } finally {
      apiConfig.writeTimeoutMs = 20_000;
    }
  });

  it('show a vote being sent as pending with the count unchanged', async () => {
    const answer = deferred();
    const calls = mockApi({ me: visitor, routes: { 'POST vote': answer.route } });
    history.replaceState(null, '', '/workshop#computers/exec');
    await renderApp();
    await waitFor(() => expect(calls.some((call) => call.path === 'vote' && call.method === 'GET')).toBe(true));
    await userEvent.click(within(itemOf('Run a command')).getByRole('button', { name: /4 votes/ }));
    await waitFor(() => expect(posts(calls, 'vote')).toHaveLength(1));
    const button = within(itemOf('Run a command')).getByRole('button', { name: /4 votes/ });
    expect(button).toBeDisabled();
    expect(button).toHaveClass('ws-pending');
    await act(async () => answer.release(ok()));
    expect(within(itemOf('Run a command')).getByRole('button', { name: /5 votes/ })).toHaveAttribute('aria-pressed', 'true');
  });

  it('check the person\'s vote once per opening, never retrying a failure', async () => {
    const calls = mockApi({ me: visitor, routes: { 'GET vote': () => refused(502, 'GitHub failed.') } });
    history.replaceState(null, '', '/workshop#computers');
    await renderApp();
    await userEvent.click(rowButton('Run a command'));
    await userEvent.type(await screen.findByRole('textbox', { name: 'Comment' }), 'Several keystrokes');
    const checks = () => calls.filter((call) => call.method === 'GET' && call.path === 'vote').length;
    expect(checks()).toBe(1);
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    await userEvent.click(rowButton('Run a command'));
    await userEvent.click(rowButton('Run a command'));
    await waitFor(() => expect(checks()).toBe(2));
  });

  it('check the GitHub link like any other link, and link earlier comments', async () => {
    mockApi({ routes: { 'GET issue': (url) => ok(detail(Number(url.searchParams.get('n')), { url: 'javascript:alert(1)', more: true })) } });
    history.replaceState(null, '', '/workshop#computers/exec');
    await renderApp();
    await screen.findByText('Sign in to comment');
    expect(screen.getByRole('link', { name: 'or comment directly on GitHub' })).toHaveAttribute('href', 'https://github.com/amontlabs/silo/issues/13');
    expect(await screen.findByRole('link', { name: 'Earlier comments on GitHub ↗' })).toHaveAttribute('href', 'https://github.com/amontlabs/silo/issues/13');
  });
});
