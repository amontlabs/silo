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
    history.replaceState(null, '', '/workshop#part-b');
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
    history.replaceState(null, '', '/workshop#part-b/create');
    mockApi();
    await renderApp();
    const login = '/api/workshop/login?return=%2Fworkshop%23part-b%2Fcreate';
    expect(screen.getByRole('link', { name: 'Sign in with GitHub' })).toHaveAttribute('href', login);
    expect(within(itemOf('Create and delete a computer')).getByRole('link', { name: /12 votes/ })).toHaveAttribute('href', login);
    expect(await screen.findByRole('link', { name: 'Sign in to comment' })).toHaveAttribute('href', login);
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
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

  it('says once that sign-in did not complete and cleans the address', async () => {
    history.replaceState(null, '', '/workshop?signin=failed#part-b/exec');
    mockApi();
    await renderApp();
    expect(screen.getAllByText('Sign-in did not complete.').length).toBeGreaterThan(0);
    expect(location.search).toBe('');
    expect(location.hash).toBe('#part-b/exec');
  });

  it('say that signing out failed', async () => {
    mockApi({ me: visitor, routes: { 'POST logout': () => refused(502, 'GitHub failed.') } });
    history.replaceState(null, '', '/workshop#part-b/exec');
    await renderApp();
    await screen.findByRole('textbox', { name: 'Comment' });
    await userEvent.click(screen.getByRole('button', { name: 'Sign out' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(/^Could not sign out\.$/);
  });

  it('give a signed-out visitor a reason on the thumbs-up, and say offline plainly', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#part-b');
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
      expect(part('part-a')).toBeInTheDocument();
      for (const key of ['d', 'D', 'Enter', 'Escape']) fireEvent.keyDown(document.body, { key });
      act(() => { location.hash = '#decisions/decided-core-in-zig'; });
      await waitFor(() => expect(location.hash).toBe(''));
      expect(screen.queryByText('The core is written in Zig')).not.toBeInTheDocument();
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
    expect(rows.map((b) => b.textContent)).toEqual(['The core is written in Zig', 'Changes go one way only', 'Security is built properly']);
    await userEvent.click(rowButton('Changes go one way only'));
    expect(screen.getByText('Changes go one way only. Nothing else moves.')).toBeInTheDocument();
    expect(screen.getByText('16 Nov · Part D, Part E')).toBeInTheDocument();
    expect(location.hash).toBe('#decisions/decided-one-way');
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /^Take/ })).not.toBeInTheDocument();
  });
});

