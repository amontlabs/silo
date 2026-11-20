// The first frame and the load: what the browser remembers, placeholders while nothing is known, the fresh ask
// after an old answer, the retry and "Try again", and the person `me` answers.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { App } from './App';
import type { Me } from './api';
import { buildData, issue, liveState } from './fixtures';
import type { LiveState } from './plan';
import { clock, itemOf, mockApi, ok, owner, part, posts, refused, renderApp, rowButton, visitor } from './test-support';
import { loadConfig } from './useWorkshop';
import { applyKept, keep, newerAnswer } from './writes';

const STATE = 'silo-workshop-state';
const PERSON = 'silo-workshop-me';
const marker = () => { document.cookie = 'workshop-hint=1; path=/'; };
const remember = (state: LiveState, person?: Me) => {
  localStorage.setItem(STATE, JSON.stringify(state));
  if (person) localStorage.setItem(PERSON, JSON.stringify(person));
};
/** A reply that waits until `release` is called. */
function held<T>() {
  let release: (value: T) => void = () => {};
  const promise = new Promise<T>((resolve) => { release = resolve; });
  return { promise, release: (value: T) => release(value) };
}
const at = (ms: number) => new Date(ms).toISOString();
const votes = (name: string) => itemOf(name).querySelector('.ws-vote')?.textContent;

