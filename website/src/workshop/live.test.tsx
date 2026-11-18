// The first frame and the load: what the browser remembers, placeholders while nothing is known, the fresh ask
// after an old answer, the retry and "Try again", and verdicts that wait for `me`.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { App, OPEN_GUARD_MS } from './App';
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
    history.replaceState(null, '', '/workshop#computers');
    render(<App data={buildData()} />);
    expect(votes('Create and delete a computer')).toBe('👍 12');
    expect(within(itemOf('Create and delete a computer')).getByLabelText('2 comments')).toBeInTheDocument();
    expect(screen.getByText('polarzero')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Sign out' })).toBeInTheDocument();
    // The owner's words, from memory.
    expect(screen.getByRole('link', { name: /to decide/ })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Decisions' })).toBeInTheDocument();
    expect(within(itemOf('Create and delete a computer')).getByText('to decide')).toBeInTheDocument();
    expect(document.querySelector('.ws-ph')).toBeNull();
  });

  it('has a placeholder where numbers will come while nothing is remembered, and none for what may not exist', () => {
    marker();
    const answer = held<{ status: number; body: unknown }>();
    mockApi({ state: () => answer.promise, routes: { 'GET me': () => answer.promise } });
    history.replaceState(null, '', '/workshop#computers');
    render(<App data={buildData()} />);
    const rows = document.querySelectorAll('.ws-detail .ws-item');
    expect(rows.length).toBe(2);
    for (const row of rows) {
      const slot = row.querySelector('.ws-votes') as HTMLElement;
      expect(slot.querySelector('.ws-ph-vote')).toHaveAttribute('aria-hidden', 'true');
      expect(slot).toHaveAttribute('aria-busy', 'true');
      expect(row.querySelector('.ws-talks')?.textContent).toBe('');
    }
    // The header slot holds the room of someone signed in.
    expect(document.querySelector('.ws-account-inline .ws-ph-account')).toBeInTheDocument();
    expect(screen.queryByText(/to decide/)).not.toBeInTheDocument();
  });

  it('holds the suggestions\' place with one ghost row, and says offline only for a failed load', async () => {
    const answer = held<{ status: number; body: unknown }>();
    mockApi({ state: () => answer.promise });
    history.replaceState(null, '', '/workshop#suggested');
    render(<App data={buildData()} />);
    const section = screen.getByRole('region', { name: 'Suggested by people' });
    expect(section).toHaveAttribute('aria-busy', 'true');
    expect(section.querySelectorAll('.ws-ph-row')).toHaveLength(1);
    expect(section.querySelector('.ws-ph-row .ws-votes .ws-ph-vote')).toBeInTheDocument();
    expect(screen.queryByText('Suggestions are offline.')).not.toBeInTheDocument();
    // Signed out: "Sign in to suggest" at once.
    expect(screen.getByRole('link', { name: 'Sign in to suggest' })).toBeInTheDocument();
    await act(async () => answer.release(ok(liveState())));
    expect(await screen.findByRole('button', { name: /^Share a base computer/ })).toBeInTheDocument();
    expect(section.querySelector('.ws-ph-row')).toBeNull();
    expect(section).not.toHaveAttribute('aria-busy');
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
    history.replaceState(null, '', '/workshop#computers');
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
    history.replaceState(null, '', '/workshop#computers');
    const first = await renderApp();
    await userEvent.click(screen.getByRole('button', { name: '12 votes' }));
    await waitFor(() => expect(posts(calls, 'vote')).toHaveLength(1));
    await waitFor(() => expect(votes('Create and delete a computer')).toBe('👍 13'));
    first.unmount();
    const answer = held<{ status: number; body: unknown }>();
    mockApi({ state: () => answer.promise, me: owner });
    render(<App data={buildData()} />);
    expect(votes('Create and delete a computer')).toBe('👍 13');
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
          ? ok(liveState({ 12: issue({ entry: 'computers', title: 'Create and delete a computer', up: 30, comments: 5, updates: 3 }) }))
          : ok({ ...liveState(), fetchedAt: at(Date.now() - 1000), old: true });
      },
    });
    history.replaceState(null, '', '/workshop#computers');
    await renderApp();
    await waitFor(() => expect(votes('Create and delete a computer')).toBe('👍 30'));
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
    remember(liveState({ 12: issue({ entry: 'computers', title: 'Create and delete a computer', up: 40, comments: 5, updates: 3 }) }));
    const calls = mockApi({
      state: (url) => (url.searchParams.get('fresh') === '1'
        ? { status: 502, body: { error: 'github', message: 'GitHub failed.' } }
        : ok({ ...liveState(), fetchedAt: at(Date.now() - 3600_000), old: true })),
    });
    history.replaceState(null, '', '/workshop#computers');
    await renderApp();
    await waitFor(() => expect(calls.filter((call) => call.path === 'state')).toHaveLength(2));
    await act(async () => {});
    expect(votes('Create and delete a computer')).toBe('👍 40');
    expect(screen.queryByText('Votes and comments are offline.')).not.toBeInTheDocument();
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('tries once more after a failure, then offers "Try again", which loads', async () => {
    loadConfig.retryMs = 150;
    let fail = true;
    const calls = mockApi({ state: () => (fail ? { status: 502, body: { error: 'github', message: 'GitHub failed.' } } : ok(liveState())) });
    history.replaceState(null, '', '/workshop#computers');
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
    await waitFor(() => expect(votes('Create and delete a computer')).toBe('👍 12'));
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
    history.replaceState(null, '', '/workshop#computers');
    await renderApp();
    expect(votes('Create and delete a computer')).toBe('👍 12');
    expect(screen.getAllByText('Votes and comments are offline.').length).toBeGreaterThan(0);
  });
});

