import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { apiConfig } from './api';
import { detail, itemOf, mockApi, ok, owner, part, posts, refused, renderApp, rowButton, visitor } from './test-support';
import type { Route } from './test-support';

// Signing in and writes.
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
    expect(posts(calls, 'comment')[0]).toMatchObject({ body: { entry: 'create', text: 'Please keep the defaults small.' }, headers: { 'X-Workshop': '1' } });

    await userEvent.click(within(itemOf('Create and delete a computer')).getByRole('button', { name: /12 votes/ }));
    await waitFor(() => expect(posts(calls, 'vote')).toHaveLength(1));
    expect(posts(calls, 'vote')[0].body).toEqual({ entry: 'create', on: true });
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
    expect(within(group).getAllByRole('button', { name: /votes/ }).map((b) => b.textContent)).toEqual(['👍14', '👍9', '👍']);
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

describe('the old list of decisions', () => {
  it('is gone for everyone: no link, and its address shows the map', async () => {
    for (const me of [null, visitor, owner]) {
      history.replaceState(null, '', '/workshop#decisions');
      mockApi({ me });
      const view = await renderApp();
      await waitFor(() => expect(location.hash).toBe(''));
      expect(screen.queryByRole('link', { name: /^Decisions/ })).not.toBeInTheDocument();
      expect(screen.queryByText('Only you see this.')).not.toBeInTheDocument();
      expect(document.querySelector('a[href="#decisions"]')).toBeNull();
      expect(part('part-a')).toBeInTheDocument();
      view.unmount();
    }
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

  // The 👍 of "Run a command" (4 votes, thread 13), asked in these tests with the API's answers held back.
  const exec = () => within(itemOf('Run a command'));
  const mine = (on: boolean) => () => ok({ on, comments: [] });

  it('show a vote the page knows as pressed and one up in the same frame, before any answer', async () => {
    const answer = deferred();
    const calls = mockApi({ me: visitor, routes: { 'POST vote': answer.route } });
    history.replaceState(null, '', '/workshop#part-b/exec');
    await renderApp();
    await waitFor(() => expect(calls.some((call) => call.path === 'vote' && call.method === 'GET')).toBe(true));
    fireEvent.click(exec().getByRole('button', { name: /4 votes/ }));
    // No await between the click and what is read: this is the frame of the click.
    const button = exec().getByRole('button', { name: /5 votes/ });
    expect(button).toHaveAttribute('aria-pressed', 'true');
    expect(button).toHaveAttribute('aria-busy', 'true');
    expect(button).toBeDisabled();
    expect(button).toHaveClass('ws-pending');
    expect(announced()).toBe('Sending vote');
    await waitFor(() => expect(posts(calls, 'vote')).toHaveLength(1));
    expect(posts(calls, 'vote')[0].body).toEqual({ entry: 'exec', on: true });
    // A second click while the request runs does nothing.
    fireEvent.click(button);
    expect(posts(calls, 'vote')).toHaveLength(1);
    expect(localStorage.getItem('silo-workshop-writes-2')).toBeNull();
    // The server's count then replaces the page's own.
    await act(async () => answer.release(ok({ ok: true, up: 9 })));
    const done = exec().getByRole('button', { name: /9 votes/ });
    expect(done).toHaveAttribute('aria-pressed', 'true');
    expect(done).not.toHaveClass('ws-pending');
    expect(done).toBeEnabled();
  });

  it('show a vote taken back as released and one down in the same frame', async () => {
    const answer = deferred();
    const calls = mockApi({ me: visitor, routes: { 'GET vote': mine(true), 'POST vote': answer.route } });
    history.replaceState(null, '', '/workshop#part-b/exec');
    await renderApp();
    await waitFor(() => expect(exec().getByRole('button', { name: /4 votes/ })).toHaveAttribute('aria-pressed', 'true'));
    fireEvent.click(exec().getByRole('button', { name: /4 votes/ }));
    expect(exec().queryByRole('button', { name: /5 votes/ })).toBeNull();
    const button = exec().getByRole('button', { name: /3 votes/ });
    expect(button).toHaveAttribute('aria-pressed', 'false');
    expect(button).toHaveClass('ws-pending');
    await waitFor(() => expect(posts(calls, 'vote')).toHaveLength(1));
    expect(posts(calls, 'vote')[0].body).toEqual({ entry: 'exec', on: false });
    await act(async () => answer.release(ok({ ok: true, up: 3 })));
    expect(exec().getByRole('button', { name: /3 votes/ })).toHaveAttribute('aria-pressed', 'false');
  });

  it('show a vote the page does not know yet as working, and change it once `GET vote` has answered', async () => {
    const asked = deferred();
    const answer = deferred();
    const calls = mockApi({ me: visitor, routes: { 'GET vote': asked.route, 'POST vote': answer.route } });
    history.replaceState(null, '', '/workshop#part-b');
    await renderApp();
    fireEvent.click(exec().getByRole('button', { name: /4 votes/ }));
    // Working at once, and nothing guessed: the count and the pressed state stay as they are.
    const waiting = exec().getByRole('button', { name: /4 votes/ });
    expect(waiting).toHaveClass('ws-pending');
    expect(waiting).toHaveAttribute('aria-busy', 'true');
    expect(waiting).toHaveAttribute('aria-pressed', 'false');
    expect(posts(calls, 'vote')).toHaveLength(0);
    await act(async () => asked.release(ok({ on: true, comments: [] })));
    // The person had given their 👍: the click takes it back, and the count never went up.
    expect(exec().queryByRole('button', { name: /5 votes/ })).toBeNull();
    const taken = exec().getByRole('button', { name: /3 votes/ });
    expect(taken).toHaveAttribute('aria-pressed', 'false');
    expect(taken).toHaveClass('ws-pending');
    await waitFor(() => expect(posts(calls, 'vote')).toHaveLength(1));
    expect(posts(calls, 'vote')[0].body).toEqual({ entry: 'exec', on: false });
    await act(async () => answer.release(ok({ ok: true, up: 3 })));
    expect(exec().getByRole('button', { name: /3 votes/ })).not.toHaveClass('ws-pending');
  });

  it('put a refused vote back to the pressed state and the count it had, and say why', async () => {
    const answer = deferred();
    const calls = mockApi({ me: visitor, routes: { 'POST vote': answer.route } });
    history.replaceState(null, '', '/workshop#part-b/exec');
    await renderApp();
    await waitFor(() => expect(calls.some((call) => call.path === 'vote' && call.method === 'GET')).toBe(true));
    fireEvent.click(exec().getByRole('button', { name: /4 votes/ }));
    expect(exec().getByRole('button', { name: /5 votes/ })).toHaveAttribute('aria-pressed', 'true');
    await act(async () => answer.release(refused(429, 'GitHub is busy. Try again in a minute.')));
    expect(await screen.findByText('Vote: not saved. GitHub is busy. Try again in a minute.')).toBeInTheDocument();
    const back = exec().getByRole('button', { name: /4 votes/ });
    expect(back).toHaveAttribute('aria-pressed', 'false');
    expect(back).toBeEnabled();
    expect(back).not.toHaveClass('ws-pending');
  });

  it('put a vote on a thread found closed back too, and show the thread closed', async () => {
    const answer = deferred();
    const calls = mockApi({ me: visitor, routes: { 'POST vote': answer.route } });
    history.replaceState(null, '', '/workshop#part-b/exec');
    await renderApp();
    await waitFor(() => expect(calls.some((call) => call.path === 'vote' && call.method === 'GET')).toBe(true));
    fireEvent.click(exec().getByRole('button', { name: /4 votes/ }));
    expect(exec().getByRole('button', { name: /5 votes/ })).toBeInTheDocument();
    await act(async () => answer.release({ status: 403, body: { error: 'closed', message: 'This thread is closed on GitHub.', n: 13 } }));
    expect(await exec().findByText(/This thread is closed on GitHub\./)).toBeInTheDocument();
    const back = exec().getByRole('button', { name: /4 votes/ });
    expect(back).toHaveAttribute('aria-pressed', 'false');
    expect(back).not.toHaveClass('ws-pending');
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
      during: () => expect(within(itemOf(VOTED)).getByRole('button', { name: /5 votes/ })).toBeDisabled(),
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
        await write.make();
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
        write.undone();
        expect(screen.queryByText('Sending…')).not.toBeInTheDocument();
        expect(screen.queryByText('Saving…')).not.toBeInTheDocument();
        expect(localStorage.getItem(STORE)).toBeNull();
      } finally {
        apiConfig.writeTimeoutMs = timeout;
      }
    });
  });
});