describe('the owner deciding entries', () => {
  it('sees how many wait on each part and tile, and the lowest one opens from the link', async () => {
    mockApi({ me: owner });
    await renderApp();
    expect(part('part-b').querySelector('.ws-todo')).toHaveTextContent('2');
    // A verdict written in the file never waits for the owner.
    expect(part('part-a').querySelector('.ws-todo')).toBeNull();
    expect(part('part-c').querySelector('.ws-todo')).toBeNull();
    expect(tile('part-b')).toHaveTextContent('2 to decide');
    expect(part('part-b')).toHaveAccessibleName(/, 2 to decide\./);
    // The lowest entry waiting for the owner.
    await userEvent.click(screen.getByRole('link', { name: '4 to decide' }));
    await ownerOn('Create and delete a computer');
    expect(location.hash).toBe('#part-b/create');
    expect(within(itemOf('Create and delete a computer')).getByText('to decide')).toBeInTheDocument();
    expect(document.activeElement).toBe(rowButton('Create and delete a computer'));
    for (const name of [/^Take/, /^Later$/, /^Drop$/]) expect(screen.getByRole('button', { name })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /^(Refine|Confirm)/ })).not.toBeInTheDocument();
    expect(screen.getByText('Suggested: take. Key records by id.')).toBeInTheDocument();
  });

  it('takes with Enter after the guard, then the next in the area, then the lowest anywhere', async () => {
    const time = clock();
    const calls = mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#part-b/create');
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
    expect(location.hash).toBe('#part-b/exec');
    // A second fast Enter lands on the row that just opened and is refused.
    fireEvent.keyDown(document.activeElement as Element, { key: 'Enter' });
    expect(posts(calls, 'verdict')).toHaveLength(1);
    time.advance(OPEN_GUARD_MS);
    await userEvent.click(screen.getByRole('button', { name: 'Later' }));
    expect(posts(calls, 'verdict').map((call) => call.body)).toEqual([{ n: 12, verdict: 'take' }, { n: 13, verdict: 'later' }]);
    // Part B has none left: the lowest one anywhere opens, its area unfolds.
    await ownerOn('Fork a computer');
    expect(screen.getByRole('heading', { level: 2, name: 'Part D' })).toBeInTheDocument();
    expect(location.hash).toBe('#part-d/fork');
    // The guard holds across the area change.
    fireEvent.keyDown(document.activeElement as Element, { key: 'Enter' });
    expect(posts(calls, 'verdict')).toHaveLength(2);
  });

  it('ignores Enter in a field and on other buttons', async () => {
    const time = clock();
    const calls = mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#part-b/create');
    await renderApp();
    await ownerOn('Create and delete a computer');
    time.advance(OPEN_GUARD_MS);
    fireEvent.keyDown(await screen.findByRole('textbox', { name: 'Comment' }), { key: 'Enter' });
    fireEvent.keyDown(screen.getByRole('button', { name: 'Drop' }), { key: 'Enter' });
    expect(posts(calls, 'verdict')).toHaveLength(0);
  });

  it('brings the row made current into view when its verdict buttons are out of sight', async () => {
    mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#part-b');
    await renderApp();
    const scrolled: string[] = [];
    Element.prototype.scrollIntoView = function scroll(this: Element) { scrolled.push(this.closest('.ws-item')?.id ?? ''); };
    await userEvent.click(rowButton('Create and delete a computer'));
    await wait(OPEN_GUARD_MS + 50);
    await userEvent.click(rowButton('Run a command'));
    await wait(OPEN_GUARD_MS + 50);
    // The first row's buttons are scrolled far above the window, then the row opened last is taken: the first
    // becomes current and is brought back into view.
    const first = itemOf('Create and delete a computer').querySelector('.ws-btn.go')?.closest('.ws-state') as HTMLElement;
    first.getBoundingClientRect = () => away;
    act(() => (document.activeElement as HTMLElement).blur());
    scrolled.length = 0;
    fireEvent.keyDown(document.body, { key: 'Enter' });
    await wait(300);
    expect(scrolled).toContain('entry-create');
    expect(location.hash).toBe('#part-b/create');
  });

  it('counts only undecided entries with an issue, whatever their file says elsewhere', async () => {
    const data = buildData();
    const base = data.features.find((feature) => feature.id === 'exec') as Feature;
    // Entries whose file already holds a verdict never wait for the owner; undecided ones with an issue do.
    const decided = (['take', 'later', 'drop'] as const).map((verdict, i): Feature => ({ ...base, position: 30 + i, id: `c-${verdict}`, name: `Decided ${verdict}`, verdict }));
    const open = { ...base, position: 40, id: 'c-open', name: 'Open one', verdict: null } as Feature;
    data.features = [...data.features, ...decided, open];
    mockApi({ me: owner, state: () => ok(liveState(Object.fromEntries([...decided, open].map((feature, i) => [50 + i, issue({ entry: feature.id, title: feature.name })])))) });
    history.replaceState(null, '', '/workshop#part-b');
    render(<App data={data} />);
    await waitFor(() => expect(document.querySelector('.ws-detail .ws-tally')).toHaveTextContent(/to decide/));
    // `create`, `exec` and the new open one wait; the decided ones count as what their file says.
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
    history.replaceState(null, '', '/workshop#part-b/exec');
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

  it('show a vote being sent as pending with the count unchanged', async () => {
    const answer = deferred();
    const calls = mockApi({ me: visitor, routes: { 'POST vote': answer.route } });
    history.replaceState(null, '', '/workshop#part-b/exec');
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
    history.replaceState(null, '', '/workshop#part-b');
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
    history.replaceState(null, '', '/workshop#part-b/exec');
    await renderApp();
    await screen.findByText('Sign in to comment');
    expect(screen.getByRole('link', { name: 'or comment directly on GitHub' })).toHaveAttribute('href', 'https://github.com/amontlabs/silo/issues/13');
    expect(await screen.findByRole('link', { name: 'Earlier comments on GitHub ↗' })).toHaveAttribute('href', 'https://github.com/amontlabs/silo/issues/13');
  });
});

