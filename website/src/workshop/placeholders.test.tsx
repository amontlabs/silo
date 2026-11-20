// One check of the placeholders shown while the page waits for an answer: each answer is held in turn (state, me, an
// issue's comments), and every place that waits on it is visited. While held, a placeholder is hidden from screen
// readers, sits in a container that says it is busy and holds nothing focusable; once the answers come, none is left
// and nothing says busy.
import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { App } from './App';
import type { Comment } from './api';
import { buildData, issue, liveState } from './fixtures';
import { LATEST_GHOSTS } from './Latest';
import { detail, itemOf, mockApi, ok, owner, rowButton } from './test-support';
import type { Reply } from './test-support';

// The placeholders' class names, read from the code that draws them: ws-ph (and ws-ph-*), a ghost, a held place.
const dir = join(process.cwd(), 'src/workshop');
const sources = readdirSync(dir).filter((file) => /\.tsx$/.test(file) && !/\.test\.|^test-support/.test(file));
const NAME = /\bws-(?:ph(?:-[a-z]+)?|[a-z]+-ghost|held)\b/g;
const classes = [...new Set(sources.flatMap((file) => readFileSync(join(dir, file), 'utf8').match(NAME) ?? []))];
const PLACEHOLDER = classes.map((name) => `.${name}`).join(',');

/** A reply that waits until `release` is called. */
function held() {
  let release: (answer: Awaited<Reply>) => void = () => {};
  const promise = new Promise<Awaited<Reply>>((resolve) => { release = resolve; });
  return { promise, release };
}

const thread: Comment[] = [
  { id: 1, author: 'mara-k', avatar: '', bot: false, createdAt: '2026-11-10T10:00:00Z', text: 'One.' },
  { id: 2, author: 'mara-k', avatar: '', bot: false, createdAt: '2026-11-12T10:00:00Z', text: 'Two.' },
];

const focusable = (element: Element) => [element, ...element.querySelectorAll('*')].filter((node) =>
  node.matches('a[href], button, input, select, textarea, summary, [contenteditable], [tabindex]')
  && !node.matches(':disabled') && node.getAttribute('tabindex') !== '-1' && !node.closest('[inert]'));

type Holds = { state?: boolean; me?: boolean; issue?: boolean };
type Visit = { name: string; hash: string; opens?: string; holds: Holds; signedIn: boolean };

// Each place that waits, with the answers it waits on. The marker (signed in before) makes `me` a question to ask.
const VISITS: Visit[] = [
  { name: 'rows and an open entry, state and me asked', hash: '#part-b', opens: 'Create and delete a computer', holds: { state: true, me: true }, signedIn: true },
  { name: 'suggestions, state and me asked', hash: '#suggested', holds: { state: true, me: true }, signedIn: true },
  { name: 'Latest, state and me asked', hash: '#latest', holds: { state: true, me: true }, signedIn: true },
  { name: 'rows and an open entry, state asked, signed out', hash: '#part-b', opens: 'Create and delete a computer', holds: { state: true }, signedIn: false },
  { name: 'suggestions, state asked, signed out', hash: '#suggested', holds: { state: true }, signedIn: false },
  { name: 'an open entry with no thread, state and me asked', hash: '#part-d', opens: 'One silo command', holds: { state: true, me: true }, signedIn: true },
  { name: 'an open entry with no thread, state asked, signed out', hash: '#part-d', opens: 'One silo command', holds: { state: true }, signedIn: false },
  { name: 'Latest, state asked, signed out', hash: '#latest', holds: { state: true }, signedIn: false },
  { name: 'suggest form, only me asked', hash: '#suggested', holds: { me: true }, signedIn: true },
  { name: 'rows, only me asked', hash: '#part-b', holds: { me: true }, signedIn: true },
  { name: 'the thread of an open entry, only its comments asked', hash: '#part-b', opens: 'Create and delete a computer', holds: { issue: true }, signedIn: true },
];