describe('the first frame', () => {
  it('shows the remembered numbers and the remembered person before any answer', () => {
    marker();
    remember(liveState(), owner);
    const answer = held<{ status: number; body: unknown }>();
    mockApi({ state: () => answer.promise, routes: { 'GET me': () => answer.promise } });
    history.replaceState(null, '', '/workshop#part-b');
    render(<App data={buildData()} />);
    expect(votes('Create and delete a computer')).toBe('👍12');
    expect(within(itemOf('Create and delete a computer')).getByLabelText('2 comments')).toBeInTheDocument();
    expect(screen.getByText('polarzero')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Sign out' })).toBeInTheDocument();
    expect(document.querySelector('.ws-ph')).toBeNull();
  });

  it('reads what an earlier page remembered: a person with a "decides" flag, a state with verdicts', async () => {
    marker();
    const old = liveState();
    for (const [number, verdict] of [['10', 'take'], ['12', 'drop']]) old.issues[number] = { ...old.issues[number], verdict } as typeof old.issues[string];
    localStorage.setItem(STATE, JSON.stringify(old));
    localStorage.setItem(PERSON, JSON.stringify({ login: 'polarzero', avatar: '', canDecide: true }));
    const answer = held<{ status: number; body: unknown }>();
    mockApi({ state: () => answer.promise, routes: { 'GET me': () => answer.promise } });
    history.replaceState(null, '', '/workshop#part-b');
    render(<App data={buildData()} />);
    expect(screen.getByText('polarzero')).toBeInTheDocument();
    expect(votes('Create and delete a computer')).toBe('👍12');
    expect(screen.queryByText(/to decide/)).toBeNull();
    expect(within(itemOf('Create and delete a computer')).queryByText('dropped')).toBeNull();
    await act(async () => answer.release(ok(owner)));
    await waitFor(() => expect(JSON.parse(localStorage.getItem(PERSON) ?? '{}')).toEqual({ login: 'polarzero', avatar: '' }));
  });

  it('renders sign-in at once without the session marker, without waiting for `state` or `me`', () => {
    const answer = held<{ status: number; body: unknown }>();
    const calls = mockApi({ state: () => answer.promise });
    history.replaceState(null, '', '/workshop');
    render(<App data={buildData()} />);
    const signIn = screen.getByRole('link', { name: 'Sign in with GitHub' });
    expect(signIn).toHaveAttribute('aria-describedby', 'ws-signin-tip');
    expect(document.getElementById('ws-signin-tip')).toHaveTextContent('or comment directly on GitHub');
    expect(document.getElementById('ws-signin-tip')).toHaveAttribute('role', 'tooltip');
    expect(screen.getByRole('link', { name: 'Suggest a feature' })).toBeInTheDocument();
    expect(calls.some((call) => call.path === 'me')).toBe(false);
  });

  it('forgets a remembered person without the session marker, and when `me` answers signed out', async () => {
    remember(liveState(), owner);
    mockApi();
    const first = await renderApp();
    expect(screen.queryByText('polarzero')).not.toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Sign in with GitHub' })).toBeInTheDocument();
    first.unmount();
    marker();
    localStorage.setItem(PERSON, JSON.stringify(owner));
    mockApi({ routes: { 'GET me': () => ({ status: 401, body: { error: 'unauthorized' } }) } });
    await renderApp();
    expect(screen.getByRole('link', { name: 'Sign in with GitHub' })).toBeInTheDocument();
    expect(localStorage.getItem(PERSON)).toBeNull();
  });

  it('ignores a remembered state over seven days old or of another shape', () => {
    const answer = held<{ status: number; body: unknown }>();
    mockApi({ state: () => answer.promise });
    history.replaceState(null, '', '/workshop#part-b');
    remember({ ...liveState(), fetchedAt: at(Date.now() - 8 * 86_400_000) });
    const old = render(<App data={buildData()} />);
    expect(document.querySelector('.ws-ph-vote')).toBeInTheDocument();
    old.unmount();
    localStorage.setItem(STATE, JSON.stringify({ repo: 'amontlabs/silo', fetchedAt: at(Date.now()), issues: { 12: { up: 'many' } } }));
    render(<App data={buildData()} />);
    expect(document.querySelector('.ws-ph-vote')).toBeInTheDocument();
  });

  it('remembers the answer and, apart, its own writes, and shows both on the next visit', async () => {
    const calls = mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#part-b');
    const first = await renderApp();
    await userEvent.click(screen.getByRole('button', { name: '12 votes' }));
    await waitFor(() => expect(posts(calls, 'vote')).toHaveLength(1));
    await waitFor(() => expect(votes('Create and delete a computer')).toBe('👍13'));
    first.unmount();
    const answer = held<{ status: number; body: unknown }>();
    mockApi({ state: () => answer.promise, me: owner });
    render(<App data={buildData()} />);
    expect(votes('Create and delete a computer')).toBe('👍13');
    expect(screen.getByText('polarzero')).toBeInTheDocument();
    // The answer is remembered as it came.
    expect(JSON.parse(localStorage.getItem(STATE) ?? '{}').issues[12].up).toBe(12);
  });
});

describe('the account slot', () => {
  const headScript = /<script>\s*\/\/ A readable marker[\s\S]*?<\/script>/.exec(readFileSync(join(process.cwd(), 'workshop/index.html'), 'utf8'))?.[0]
    .replace(/<\/?script>/g, '') ?? '';

  it('is marked for the signed-in width before first paint when the session marker is there', () => {
    document.documentElement.classList.remove('ws-session');
    new Function(headScript)();
    expect(document.documentElement).not.toHaveClass('ws-session');
    document.cookie = 'workshop-hint=1; path=/';
    new Function(headScript)();
    expect(document.documentElement).toHaveClass('ws-session');
    document.documentElement.classList.remove('ws-session');
  });

  it('takes the width of what it holds only once `me` has answered, and shows a long login whole on hover', async () => {
    const login = 'a-login-of-thirty-nine-characters-xxxxx';
    let answer: (value: { status: number; body: unknown }) => void = () => {};
    mockApi({ me: owner, routes: { 'GET me': () => new Promise((resolve) => { answer = resolve; }) } });
    const slot = document.createElement('div');
    document.body.append(slot);
    render(<App data={buildData()} accountSlot={slot} />);
    await act(async () => {});
    expect(slot).not.toHaveAttribute('data-account');
    await act(async () => answer(ok({ ...owner, login })));
    await waitFor(() => expect(slot).toHaveAttribute('data-account', 'in'));
    expect(screen.getByText(login)).toHaveAttribute('title', login);
    expect(screen.getByText(login)).toHaveClass('ws-login');
    slot.remove();
  });

  it('keeps its width during the visit once `me` answers signed in, and stores the width of the login for the next', async () => {
    vi.stubGlobal('matchMedia', (query: string) => ({ matches: query === '(min-width: 481px)', media: query, addEventListener: () => {}, removeEventListener: () => {} }));
    vi.spyOn(HTMLElement.prototype, 'scrollWidth', 'get').mockReturnValue(70);
    vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockReturnValue(52);
    mockApi({ me: owner });
    const slot = document.createElement('div');
    document.body.append(slot);
    render(<App data={buildData()} accountSlot={slot} />);
    await waitFor(() => expect(slot).toHaveAttribute('data-account', 'in'));
    await waitFor(() => expect(localStorage.getItem('silo-workshop-account-width')).toBe('134'));
    // Applied only by the head script, before the next visit's first paint.
    expect(document.documentElement.style.getPropertyValue('--ws-account-w')).toBe('');
    slot.remove();
  });

  it('switches to the signed-out width when the marker was stale', async () => {
    mockApi({ me: owner, routes: { 'GET me': () => refused(401, 'Not signed in.') } });
    const slot = document.createElement('div');
    document.body.append(slot);
    render(<App data={buildData()} accountSlot={slot} />);
    await waitFor(() => expect(slot).toHaveAttribute('data-account', 'out'));
    slot.remove();
  });
});

describe('the load', () => {
  it('asks once more with fresh=1 after an old answer, and shows that answer in place', async () => {
    const asked: string[] = [];
    mockApi({
      state: (url) => {
        asked.push(url.search);
        return url.searchParams.get('fresh') === '1'
          ? ok(liveState({ 12: issue({ entry: 'create', title: 'Create and delete a computer', up: 30, comments: 2 }) }))
          : ok({ ...liveState(), fetchedAt: at(Date.now() - 1000), old: true });
      },
    });
    history.replaceState(null, '', '/workshop#part-b');
    await renderApp();
    await waitFor(() => expect(votes('Create and delete a computer')).toBe('👍30'));
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(asked).toEqual(['', '?fresh=1']);
  });

  it('counts an answer as old from the Age header or from its fetchedAt, and only once', async () => {
    for (const reply of [
      () => ({ status: 200, body: liveState(), headers: { Age: '40' } }),
      () => ok({ ...liveState(), fetchedAt: at(Date.now() - 3 * 60_000) }),
    ]) {
      const asked: string[] = [];
      mockApi({ state: (url) => { asked.push(url.search); return url.search ? ok({ ...liveState(), fetchedAt: at(Date.now() - 5 * 60_000) }) : reply(); } });
      const view = await renderApp();
      await waitFor(() => expect(asked).toEqual(['', '?fresh=1']));
      await new Promise((resolve) => setTimeout(resolve, 30));
      expect(asked).toEqual(['', '?fresh=1']);
      view.unmount();
      localStorage.clear();
    }
    const asked: string[] = [];
    mockApi({ state: (url) => { asked.push(url.search); return ok(liveState()); } });
    await renderApp();
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(asked).toEqual(['']);
  });

  it('never replaces a newer state shown with an older answer, and says nothing when the fresh ask fails', async () => {
    remember(liveState({ 12: issue({ entry: 'create', title: 'Create and delete a computer', up: 40, comments: 2 }) }));
    const calls = mockApi({
      state: (url) => (url.searchParams.get('fresh') === '1'
        ? { status: 502, body: { error: 'github', message: 'GitHub failed.' } }
        : ok({ ...liveState(), fetchedAt: at(Date.now() - 3600_000), old: true })),
    });
    history.replaceState(null, '', '/workshop#part-b');
    await renderApp();
    await waitFor(() => expect(calls.filter((call) => call.path === 'state')).toHaveLength(2));
    await act(async () => {});
    expect(votes('Create and delete a computer')).toBe('👍40');
    expect(screen.queryByText('Votes and comments are offline.')).not.toBeInTheDocument();
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('tries once more after a failure, then offers "Try again", which loads', async () => {
    loadConfig.retryMs = 150;
    let fail = true;
    const calls = mockApi({ state: () => (fail ? { status: 502, body: { error: 'github', message: 'GitHub failed.' } } : ok(liveState())) });
    history.replaceState(null, '', '/workshop#part-b');
    render(<App data={buildData()} />);
    await waitFor(() => expect(calls.filter((call) => call.path === 'state')).toHaveLength(1));
    await new Promise((resolve) => setTimeout(resolve, 60));
    expect(calls.filter((call) => call.path === 'state')).toHaveLength(1);
    const again = await screen.findAllByRole('button', { name: 'Try again' }, { timeout: 2000 });
    expect(calls.filter((call) => call.path === 'state')).toHaveLength(2);
    expect(screen.getAllByText('Votes and comments are offline.').length).toBeGreaterThan(0);
    expect(document.querySelector('.ws-ph-vote')).toBeNull();
    fail = false;
    fireEvent.click(again[0]);
    for (const button of screen.getAllByRole('button', { name: 'Try again' })) expect(button).toBeDisabled();
    await waitFor(() => expect(votes('Create and delete a computer')).toBe('👍12'));
    expect(screen.queryByText('Votes and comments are offline.')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: 'Try again' })).not.toBeInTheDocument();
  });

  it('does not try again by itself when the server has no live state at all', async () => {
    const calls = mockApi({ state: () => ({ status: 503, body: { error: 'not_configured', message: 'Live comments are not set up on this server.' } }) });
    await renderApp();
    await new Promise((resolve) => setTimeout(resolve, 60));
    expect(calls.filter((call) => call.path === 'state')).toHaveLength(1);
    expect(screen.getAllByRole('button', { name: 'Try again' }).length).toBeGreaterThan(0);
  });

  it('keeps remembered numbers on screen when the load fails', async () => {
    remember(liveState());
    mockApi({ state: () => ({ status: 502, body: { error: 'github', message: 'GitHub failed.' } }) });
    history.replaceState(null, '', '/workshop#part-b');
    await renderApp();
    expect(votes('Create and delete a computer')).toBe('👍12');
    expect(screen.getAllByText('Votes and comments are offline.').length).toBeGreaterThan(0);
  });
});

describe('the page\'s own writes over an answer', () => {
  const computers = (up: number, comments = 2) => liveState({ 12: issue({ entry: 'create', title: 'Create and delete a computer', up, comments }) });
  const stored = () => JSON.parse(localStorage.getItem(STATE) ?? '{}').issues[12];

  // Remembered 12, real 20: the person votes at once, the server answers 21. The page shows 21 over memory, over
  // the CDN's copy (20), over a fresh answer seconds later that already has 21 and over one that still has 20; an
  // answer fetched over a minute later shows its own number.
  for (const [what, server] of [['a vote', true], ['a vote, from an older server that answers no count', false]] as const) {
    it(`keep the count the server answered for ${what}`, async () => {
      const time = clock();
      marker();
      remember({ ...computers(12), fetchedAt: at(Date.now() - 86_400_000) }, visitor);
      const cdn = held<{ status: number; body: unknown; headers?: Record<string, string> }>();
      const fresh = held<{ status: number; body: unknown }>();
      const calls = mockApi({
        me: visitor,
        state: (url) => (url.searchParams.get('fresh') === '1' ? fresh.promise : cdn.promise),
        routes: { 'POST vote': () => ok(server ? { ok: true, up: 21 } : { ok: true }) },
      });
      history.replaceState(null, '', '/workshop#part-b');
      const first = render(<App data={buildData()} />);
      expect(votes('Create and delete a computer')).toBe('👍12');
      fireEvent.click(screen.getByRole('button', { name: '12 votes' }));
      await waitFor(() => expect(posts(calls, 'vote')).toHaveLength(1));
      // Without a count from the server, the count shown moves by one.
      const kept = server ? '👍21' : '👍13';
      await waitFor(() => expect(votes('Create and delete a computer')).toBe(kept));
      await act(async () => cdn.release({ status: 200, body: { ...computers(20), fetchedAt: at(Date.now() - 5 * 60_000) }, headers: { Age: '300' } }));
      await waitFor(() => expect(stored().up).toBe(20));
      expect(votes('Create and delete a computer')).toBe(kept);
      time.advance(5_000);
      await act(async () => fresh.release(ok({ ...computers(21), fetchedAt: at(Date.now()) })));
      await waitFor(() => expect(stored().up).toBe(21));
      expect(votes('Create and delete a computer')).toBe(kept);
      first.unmount();
      // The next visit: memory has 21 and the write; an answer seconds later still has 20, one over a minute later 23.
      const late = held<{ status: number; body: unknown }>();
      mockApi({ me: visitor, state: () => late.promise });
      const second = render(<App data={buildData()} />);
      expect(votes('Create and delete a computer')).toBe(kept);
      await act(async () => late.release(ok({ ...computers(20), fetchedAt: at(Date.now()) })));
      expect(votes('Create and delete a computer')).toBe(kept);
      second.unmount();
      time.advance(2 * 60_000);
      mockApi({ me: visitor, state: () => ok({ ...computers(23), fetchedAt: at(Date.now()) }) });
      render(<App data={buildData()} />);
      await waitFor(() => expect(votes('Create and delete a computer')).toBe('👍23'));
    });
  }

  it('keep the comment count the server answered', async () => {
    const time = clock();
    marker();
    remember({ ...computers(12, 2), fetchedAt: at(Date.now() - 86_400_000) }, visitor);
    const cdn = held<{ status: number; body: unknown }>();
    const fresh = held<{ status: number; body: unknown }>();
    mockApi({ me: visitor, state: (url) => (url.search ? fresh.promise : cdn.promise), routes: { 'POST comment': () => ok({ ok: true, id: 1, comments: 9 }) } });
    history.replaceState(null, '', '/workshop#part-b/create');
    const first = render(<App data={buildData()} />);
    expect(within(itemOf('Create and delete a computer')).getByLabelText('2 comments')).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole('textbox', { name: 'Comment' })).toBeEnabled());
    await userEvent.type(screen.getByRole('textbox', { name: 'Comment' }), 'Mine.');
    await userEvent.click(screen.getByRole('button', { name: 'Comment' }));
    await waitFor(() => expect(within(itemOf('Create and delete a computer')).getByLabelText('9 comments')).toBeInTheDocument());
    await act(async () => cdn.release(ok({ ...computers(12, 8), fetchedAt: at(Date.now() - 5 * 60_000) })));
    expect(within(itemOf('Create and delete a computer')).getByLabelText('9 comments')).toBeInTheDocument();
    // Seconds later, an answer that already counts it: still 9.
    time.advance(5_000);
    await act(async () => fresh.release(ok({ ...computers(12, 9), fetchedAt: at(Date.now()) })));
    await waitFor(() => expect(stored().comments).toBe(9));
    expect(within(itemOf('Create and delete a computer')).getByLabelText('9 comments')).toBeInTheDocument();
    first.unmount();
    // Over a minute later, the server's own count.
    time.advance(2 * 60_000);
    mockApi({ me: visitor, state: () => ok({ ...computers(12, 11), fetchedAt: at(Date.now()) }) });
    render(<App data={buildData()} />);
    await waitFor(() => expect(within(itemOf('Create and delete a computer')).getByLabelText('11 comments')).toBeInTheDocument());
  });

  it('keep a new suggestion over a remembered state and a cached answer that lack it', async () => {
    clock();
    const suggestion = issue({ kind: 'suggestion', title: 'A new idea', author: 'mara-k', bot: false });
    keep({ n: 40, set: suggestion }, Date.now());
    expect(applyKept({ ...liveState(), fetchedAt: at(Date.now() - 86_400_000) }, Date.now()).issues[40]).toEqual(suggestion);
    expect(applyKept({ ...liveState(), fetchedAt: at(Date.now() + 30_000) }, Date.now()).issues[40]).toEqual(suggestion);
    remember({ ...liveState(), fetchedAt: at(Date.now() - 3600_000) });
    const answer = held<{ status: number; body: unknown }>();
    mockApi({ state: () => answer.promise });
    history.replaceState(null, '', '/workshop#suggested');
    render(<App data={buildData()} />);
    expect(screen.getByRole('button', { name: /^A new idea/ })).toBeInTheDocument();
    await act(async () => answer.release(ok({ ...liveState(), fetchedAt: at(Date.now() - 5 * 60_000) })));
    expect(screen.getByRole('button', { name: /^A new idea/ })).toBeInTheDocument();
  });

  it('hold on every path: random answers, writes and time; the newest answer kept as it came, values applied in order', () => {
    // The reference: the answer kept is the newest by fetchedAt, unchanged. Issue 12 shown is that answer with the
    // value of the last write of each field that is under a day old and that the answer was not fetched more than a
    // minute after. Showing it again changes nothing.
    let seed = 7;
    const random = () => { seed = (seed * 1103515245 + 12345) % 2 ** 31; return seed / 2 ** 31; };
    for (let run = 0; run < 50; run += 1) {
      localStorage.clear();
      const time = clock();
      let kept = null as LiveState | null;
      let model = null as { fetched: number; up: number } | null;
      const writes: { at: number; up?: number; comments?: number }[] = [];
      for (let step = 0; step < 40; step += 1) {
        const roll = random();
        if (roll < 0.2) {
          const up = Math.floor(random() * 50);
          writes.push({ at: Date.now(), up });
          keep({ n: 12, set: { up } }, Date.now());
        } else if (roll < 0.3) {
          const comments = Math.floor(random() * 50);
          writes.push({ at: Date.now(), comments });
          keep({ n: 12, set: { comments } }, Date.now());
        } else if (roll < 0.75) {
          const fetched = Date.now() - Math.floor(random() * 10 * 60_000) + Math.floor(random() * 3 * 60_000);
          const up = Math.floor(random() * 50);
          kept = newerAnswer(kept, { ...computers(up), fetchedAt: at(fetched), old: random() < 0.5 });
          if (!model || fetched >= model.fetched) model = { fetched, up };
        } else {
          time.advance(Math.floor(random() * (random() < 0.1 ? 30 * 3600_000 : 4 * 60_000)));
        }
        if (!kept || !model) continue;
        expect(Date.parse(kept.fetchedAt)).toBe(model.fetched);
        expect(kept.issues[12].up).toBe(model.up);
        expect(kept.old).toBeUndefined();
        let up = model.up;
        let comments = 2;
        for (const write of writes) {
          if (Date.now() - write.at >= 24 * 3600_000 || model.fetched > write.at + 60_000) continue;
          if (write.up !== undefined) up = write.up;
          if (write.comments !== undefined) comments = write.comments;
        }
        const shown = applyKept(kept, Date.now());
        expect(shown.issues[12].up).toBe(up);
        expect(shown.issues[12].comments).toBe(comments);
        expect(applyKept(shown, Date.now())).toEqual(shown);
      }
      vi.restoreAllMocks();
    }
  });

  it('keep the page\'s own comment over an older answer', async () => {
    mockApi({ me: visitor });
    history.replaceState(null, '', '/workshop#part-b/exec');
    const first = await renderApp();
    await userEvent.type(await screen.findByRole('textbox', { name: 'Comment' }), 'Mine.');
    await userEvent.click(screen.getByRole('button', { name: 'Comment' }));
    await act(async () => {});
    first.unmount();
    history.replaceState(null, '', '/workshop#part-b/exec');
    await renderApp();
    expect(await screen.findByText('Mine.')).toBeInTheDocument();
    expect(within(itemOf('Run a command')).getByText('💬 1')).toBeInTheDocument();
  });
});

