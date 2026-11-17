// The first frame and the load: what the browser remembers, placeholders while nothing is known, the fresh ask
// after an old answer, the retry and "Try again", and verdicts that wait for `me`.
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { App, OPEN_GUARD_MS } from './App';
import type { Me } from './api';
import { buildData, issue, liveState } from './fixtures';
import type { LiveState } from './plan';
import { clock, itemOf, mockApi, ok, owner, part, posts, renderApp, rowButton } from './test-support';
import { loadConfig } from './useWorkshop';
import { afterAnswer, keep } from './writes';

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

  it('holds the suggestions\' place with two ghost rows, and says offline only for a failed load', async () => {
    const answer = held<{ status: number; body: unknown }>();
    mockApi({ state: () => answer.promise });
    history.replaceState(null, '', '/workshop#suggested');
    render(<App data={buildData()} />);
    const section = screen.getByRole('region', { name: 'Suggested by people' });
    expect(section).toHaveAttribute('aria-busy', 'true');
    expect(section.querySelectorAll('.ws-latest-ghost')).toHaveLength(2);
    expect(screen.queryByText('Suggestions are offline.')).not.toBeInTheDocument();
    // Signed out: "Sign in to suggest" at once.
    expect(screen.getByRole('link', { name: 'Sign in to suggest' })).toBeInTheDocument();
    await act(async () => answer.release(ok(liveState())));
    expect(await screen.findByRole('button', { name: /^Share a base computer/ })).toBeInTheDocument();
    expect(section.querySelector('.ws-latest-ghost')).toBeNull();
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
    await renderApp();
    expect(screen.queryByText('polarzero')).not.toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Sign in with GitHub' })).toBeInTheDocument();
    marker();
    localStorage.setItem(PERSON, JSON.stringify(owner));
    mockApi({ routes: { 'GET me': () => ({ status: 401, body: { error: 'unauthorized' } }) } });
    document.body.innerHTML = '';
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

  it('remembers what it showed, its own writes applied, for the next visit', async () => {
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
  it('win over a state fetched before them, not over one fetched more than a minute after', async () => {
    const time = clock();
    keep(12, { up: 13 }, Date.now());
    const base = liveState();
    const before = afterAnswer(null, { ...base, fetchedAt: at(Date.now() - 5 * 60_000) }, Date.now());
    expect(before.issues[12].up).toBe(13);
    const within = afterAnswer(null, { ...base, fetchedAt: at(Date.now() + 50_000) }, Date.now() + 50_000);
    expect(within.issues[12].up).toBe(13);
    time.advance(2 * 60_000);
    const after = afterAnswer(null, { ...base, fetchedAt: at(Date.now()) }, Date.now());
    expect(after.issues[12].up).toBe(12);
    // A day later the write is gone, whatever the state.
    time.advance(24 * 3600_000);
    expect(afterAnswer(null, { ...base, fetchedAt: at(0) }, Date.now()).issues[12].up).toBe(12);
  });

  it('hold on every path: random answers, writes and time, never back in time, own writes until a newer state', () => {
    // The reference: the state shown is the newest answer accepted; issue 12 shows the page's own last write when
    // that answer was fetched no later than a minute after it and the write is under a day old, else the answer's.
    // A write also changes the state shown at once, as the page does.
    let seed = 7;
    const random = () => { seed = (seed * 1103515245 + 12345) % 2 ** 31; return seed / 2 ** 31; };
    for (let run = 0; run < 50; run += 1) {
      localStorage.clear();
      const time = clock();
      let shown = null as LiveState | null;
      let model = null as { fetched: number; up: number } | null;
      let write = null as { at: number; up: number } | null;
      for (let step = 0; step < 40; step += 1) {
        const roll = random();
        if (roll < 0.25) {
          write = { at: Date.now(), up: 100 + Math.floor(random() * 100) };
          keep(12, { up: write.up }, Date.now());
          if (shown && model) {
            shown = { ...shown, issues: { ...shown.issues, 12: { ...shown.issues[12], up: write.up } } };
            model = { ...model, up: write.up };
          }
        } else if (roll < 0.75) {
          const fetched = Date.now() - Math.floor(random() * 10 * 60_000) + Math.floor(random() * 3 * 60_000);
          const serverUp = Math.floor(random() * 100);
          const answer = liveState({ 12: issue({ entry: 'computers', title: 'Create and delete a computer', up: serverUp }) });
          shown = afterAnswer(shown, { ...answer, fetchedAt: at(fetched), old: random() < 0.5 }, Date.now());
          if (!model || fetched >= model.fetched) {
            const own = write && fetched <= write.at + 60_000 && Date.now() - write.at < 24 * 3600_000;
            model = { fetched, up: own && write ? write.up : serverUp };
          }
        } else {
          time.advance(Math.floor(random() * (random() < 0.1 ? 30 * 3600_000 : 4 * 60_000)));
        }
        if (!shown || !model) continue;
        expect(Date.parse(shown.fetchedAt)).toBe(model.fetched);
        expect(shown.issues[12].up).toBe(model.up);
        expect(shown.old).toBeUndefined();
      }
      vi.restoreAllMocks();
    }
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
});