describe('the page\'s own writes over an answer', () => {
  const computers = (up: number, comments = 5) => liveState({ 12: issue({ entry: 'computers', title: 'Create and delete a computer', up, comments, updates: 3 }) });
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
      history.replaceState(null, '', '/workshop#computers');
      const first = render(<App data={buildData()} />);
      expect(votes('Create and delete a computer')).toBe('👍 12');
      fireEvent.click(screen.getByRole('button', { name: '12 votes' }));
      await waitFor(() => expect(posts(calls, 'vote')).toHaveLength(1));
      // Without a count from the server, the count shown moves by one.
      const kept = server ? '👍 21' : '👍 13';
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
      await waitFor(() => expect(votes('Create and delete a computer')).toBe('👍 23'));
    });
  }

  it('keep the comment count the server answered, the whole count with history lines', async () => {
    const time = clock();
    marker();
    remember({ ...computers(12, 5), fetchedAt: at(Date.now() - 86_400_000) }, visitor);
    const cdn = held<{ status: number; body: unknown }>();
    const fresh = held<{ status: number; body: unknown }>();
    mockApi({ me: visitor, state: (url) => (url.search ? fresh.promise : cdn.promise), routes: { 'POST comment': () => ok({ ok: true, id: 1, comments: 9 }) } });
    history.replaceState(null, '', '/workshop#computers/computers');
    const first = render(<App data={buildData()} />);
    // 5 comments, 3 of them history lines.
    expect(within(itemOf('Create and delete a computer')).getByLabelText('2 comments')).toBeInTheDocument();
    await waitFor(() => expect(screen.getByRole('textbox', { name: 'Comment' })).toBeEnabled());
    await userEvent.type(screen.getByRole('textbox', { name: 'Comment' }), 'Mine.');
    await userEvent.click(screen.getByRole('button', { name: 'Comment' }));
    await waitFor(() => expect(within(itemOf('Create and delete a computer')).getByLabelText('6 comments')).toBeInTheDocument());
    await act(async () => cdn.release(ok({ ...computers(12, 8), fetchedAt: at(Date.now() - 5 * 60_000) })));
    expect(within(itemOf('Create and delete a computer')).getByLabelText('6 comments')).toBeInTheDocument();
    // Seconds later, an answer that already counts it: still 6.
    time.advance(5_000);
    await act(async () => fresh.release(ok({ ...computers(12, 9), fetchedAt: at(Date.now()) })));
    await waitFor(() => expect(stored().comments).toBe(9));
    expect(within(itemOf('Create and delete a computer')).getByLabelText('6 comments')).toBeInTheDocument();
    first.unmount();
    // Over a minute later, the server's own count.
    time.advance(2 * 60_000);
    mockApi({ me: visitor, state: () => ok({ ...computers(12, 11), fetchedAt: at(Date.now()) }) });
    render(<App data={buildData()} />);
    await waitFor(() => expect(within(itemOf('Create and delete a computer')).getByLabelText('8 comments')).toBeInTheDocument());
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
      const writes: { at: number; up?: number; verdict?: 'take' | 'later' }[] = [];
      for (let step = 0; step < 40; step += 1) {
        const roll = random();
        if (roll < 0.2) {
          const up = Math.floor(random() * 50);
          writes.push({ at: Date.now(), up });
          keep({ n: 12, set: { up } }, Date.now());
        } else if (roll < 0.3) {
          const verdict = random() < 0.5 ? 'take' : 'later';
          writes.push({ at: Date.now(), verdict });
          keep({ n: 12, set: { verdict } }, Date.now());
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
        let verdict: 'take' | 'later' | null = null;
        for (const write of writes) {
          if (Date.now() - write.at >= 24 * 3600_000 || model.fetched > write.at + 60_000) continue;
          if (write.up !== undefined) up = write.up;
          if (write.verdict) verdict = write.verdict;
        }
        const shown = applyKept(kept, Date.now());
        expect(shown.issues[12].up).toBe(up);
        expect(shown.issues[12].verdict).toBe(verdict);
        expect(applyKept(shown, Date.now())).toEqual(shown);
      }
      vi.restoreAllMocks();
    }
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
});

