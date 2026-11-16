import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { App, OPEN_GUARD_MS } from './App';
import { apiConfig } from './api';
import { buildData, issue, liveState } from './fixtures';
import {
  areaTitle, clock, detail, itemOf, mockApi, ok, owner, ownerOn, part, posts, refused, renderApp, rowButton, tile, visitor,
} from './test-support';
import type { Route } from './test-support';

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
      act(() => { location.hash = '#decisions/issue-42'; });
      await waitFor(() => expect(location.hash).toBe(''));
      expect(screen.queryByText('After a task ends, what happens to its computer?')).not.toBeInTheDocument();
      view.unmount();
    }
  });

  it('open for the owner from the link, with open decisions then the decided log', async () => {
    mockApi({ me: owner });
    await renderApp();
    await userEvent.click(screen.getByRole('link', { name: 'Decisions · 2 open' }));
    expect(location.hash).toBe('#decisions');
    const open = screen.getByRole('region', { name: 'Open' });
    expect(within(open).getAllByRole('button').map((b) => b.textContent)).toEqual(['After a task ends, what happens to its computer?', 'Which Zig version to pin']);
    const decided = screen.getByRole('region', { name: 'Decided' });
    expect(within(decided).getAllByRole('button').map((b) => b.textContent)).toEqual(['The engine is written in Zig', 'Merging changes main only', 'Security is built properly']);
    expect(within(decided).getAllByText('16 Nov')).toHaveLength(2);
    await userEvent.click(rowButton('Merging changes main only'));
    expect(screen.getByText("16 Nov · orchestrator, at the owner's word")).toBeInTheDocument();
    expect(location.hash).toBe('#decisions/decided-2');
  });

  it('let the owner decide an open decision, shown dimmed until it is confirmed', async () => {
    let release: (value: { status: number; body: unknown }) => void = () => {};
    const calls = mockApi({ me: owner, routes: { 'POST decide': () => new Promise((resolve) => { release = resolve; }) } });
    history.replaceState(null, '', '/workshop#issue-42');
    await renderApp();
    await ownerOn('After a task ends');
    expect(location.hash).toBe('#decisions/issue-42');
    const decide = screen.getByRole('button', { name: 'Decide' });
    expect(decide).toBeDisabled();
    await userEvent.type(screen.getByRole('textbox', { name: 'Decision' }), 'It is deleted.');
    await userEvent.click(decide);
    expect(posts(calls, 'decide')[0].body).toEqual({ n: 42, text: 'It is deleted.' });
    const pending = screen.getByText('It is deleted.').closest('.ws-c') as HTMLElement;
    expect(pending).toHaveClass('ws-pending');
    expect(within(screen.getByRole('region', { name: 'Open' })).getByRole('button', { name: /^After a task ends/ })).toBeInTheDocument();
    await act(async () => release(ok()));
    expect(within(screen.getByRole('region', { name: 'Decided' })).getByRole('button', { name: /^After a task ends/ })).toBeInTheDocument();
  });
});