/** Between two visits in one test: what the first one remembered (the answer, the sign-in marker) must not reach the next. */
function leave() {
  cleanup();
  localStorage.clear();
  document.cookie = 'workshop-hint=; expires=Thu, 01 Jan 1970 00:00:00 GMT; path=/';
}

const visitOf = (name: string) => VISITS.find((visit) => visit.name.startsWith(name)) as Visit;

const placeholders = () => [...document.querySelectorAll(PLACEHOLDER)].filter((element) => !element.hasAttribute('aria-busy'));

/** Renders the page with the given answers held; `release` lets them all come. */
async function hold(visit: Visit) {
  const state = held();
  const me = held();
  const comments = held();
  mockApi({
    state: () => (visit.holds.state ? state.promise : ok(liveState())),
    routes: {
      'GET me': () => (visit.holds.me ? me.promise : ok(owner)),
      'GET issue': (url) => (visit.holds.issue ? comments.promise : ok(detail(Number(url.searchParams.get('n')), { comments: thread }))),
    },
  });
  if (visit.signedIn) document.cookie = 'workshop-hint=1; path=/';
  history.replaceState(null, '', `/workshop${visit.hash}`);
  // Like the page: the account is drawn in a slot of the header, which the page marks busy.
  const slot = document.body.appendChild(document.createElement('div'));
  render(<App data={buildData()} accountSlot={slot} />);
  await waitFor(() => expect(document.getElementById('ws-part-title')).not.toBeNull());
  if (!visit.holds.state) await waitFor(() => expect(document.querySelector('.ws[data-live="answered"]')).not.toBeNull());
  if (visit.opens) fireEvent.click(rowButton(visit.opens));
  const release = async () => {
    await act(async () => {
      state.release(ok(liveState({
        10: issue({ entry: 'core', title: 'The engine runs on its own', up: 8, comments: 5 }),
        40: issue({ entry: 'core-in-zig', title: 'Which language is the core written in?', up: 6, comments: 4 }),
        41: issue({ entry: 'calls', title: 'Every call defined once', up: 3, comments: 2 }),
      })));
      me.release(ok(owner));
      comments.release(ok(detail(12, { comments: thread })));
    });
    await waitFor(() => expect(document.querySelector('.ws[data-live="answered"]')).not.toBeNull());
    slot.remove();
  };
  return release;
}