describe('verdicts and `me`', () => {
  it('are not sent while only memory says the person decides, and are once `me` answered', async () => {
    const time = clock();
    marker();
    remember(liveState(), owner);
    const me = held<{ status: number; body: unknown }>();
    const calls = mockApi({ routes: { 'GET me': () => me.promise } });
    history.replaceState(null, '', '/workshop#computers/computers');
    render(<App data={buildData()} />);
    // The state is remembered: the row opens at once.
    expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'true');
    await waitFor(() => expect(document.querySelector('.ws[data-live="answered"]')).not.toBeNull());
    time.advance(OPEN_GUARD_MS);
    const take = screen.getByRole('button', { name: /^Take/ });
    expect(take).toHaveClass('ws-unarmed');
    fireEvent.click(take);
    fireEvent.keyDown(rowButton('Create and delete a computer'), { key: 'Enter' });
    expect(posts(calls, 'verdict')).toHaveLength(0);
    await act(async () => me.release(ok(owner)));
    await waitFor(() => expect(screen.getByRole('button', { name: /^Take/ })).not.toHaveClass('ws-unarmed'));
    fireEvent.click(screen.getByRole('button', { name: /^Take/ }));
    expect(posts(calls, 'verdict').map((call) => call.body)).toEqual([{ n: 12, verdict: 'take' }]);
  });

  it('say nothing on page load when the session is gone: the vote check waits for `me`', async () => {
    marker();
    remember(liveState(), visitor);
    const unauthorized = () => ({ status: 401, body: { error: 'unauthorized', message: 'Sign in again.' } });
    const calls = mockApi({ routes: { 'GET me': unauthorized, 'GET vote': unauthorized } });
    history.replaceState(null, '', '/workshop#computers/computers');
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
    mockApi({ me: { login: 'mara-k', avatar: '', canDecide: false } });
    history.replaceState(null, '', '/workshop#computers');
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
    history.replaceState(null, '', '/workshop#computers/computers');
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
    expect(part('engine')).toBeInTheDocument();
  });

  it('opens late beside the row the person opened meanwhile, which stays the current one', async () => {
    let release: (value: { status: number; body: unknown }) => void = () => {};
    mockApi({ state: () => new Promise((resolve) => { release = resolve; }) });
    history.replaceState(null, '', '/workshop#computers/exec');
    render(<App data={buildData()} />);
    await userEvent.click(rowButton('Create and delete a computer'));
    expect(location.hash).toBe('#computers/computers');
    await act(async () => release(ok(liveState())));
    await waitFor(() => expect(rowButton('Run a command')).toHaveAttribute('aria-expanded', 'true'));
    expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'true');
    expect(location.hash).toBe('#computers/computers');
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
  it('holds the suggest form with the form\'s own boxes while `me` is asked, out of reach', () => {
    marker();
    const answer = held<{ status: number; body: unknown }>();
    mockApi({ state: () => answer.promise, routes: { 'GET me': () => answer.promise } });
    history.replaceState(null, '', '/workshop#suggested');
    render(<App data={buildData()} />);
    const shape = document.querySelector('.ws-form.ws-ph-shape') as HTMLElement;
    expect(shape).toHaveAttribute('aria-hidden', 'true');
    expect(shape.parentElement).toHaveAttribute('aria-busy', 'true');
    expect(shape.querySelectorAll('input, textarea')).toHaveLength(2);
    expect(shape.querySelector('textarea')).toHaveAttribute('rows', '2');
    for (const field of shape.querySelectorAll('input, textarea')) expect(field).toHaveAttribute('tabindex', '-1');
    expect(screen.queryByRole('textbox', { name: 'Title' })).not.toBeInTheDocument();
  });

  it('opens an entry before the answer with "Sign in to comment" and the room of the line under it, signed out', async () => {
    const answer = held<{ status: number; body: unknown }>();
    mockApi({ state: () => answer.promise });
    history.replaceState(null, '', '/workshop#computers');
    render(<App data={buildData()} />);
    fireEvent.click(rowButton('Create and delete a computer'));
    const item = itemOf('Create and delete a computer');
    expect(within(item).getByRole('link', { name: 'Sign in to comment' })).toBeInTheDocument();
    expect(item.querySelector('.ws-gh-alt.ws-held')).toHaveAttribute('aria-hidden', 'true');
    await act(async () => answer.release(ok(liveState())));
    expect(await within(item).findByRole('link', { name: 'or comment directly on GitHub' })).toBeInTheDocument();
    expect(item.querySelector('.ws-held')).toBeNull();
  });

  it('holds the comment box with its own boxes when someone may be signed in', () => {
    marker();
    const answer = held<{ status: number; body: unknown }>();
    mockApi({ state: () => answer.promise, routes: { 'GET me': () => answer.promise } });
    history.replaceState(null, '', '/workshop#computers');
    render(<App data={buildData()} />);
    fireEvent.click(rowButton('Create and delete a computer'));
    const shape = itemOf('Create and delete a computer').querySelector('.ws-box.ws-ph-shape') as HTMLElement;
    expect(shape.querySelector('textarea')).toHaveAttribute('rows', '1');
    expect(shape.querySelector('.ws-btn')).toHaveTextContent('Comment');
  });

  it('closes the sign-in tooltip with Escape until the focus leaves, without leaving the place', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#computers');
    await renderApp();
    const button = screen.getByRole('link', { name: 'Sign in with GitHub' });
    const wrap = button.closest('.ws-signin-wrap') as HTMLElement;
    act(() => button.focus());
    fireEvent.keyDown(button, { key: 'Escape' });
    expect(wrap).toHaveClass('closed');
    expect(location.hash).toBe('#computers');
    act(() => button.blur());
    await waitFor(() => expect(wrap).not.toHaveClass('closed'));
    fireEvent.pointerEnter(wrap);
    fireEvent.keyDown(document.body, { key: 'Escape' });
    expect(wrap).toHaveClass('closed');
    expect(location.hash).toBe('#computers');
    fireEvent.pointerLeave(wrap);
    await waitFor(() => expect(wrap).not.toHaveClass('closed'));
    // With the tooltip closed or not shown, Escape goes back as before.
    fireEvent.keyDown(document.body, { key: 'Escape' });
    await waitFor(() => expect(location.hash).toBe(''));
  });
});