describe('`me`', () => {
  it('say nothing on page load when the session is gone: the vote check waits for `me`', async () => {
    marker();
    remember(liveState(), visitor);
    const unauthorized = () => ({ status: 401, body: { error: 'unauthorized', message: 'Sign in again.' } });
    const calls = mockApi({ routes: { 'GET me': unauthorized, 'GET vote': unauthorized } });
    history.replaceState(null, '', '/workshop#part-b/create');
    render(<App data={buildData()} />);
    expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'true');
    expect(await screen.findByRole('link', { name: 'Sign in with GitHub' })).toBeInTheDocument();
    await new Promise((resolve) => setTimeout(resolve, 30));
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    expect(screen.queryByText('Sign in again.')).not.toBeInTheDocument();
    expect(calls.some((call) => call.path === 'vote')).toBe(false);
    expect(localStorage.getItem(PERSON)).toBeNull();
  });

  it('follow `me` when it answers differently from memory', async () => {
    marker();
    remember(liveState(), owner);
    mockApi({ me: visitor });
    history.replaceState(null, '', '/workshop#part-b');
    render(<App data={buildData()} />);
    expect(screen.getByText('polarzero')).toBeInTheDocument();
    expect(await screen.findByText('mara-k')).toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'Decisions' })).not.toBeInTheDocument();
    expect(JSON.parse(localStorage.getItem(PERSON) ?? '{}').login).toBe('mara-k');
  });
});