describe('the owner deciding entries', () => {
  it('sees how many wait on each part and tile, and the lowest one opens from the link', async () => {
    mockApi({ me: owner });
    await renderApp();
    expect(part('computers').querySelector('.ws-todo')).toHaveTextContent('2');
    // The carried-over verdict on `calls` waits for the owner's confirmation.
    expect(part('engine').querySelector('.ws-todo')).toHaveTextContent('1');
    expect(part('setup').querySelector('.ws-todo')).toBeNull();
    expect(tile('computers')).toHaveTextContent('2 to decide');
    expect(part('computers')).toHaveAccessibleName(/, 2 to decide\./);
    // The lowest-numbered entry waiting for the owner is the carried-over one, to confirm.
    await userEvent.click(screen.getByRole('link', { name: '5 to decide' }));
    await ownerOn('Every call defined once');
    expect(location.hash).toBe('#engine/calls');
    expect(within(itemOf('Every call defined once')).getByText('confirm')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /^Confirm/ })).toBeInTheDocument();
    expect(document.activeElement).toBe(rowButton('Every call defined once'));
    act(() => { location.hash = '#computers/computers'; });
    await ownerOn('Create and delete a computer');
    for (const name of [/^Take/, /^Later$/, /^Drop$/, /^Refine…$/]) expect(screen.getByRole('button', { name })).toBeInTheDocument();
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
    await ownerOn('Every call defined once');
    expect(screen.getByRole('heading', { level: 2, name: 'Engine' })).toBeInTheDocument();
    expect(location.hash).toBe('#engine/calls');
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

  it('does nothing on Enter when the open row is not on screen', async () => {
    const time = clock();
    const calls = mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#computers/computers');
    await renderApp();
    await ownerOn('Create and delete a computer');
    time.advance(OPEN_GUARD_MS);
    fireEvent.keyDown(document.body, { key: 'Escape' });
    await waitFor(() => expect(areaTitle()).toBeNull());
    fireEvent.keyDown(document.body, { key: 'Enter' });
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

  it('guards Later, Drop and Refine like Take, and needs a note to refine', async () => {
    const time = clock();
    const calls = mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#computers/computers');
    await renderApp();
    await ownerOn('Create and delete a computer');
    await userEvent.click(screen.getByRole('button', { name: 'Later' }));
    await userEvent.click(screen.getByRole('button', { name: 'Drop' }));
    await userEvent.click(screen.getByRole('button', { name: 'Refine…' }));
    const submit = screen.getByRole('button', { name: 'Refine' });
    expect(submit).toBeDisabled();
    await userEvent.type(screen.getByRole('textbox', { name: 'Note' }), 'x');
    await userEvent.click(submit);
    expect(posts(calls, 'verdict')).toHaveLength(0);
    time.advance(OPEN_GUARD_MS);
    await userEvent.click(screen.getByRole('button', { name: 'Refine' }));
    expect(posts(calls, 'verdict').map((call) => call.body)).toEqual([{ n: 12, verdict: 'refine', note: 'x' }]);
  });

  it('confirms a carried-over verdict, and a carried-over refine with a note', async () => {
    const time = clock();
    const calls = mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#engine/calls');
    const first = await renderApp();
    // The row named by the address opens once the live state is there.
    await ownerOn('Every call defined once');
    expect(screen.getByText('Planned · carried over')).toBeInTheDocument();
    time.advance(OPEN_GUARD_MS);
    await userEvent.click(screen.getByRole('button', { name: /^Confirm/ }));
    expect(posts(calls, 'verdict')[0].body).toEqual({ n: 11, verdict: 'take' });
    first.unmount();

    const data = buildData();
    data.features = data.features.map((feature) => (feature.id === 'exec' ? { ...feature, verdict: 'refine' as const } : feature));
    history.replaceState(null, '', '/workshop#computers/exec');
    render(<App data={data} />);
    await ownerOn('Run a command');
    expect(screen.getByText('Being refined · carried over')).toBeInTheDocument();
    time.advance(OPEN_GUARD_MS);
    await userEvent.click(screen.getByRole('button', { name: /^Confirm/ }));
    expect(posts(calls, 'verdict')).toHaveLength(1);
    await userEvent.type(screen.getByRole('textbox', { name: 'Note' }), 'Still to split.');
    await userEvent.click(screen.getByRole('button', { name: 'Refine' }));
    expect(posts(calls, 'verdict')[1].body).toEqual({ n: 13, verdict: 'refine', note: 'Still to split.' });
  });

  it('gives a refused note back', async () => {
    const time = clock();
    mockApi({ me: owner, routes: { 'POST verdict': () => refused(502, 'GitHub failed.') } });
    history.replaceState(null, '', '/workshop#computers/exec');
    await renderApp();
    await ownerOn('Run a command');
    time.advance(OPEN_GUARD_MS);
    await userEvent.click(screen.getByRole('button', { name: 'Refine…' }));
    await userEvent.type(screen.getByRole('textbox', { name: 'Note' }), 'Split it.');
    await userEvent.click(screen.getByRole('button', { name: 'Refine' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Run a command: not saved. GitHub failed.');
    expect(rowButton('Run a command')).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByRole('textbox', { name: 'Note' })).toHaveValue('Split it.');
    expect(screen.queryByText('Split it.', { selector: '.ws-c p' })).not.toBeInTheDocument();
  });

  it('lets the owner decide a row being refined', async () => {
    const time = clock();
    const calls = mockApi({ me: owner, state: () => ok(liveState({ 12: issue({ entry: 'computers', verdict: 'refine' }) })) });
    history.replaceState(null, '', '/workshop#computers/computers');
    await renderApp();
    await ownerOn('Create and delete a computer');
    expect(screen.getByText('Being refined.')).toBeInTheDocument();
    time.advance(OPEN_GUARD_MS);
    fireEvent.keyDown(document.activeElement as Element, { key: 'Enter' });
    expect(posts(calls, 'verdict')[0].body).toEqual({ n: 12, verdict: 'take' });
  });

  it('offers no verdict on a suggestion, which the server only records on entries', async () => {
    let release: (value: { status: number; body: unknown }) => void = () => {};
    const time = clock();
    const calls = mockApi({ me: owner, routes: { 'POST suggest': () => new Promise((resolve) => { release = resolve; }) } });
    history.replaceState(null, '', '/workshop#suggested');
    await renderApp();
    await userEvent.type(screen.getByRole('textbox', { name: 'Title' }), 'Pause a computer');
    await userEvent.click(screen.getByRole('button', { name: 'Suggest' }));
    await userEvent.click(rowButton('Share a base computer'));
    expect(screen.queryByRole('button', { name: /^Take/ })).not.toBeInTheDocument();
    expect(within(itemOf('Share a base computer')).queryByText('to decide')).not.toBeInTheDocument();
    time.advance(OPEN_GUARD_MS);
    fireEvent.keyDown(document.activeElement ?? document.body, { key: 'Enter' });
    await act(async () => {});
    expect(posts(calls, 'verdict')).toEqual([]);
    expect(screen.getByRole('link', { name: '5 to decide' })).toBeInTheDocument();
    await act(async () => release(ok({ number: 80 })));
    await waitFor(() => expect(rowButton('Pause a computer').closest('.ws-item')).toHaveAttribute('id', 'issue-80'));
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
    expect(localStorage.getItem('silo-workshop-writes')).toBeNull();
    await act(async () => answer.release(ok()));
    expect(screen.getByText('On its way.').closest('.ws-c')).not.toHaveClass('ws-pending');
    expect(within(itemOf('Run a command')).getByText('💬 1')).toBeInTheDocument();
    expect(localStorage.getItem('silo-workshop-writes')).toContain('On its way.');
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
      expect(localStorage.getItem('silo-workshop-writes')).toBeNull();
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

  it('keep a verdict as saving, with the counts unchanged, until the server answers', async () => {
    const time = clock();
    const answer = deferred();
    mockApi({ me: owner, routes: { 'POST verdict': answer.route } });
    history.replaceState(null, '', '/workshop#computers/computers');
    await renderApp();
    await ownerOn('Create and delete a computer');
    time.advance(OPEN_GUARD_MS);
    fireEvent.keyDown(document.activeElement as Element, { key: 'Enter' });
    expect(itemOf('Create and delete a computer').querySelector('.ws-word')).toHaveTextContent('Saving…');
    expect(part('computers').querySelector('.ws-todo')).toHaveTextContent('2');
    expect(document.querySelector('.ws-detail .ws-tally')).toHaveTextContent('2 to decide');
    expect(announced()).toBe('Create and delete a computer: saving. Run a command, open');
    await userEvent.click(rowButton('Create and delete a computer'));
    time.advance(OPEN_GUARD_MS);
    expect(within(itemOf('Create and delete a computer')).queryByRole('button', { name: /^Take/ })).not.toBeInTheDocument();
    expect(localStorage.getItem('silo-workshop-writes')).toBeNull();
    await act(async () => answer.release(ok()));
    expect(within(itemOf('Create and delete a computer')).getByText('planned')).toBeInTheDocument();
    expect(part('computers').querySelector('.ws-todo')).toHaveTextContent('1');
    expect(localStorage.getItem('silo-workshop-writes')).toContain('take');
  });

  it('keep the page\'s own writes for six minutes over an older state', async () => {
    const time = clock();
    const calls = mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#computers/computers');
    const first = await renderApp();
    await ownerOn('Create and delete a computer');
    time.advance(OPEN_GUARD_MS);
    fireEvent.keyDown(document.activeElement as Element, { key: 'Enter' });
    await waitFor(() => expect(posts(calls, 'verdict')).toHaveLength(1));
    await act(async () => {});
    first.unmount();
    history.replaceState(null, '', '/workshop#computers');
    const second = await renderApp();
    expect(within(itemOf('Create and delete a computer')).getByText('planned')).toBeInTheDocument();
    second.unmount();
    time.advance(6 * 60_000);
    await renderApp();
    expect(within(itemOf('Create and delete a computer')).queryByText('planned')).not.toBeInTheDocument();
  });

  it('keep the page\'s own comment over an older answer', async () => {
    mockApi({ me: visitor });
    history.replaceState(null, '', '/workshop#computers/exec');
    const first = await renderApp();
    await userEvent.type(await screen.findByRole('textbox', { name: 'Comment' }), 'Mine.');
    await userEvent.click(screen.getByRole('button', { name: 'Comment' }));
    await act(async () => {});
    first.unmount();
    history.replaceState(null, '', '/workshop#computers/exec');
    await renderApp();
    expect(await screen.findByText('Mine.')).toBeInTheDocument();
    expect(within(itemOf('Run a command')).getByText('💬 1')).toBeInTheDocument();
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

  it('ask `me` only when the marker cookie is present', async () => {
    const calls = mockApi();
    await renderApp();
    expect(calls.some((call) => call.path === 'me')).toBe(false);
    expect(screen.getByRole('link', { name: 'Sign in with GitHub' })).toBeInTheDocument();
  });

  it('check the GitHub link like any other link, and link earlier comments', async () => {
    mockApi({ routes: { 'GET issue': (url) => ok(detail(Number(url.searchParams.get('n')), { url: 'javascript:alert(1)', more: true })) } });
    history.replaceState(null, '', '/workshop#computers/exec');
    await renderApp();
    await screen.findByText('Sign in to comment');
    expect(screen.getByRole('link', { name: 'or comment on GitHub' })).toHaveAttribute('href', 'https://github.com/amontlabs/silo/issues/13');
    expect(await screen.findByRole('link', { name: 'Earlier comments on GitHub ↗' })).toHaveAttribute('href', 'https://github.com/amontlabs/silo/issues/13');
  });
});