describe('placeholders while the page waits', () => {
  it('reads the placeholder class names from the code', () => {
    expect(classes).toEqual(expect.arrayContaining(['ws-ph', 'ws-ph-vote', 'ws-ph-account', 'ws-ph-shape', 'ws-ph-row', 'ws-h-ghost', 'ws-c-ghost', 'ws-latest-ghost', 'ws-held']));
  });

  for (const visit of VISITS) {
    it(`hides, marks busy and keeps out of reach every placeholder, then leaves none: ${visit.name}`, async () => {
      const release = await hold(visit);
      const found = placeholders();
      expect(found.length, 'a placeholder is shown').toBeGreaterThan(0);
      for (const element of found) {
        const label = element.outerHTML.slice(0, 80);
        expect(element.closest('[aria-hidden="true"]'), `hidden from screen readers: ${label}`).not.toBeNull();
        expect(element.closest('[aria-busy="true"]'), `in a container that says it is busy: ${label}`).not.toBeNull();
        expect(focusable(element).map((node) => node.outerHTML.slice(0, 60)), `nothing focusable in ${label}`).toEqual([]);
      }
      await release();
      await waitFor(() => {
        expect(placeholders().map((element) => element.outerHTML.slice(0, 60))).toEqual([]);
        expect([...document.querySelectorAll('[aria-busy="true"]')].map((element) => element.outerHTML.slice(0, 60))).toEqual([]);
      });
    });
  }

  it('shows every placeholder the code draws, in at least one of those places', async () => {
    const all = new Set<string>();
    for (const visit of VISITS) {
      const release = await hold(visit);
      for (const element of placeholders()) for (const name of classes) if (element.classList.contains(name)) all.add(name);
      await release();
      leave();
    }
    // The notes' and the history's placeholders wait on a chunk, not on an answer, and are held by a busy container of their own.
    expect(classes.filter((name) => !all.has(name) && name !== 'ws-ph-notes' && name !== 'ws-h-ghost')).toEqual([]);
  });

  it('holds one place per number, per line of the thread and per line of Latest', async () => {
    // Numbers: one placeholder per vote that arrives.
    let release = await hold(visitOf('rows and an open entry, state and me'));
    const slots = document.querySelectorAll('.ws-detail .ws-item .ws-ph-vote').length;
    await release();
    expect(slots).toBeGreaterThan(0);
    expect(document.querySelectorAll('.ws-detail .ws-item .ws-vote')).toHaveLength(slots);
    leave();
    // The thread: one per comment it will hold.
    release = await hold(visitOf('the thread'));
    const item = itemOf('Create and delete a computer');
    const ghosts = item.querySelectorAll('.ws-c-ghost').length;
    await release();
    await waitFor(() => expect(item.querySelectorAll('.ws-c').length).toBeGreaterThan(0));
    expect(ghosts).toBe(item.querySelectorAll('.ws-c').length);
    leave();
    // Latest: the number the code declares.
    release = await hold(visitOf('Latest, state and me'));
    expect(document.querySelectorAll('.ws-latest-ghost')).toHaveLength(LATEST_GHOSTS);
    expect(screen.queryByText('Nothing yet.')).not.toBeInTheDocument();
    await release();
  });

  it('keeps "Sign in to suggest" and "Sign in to comment" at hand while signed out, and says offline only for a failed load', async () => {
    const release = await hold(visitOf('suggestions, state asked'));
    expect(screen.getByRole('link', { name: 'Sign in to suggest' })).toBeInTheDocument();
    expect(screen.queryByText('Suggestions are offline.')).not.toBeInTheDocument();
    await release();
    expect(await screen.findByRole('button', { name: /^Share a base computer/ })).toBeInTheDocument();
    leave();
    const open = await hold(visitOf('rows and an open entry, state asked'));
    expect(screen.getByRole('link', { name: 'Sign in to comment' })).toBeInTheDocument();
    await open();
    expect(await screen.findByRole('link', { name: 'or comment directly on GitHub' })).toBeInTheDocument();
  });
});

describe('a row with no thread yet', () => {
  const NONE = 'One silo command';

  it('holds the room of its vote while the state is asked, then shows a vote of 0, with no link to GitHub', async () => {
    document.cookie = 'workshop-hint=1; path=/';
    const state = held();
    const me = held();
    mockApi({ state: () => state.promise, routes: { 'GET me': () => me.promise } });
    history.replaceState(null, '', '/workshop#part-d');
    render(<App data={buildData()} />);
    await waitFor(() => expect(rowButton(NONE)).toBeInTheDocument());
    fireEvent.click(rowButton(NONE));
    const item = itemOf(NONE);
    expect(item.querySelector('.ws-ph-vote')).not.toBeNull();
    expect(item.querySelector('.ws-votes')).toHaveAttribute('aria-busy', 'true');
    expect(item.querySelector('.ws-ph-shape')).not.toBeNull();
    expect(item.querySelector('a[href*="github.com"]')).toBeNull();
    await act(async () => {
      state.release(ok(liveState()));
      me.release(ok(owner));
    });
    await waitFor(() => expect(document.querySelector('.ws')).toHaveAttribute('data-live', 'answered'));
    await waitFor(() => expect(within(item).getByRole('button', { name: /0 votes/ })).toBeInTheDocument());
    expect(item.querySelector(PLACEHOLDER)).toBeNull();
    expect(within(item).getByText('No comments yet.')).toBeInTheDocument();
    expect(item.querySelector('a[href*="github.com"]')).toBeNull();
  });
});