describe('addresses', () => {
  it('hold a named row closed only while nothing is shown and the load is pending', async () => {
    const answer = held<{ status: number; body: unknown }>();
    mockApi({ state: () => answer.promise });
    history.replaceState(null, '', '/workshop#part-b/create');
    render(<App data={buildData()} />);
    expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'false');
    await act(async () => answer.release(ok(liveState())));
    await waitFor(() => expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'true'));
  });

  it('wait for the load before falling back to the plan when the state shown lacks what they name', async () => {
    remember(liveState());
    const answer = held<{ status: number; body: unknown }>();
    mockApi({ state: () => answer.promise });
    history.replaceState(null, '', '/workshop#suggested/issue-40');
    render(<App data={buildData()} />);
    expect(location.hash).toBe('#suggested/issue-40');
    await act(async () => answer.release(ok(liveState({ 40: issue({ kind: 'suggestion', title: 'A new idea', author: 'lin-qi', bot: false }) }))));
    await waitFor(() => expect(rowButton('A new idea')).toHaveAttribute('aria-expanded', 'true'));
    expect(location.hash).toBe('#suggested/issue-40');
  });

  it('fall back to the plan once the load answered without what they name', async () => {
    remember(liveState());
    const answer = held<{ status: number; body: unknown }>();
    mockApi({ state: () => answer.promise });
    history.replaceState(null, '', '/workshop#suggested/issue-41');
    render(<App data={buildData()} />);
    expect(location.hash).toBe('#suggested/issue-41');
    await act(async () => answer.release(ok(liveState())));
    await waitFor(() => expect(location.hash).toBe(''));
    expect(part('part-a')).toBeInTheDocument();
  });

  it('opens late beside the row the person opened meanwhile, which stays the current one', async () => {
    let release: (value: { status: number; body: unknown }) => void = () => {};
    mockApi({ state: () => new Promise((resolve) => { release = resolve; }) });
    history.replaceState(null, '', '/workshop#part-b/exec');
    render(<App data={buildData()} />);
    await userEvent.click(rowButton('Create and delete a computer'));
    expect(location.hash).toBe('#part-b/create');
    await act(async () => release(ok(liveState())));
    await waitFor(() => expect(rowButton('Run a command')).toHaveAttribute('aria-expanded', 'true'));
    expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'true');
    expect(location.hash).toBe('#part-b/create');
  });
});