// Every write the page makes, refused every way the API can refuse it.
describe('a refused write', () => {
  type Reply = { status: number; body: unknown };
  const STORE = 'silo-workshop-writes-2';
  const ROW = 'Create and delete a computer';
  const VOTED = 'Run a command';

  // `make` sends the write; `during` is what the person sees while it is out; `undone` is what must be back
  // once it is refused; `typed` is what must come back in its input.
  const writes = [
    {
      name: 'comment', as: visitor, at: '#part-b/exec', route: 'POST comment', alert: /comment/i,
      async make() {
        await userEvent.type(await screen.findByRole('textbox', { name: 'Comment' }), 'Hello');
        await userEvent.click(screen.getByRole('button', { name: 'Comment' }));
      },
      during: () => expect(screen.getByText('Hello', { ignore: 'textarea' })).toBeInTheDocument(),
      undone: () => {
        expect(screen.queryByText('Hello', { ignore: 'textarea' })).not.toBeInTheDocument();
        expect(within(itemOf(VOTED)).queryByText(/💬/)).not.toBeInTheDocument();
      },
      typed: () => expect(screen.getByRole('textbox', { name: 'Comment' })).toHaveValue('Hello'),
    },
    {
      name: 'vote', as: visitor, at: '#part-b', route: 'POST vote', alert: /vote/i,
      async make() {
        await userEvent.click(within(itemOf(VOTED)).getByRole('button', { name: /4 votes/ }));
      },
      during: () => expect(within(itemOf(VOTED)).getByRole('button', { name: /4 votes/ })).toBeDisabled(),
      undone: () => {
        // Still four votes, not marked as mine (signed out, the thumb is a link instead of a button).
        const thumb = within(itemOf(VOTED)).getByLabelText(/4 votes/);
        expect(thumb).not.toHaveAttribute('aria-pressed', 'true');
        expect(thumb).not.toBeDisabled();
      },
      typed: null,
    },
    {
      name: 'suggestion', as: visitor, at: '#suggested', route: 'POST suggest', alert: /suggest/i,
      async make() {
        await userEvent.type(screen.getByRole('textbox', { name: 'Title' }), 'Pause');
        await userEvent.click(screen.getByRole('button', { name: 'Suggest' }));
      },
      during: () => expect(screen.getByText('Pause')).toBeInTheDocument(),
      undone: () => expect(screen.queryByText('Pause')).not.toBeInTheDocument(),
      typed: () => expect(screen.getByRole('textbox', { name: 'Title' })).toHaveValue('Pause'),
    },
    {
      name: 'verdict', as: owner, at: '#part-b/create', route: 'POST verdict', alert: new RegExp(ROW),
      async make(time: ReturnType<typeof clock>) {
        await ownerOn(ROW);
        time.advance(OPEN_GUARD_MS);
        await userEvent.click(screen.getByRole('button', { name: 'Drop' }));
      },
      during: () => {
        expect(itemOf(ROW).querySelector('.ws-word')).toHaveTextContent('Saving…');
        // The queue has moved on to the next row while this one saves.
        expect(rowButton(VOTED)).toHaveAttribute('aria-expanded', 'true');
      },
      undone: () => expect(within(itemOf(ROW)).queryByText('dropped')).not.toBeInTheDocument(),
      typed: null,
    },
  ];

  const refusals: { name: string; reply: Reply | null; why: string | null; out?: boolean }[] = [
    { name: 'refused (400)', reply: refused(400, 'Too long by two words.'), why: 'Too long by two words.' },
    { name: 'forbidden (403)', reply: refused(403, 'Only the owner may do that.'), why: 'Only the owner may do that.' },
    { name: 'conflict (409)', reply: refused(409, 'Someone decided first.'), why: 'Someone decided first.' },
    { name: 'a server error with no message (500)', reply: { status: 500, body: {} }, why: null },
    { name: 'signed out (401)', reply: refused(401, 'Session ended.'), why: null, out: true },
    { name: 'no answer', reply: null, why: null },
  ];

  describe.each(writes)('$name', (write) => {
    it.each(refusals)('answered by $name is told, undone and not kept', async ({ reply, why, out }) => {
      const time = clock();
      const gate: { release: (answer: Reply) => void } = { release: () => {} };
      const calls = mockApi({
        me: write.as,
        routes: { [write.route]: () => new Promise<Reply>((resolve) => { gate.release = resolve; }) },
      });
      history.replaceState(null, '', `/workshop${write.at}`);
      const timeout = apiConfig.writeTimeoutMs;
      if (!reply) apiConfig.writeTimeoutMs = 30;
      try {
        await renderApp();
        await write.make(time);
        await waitFor(() => expect(posts(calls, write.route.replace('POST ', ''))).toHaveLength(1));
        write.during();
        if (reply) await act(async () => gate.release(reply));

        const alert = await screen.findByRole('alert');
        if (out) {
          // The person is shown signed out: no login, the way back in, and the alert says to sign in.
          expect(alert).toHaveTextContent(/sign in/i);
          expect(screen.queryByText(write.as.login, { selector: '.ws-login' })).not.toBeInTheDocument();
          expect(screen.getByRole('link', { name: 'Sign in with GitHub' })).toBeInTheDocument();
        } else {
          // It names what failed and why; where the server gave a reason, that is the reason shown.
          expect(alert).toHaveTextContent(write.alert);
          expect(alert).toHaveTextContent(/not saved\.\s*\S/i);
          if (why) expect(alert).toHaveTextContent(why);
          write.typed?.();
        }
        if (write.name === 'verdict' && !out) {
          // The alert is in the entry it concerns, and that row is open again, with its address.
          expect(document.getElementById('entry-create')).toContainElement(alert);
          expect(rowButton(ROW)).toHaveAttribute('aria-expanded', 'true');
          expect(location.hash).toBe('#part-b/create');
        }
        write.undone();
        expect(screen.queryByText('Sending…')).not.toBeInTheDocument();
        expect(screen.queryByText('Saving…')).not.toBeInTheDocument();
        expect(localStorage.getItem(STORE)).toBeNull();
        if (write.name === 'verdict') {
          // The refused verdict is not sent again by a key press.
          fireEvent.keyDown(document.activeElement as Element, { key: 'Enter' });
          expect(posts(calls, 'verdict')).toHaveLength(1);
        }
      } finally {
        apiConfig.writeTimeoutMs = timeout;
      }
    });
  });
});