describe('a link to what only the fresh answer has', () => {
  for (const [label, freshOk] of [['opens it once the fresh answer has it', true], ['falls back to the plan when the fresh ask fails', false]] as const) {
    it(label, async () => {
      const fresh = held<{ status: number; body: unknown }>();
      mockApi({ state: (url) => (url.searchParams.get('fresh') === '1' ? fresh.promise : ok({ ...liveState(), old: true })) });
      history.replaceState(null, '', '/workshop#suggested/issue-57');
      render(<App data={buildData()} />);
      await waitFor(() => expect(document.querySelector('.ws[data-live="answered"]')).not.toBeNull());
      await new Promise((resolve) => setTimeout(resolve, 30));
      expect(location.hash).toBe('#suggested/issue-57');
      await act(async () => fresh.release(freshOk
        ? ok(liveState({ 57: issue({ kind: 'suggestion', title: 'Just suggested', author: 'lin-qi', bot: false }) }))
        : { status: 502, body: { error: 'github', message: 'GitHub failed.' } }));
      if (freshOk) {
        await waitFor(() => expect(rowButton('Just suggested')).toHaveAttribute('aria-expanded', 'true'));
        expect(location.hash).toBe('#suggested/issue-57');
      } else {
        await waitFor(() => expect(location.hash).toBe(''));
      }
    });
  }
});

describe('the suggest form', () => {
  it('shows for the person while no state is shown, keeps the draft, and sends once the state is in', async () => {
    marker();
    localStorage.setItem(PERSON, JSON.stringify(visitor));
    const answer = held<{ status: number; body: unknown }>();
    const calls = mockApi({ me: visitor, state: () => answer.promise, routes: { 'POST suggest': () => ok({ number: 77 }) } });
    history.replaceState(null, '', '/workshop#suggested');
    render(<App data={buildData()} />);
    await userEvent.type(screen.getByRole('textbox', { name: 'Title' }), 'An idea');
    const send = screen.getByRole('button', { name: 'Suggest' });
    expect(send).toBeDisabled();
    fireEvent.submit(send.closest('form') as HTMLFormElement);
    expect(posts(calls, 'suggest')).toHaveLength(0);
    expect(screen.getByRole('textbox', { name: 'Title' })).toHaveValue('An idea');
    await act(async () => answer.release(ok(liveState())));
    await waitFor(() => expect(screen.getByRole('button', { name: 'Suggest' })).toBeEnabled());
    await userEvent.click(screen.getByRole('button', { name: 'Suggest' }));
    await waitFor(() => expect(posts(calls, 'suggest')).toHaveLength(1));
  });

  it('stays, with its draft, after a failed load', async () => {
    marker();
    localStorage.setItem(PERSON, JSON.stringify(visitor));
    mockApi({ me: visitor, state: () => ({ status: 503, body: { error: 'not_configured', message: 'Not set up.' } }) });
    history.replaceState(null, '', '/workshop#suggested');
    render(<App data={buildData()} />);
    await userEvent.type(screen.getByRole('textbox', { name: 'Title' }), 'Kept');
    await waitFor(() => expect(document.querySelector('.ws[data-live="failed"]')).not.toBeNull());
    expect(screen.getByRole('textbox', { name: 'Title' })).toHaveValue('Kept');
    expect(screen.getByRole('button', { name: 'Suggest' })).toBeDisabled();
  });
});

describe('the look while waiting', () => {
  it('closes the sign-in tooltip with Escape until the focus leaves, without leaving the place', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#part-b');
    await renderApp();
    const button = screen.getByRole('link', { name: 'Sign in with GitHub' });
    const wrap = button.closest('.ws-signin-wrap') as HTMLElement;
    act(() => button.focus());
    fireEvent.keyDown(button, { key: 'Escape' });
    expect(wrap).toHaveClass('closed');
    expect(location.hash).toBe('#part-b');
    act(() => button.blur());
    await waitFor(() => expect(wrap).not.toHaveClass('closed'));
    fireEvent.pointerEnter(wrap);
    fireEvent.keyDown(document.body, { key: 'Escape' });
    expect(wrap).toHaveClass('closed');
    expect(location.hash).toBe('#part-b');
    fireEvent.pointerLeave(wrap);
    await waitFor(() => expect(wrap).not.toHaveClass('closed'));
    // With the tooltip closed or not shown, Escape goes back as before.
    fireEvent.keyDown(document.body, { key: 'Escape' });
    await waitFor(() => expect(location.hash).toBe(''));
  });
});
