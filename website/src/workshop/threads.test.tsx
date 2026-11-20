// Every decision and every feature has a thread, opened by the first comment or 👍; comments carry votes.
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { apiConfig } from './api';
import type { Comment } from './api';
import { App } from './App';
import { buildData, issue, liveState, withLists } from './fixtures';
import type { BuildData } from './plan';
import { clock, detail, itemOf, mockApi, ok, owner, posts, refused, renderApp, rowButton, visitor, wait } from './test-support';
import type { Route } from './test-support';

// `leak-guard` (a feature, "Stop what leaks") and `name-a-snapshot` (a decision that sets statements) have no thread yet.
const GUARD = 'Stop what leaks';
const SNAPSHOT = 'What is a saved moment of a computer called?';

const comment = (id: number, author: string, text: string, up = 0): Comment => ({ id, author, avatar: '', bot: false, createdAt: '2026-11-14T10:00:00Z', text, up });
const login = (hash: string) => `/api/workshop/login?return=${encodeURIComponent(`/workshop${hash}`)}`;

async function open(hash: string, name: string, options: Parameters<typeof mockApi>[0] = {}, data: BuildData = buildData(withLists())) {
  const calls = mockApi(options);
  history.replaceState(null, '', `/workshop${hash}`);
  await renderApp(data);
  await userEvent.click(rowButton(name));
  return { calls, item: itemOf(name) };
}

describe('a row with no thread yet', () => {
  it('shows a vote of 0, "No comments yet." and no link to GitHub, signed out', async () => {
    const { item } = await open('#part-b', GUARD);
    expect(within(item).getByRole('link', { name: /0 votes/ })).toHaveAttribute('href', login('#part-b/leak-guard'));
    expect(within(item).getByText('No comments yet.')).toBeInTheDocument();
    expect(await within(item).findByRole('link', { name: 'Sign in to comment' })).toBeInTheDocument();
    expect(item.querySelectorAll('a[href*="github.com"]')).toHaveLength(0);
    expect(within(item).queryByRole('link', { name: /directly on GitHub/ })).toBeNull();
    // The line is above the box.
    const talk = item.querySelector('.ws-talk') as HTMLElement;
    expect(talk.children[0]).toHaveTextContent('No comments yet.');
  });

  it('shows the same on a decision, whatever its state', async () => {
    const { item } = await open('#part-b', SNAPSHOT);
    expect(within(item).getByRole('link', { name: /0 votes/ })).toBeInTheDocument();
    expect(within(item).getByText('No comments yet.')).toBeInTheDocument();
    expect(screen.queryByText('This thread is closed on GitHub.')).toBeNull();
  });

  it('shows a vote of 0 and the comment box, signed in, still with no link to GitHub', async () => {
    const { item, calls } = await open('#part-b', GUARD, { me: owner });
    expect(within(item).getByRole('button', { name: /0 votes/ })).toHaveAttribute('aria-pressed', 'false');
    expect(within(item).getByText('No comments yet.')).toBeInTheDocument();
    expect(await within(item).findByRole('textbox', { name: 'Comment' })).toBeEnabled();
    expect(item.querySelectorAll('a[href*="github.com"]')).toHaveLength(0);
    // Nothing is asked of GitHub for a thread that does not exist: no issue, and no vote of the person's.
    expect(calls.filter((call) => call.path === 'issue' || call.path === 'vote')).toHaveLength(0);
  });

  it('takes its thread from the first comment, sent with the entry, and shows the comment once', async () => {
    const { item, calls } = await open('#part-b', GUARD, {
      me: owner,
      routes: { 'POST comment': () => ok({ ok: true, id: 901, comments: 1, n: 40 }) },
    });
    await userEvent.type(await within(item).findByRole('textbox', { name: 'Comment' }), 'Does it cover files?');
    const talk = item.querySelector('.ws-talk') as HTMLElement;
    const box = talk.querySelector('form') as HTMLElement;
    await userEvent.click(within(item).getByRole('button', { name: 'Comment' }));
    await waitFor(() => expect(within(item).getByText('Does it cover files?')).toBeInTheDocument());
    expect(posts(calls, 'comment')).toHaveLength(1);
    expect(posts(calls, 'comment')[0].body).toEqual({ entry: 'leak-guard', text: 'Does it cover files?' });
    expect(within(item).getAllByText('Does it cover files?')).toHaveLength(1);
    expect(within(item).getByText('💬 1')).toBeInTheDocument();
    expect(within(item).queryByText('No comments yet.')).toBeNull();
    // The row has its thread: the link to the issue is where the room for it was, and the panel is the same one.
    expect(within(item).getByRole('link', { name: 'GitHub #40 ↗' })).toHaveAttribute('href', 'https://github.com/amontlabs/silo/issues/40');
    expect(item.querySelector('.ws-talk form')).toBe(box);
    // The thread it took is read, as on any opening.
    await waitFor(() => expect(calls.filter((call) => call.path === 'issue')).toHaveLength(1));
    // The next comment names the entry too: the server finds the thread.
    await userEvent.type(within(item).getByRole('textbox', { name: 'Comment' }), 'And folders?');
    await userEvent.click(within(item).getByRole('button', { name: 'Comment' }));
    await waitFor(() => expect(posts(calls, 'comment')).toHaveLength(2));
    expect(posts(calls, 'comment')[1].body).toEqual({ entry: 'leak-guard', text: 'And folders?' });
  });

  it('takes its thread from the first 👍, sent with the entry, and counts it at once', async () => {
    const { item, calls } = await open('#part-b', GUARD, { me: owner, routes: { 'POST vote': () => ok({ ok: true, up: 1, n: 41 }) } });
    await userEvent.click(within(item).getByRole('button', { name: /0 votes/ }));
    const on = await within(item).findByRole('button', { name: /1 votes/ });
    expect(on).toHaveAttribute('aria-pressed', 'true');
    expect(posts(calls, 'vote')[0].body).toEqual({ entry: 'leak-guard', on: true });
    // The 👍 is the person's own on that thread now: the next click removes it.
    await userEvent.click(on);
    await waitFor(() => expect(posts(calls, 'vote')).toHaveLength(2));
    expect(posts(calls, 'vote')[1].body).toEqual({ entry: 'leak-guard', on: false });
    // The person's votes on the thread it took are asked once, with the thread.
    expect(calls.filter((call) => call.path === 'vote' && call.method === 'GET')).toHaveLength(1);
  });

  it('shows a comment sent twice (a double click, a retry after an error) once', async () => {
    let tries = 0;
    const route: Route = () => {
      tries += 1;
      return tries === 1 ? refused(502, 'GitHub did not answer.') : ok({ ok: true, id: 902, comments: 1, n: 42 });
    };
    const { item, calls } = await open('#part-b', GUARD, { me: owner, routes: { 'POST comment': route } });
    const field = await within(item).findByRole('textbox', { name: 'Comment' });
    await userEvent.type(field, 'Once.');
    const send = within(item).getByRole('button', { name: 'Comment' });
    fireEvent.click(send);
    fireEvent.click(send);
    await waitFor(() => expect(screen.getByText(/Comment: not saved/)).toBeInTheDocument());
    expect(posts(calls, 'comment')).toHaveLength(1);
    expect(within(item).getByRole('textbox', { name: 'Comment' })).toHaveValue('Once.');
    fireEvent.click(within(item).getByRole('button', { name: 'Comment' }));
    fireEvent.click(within(item).getByRole('button', { name: 'Comment' }));
    await waitFor(() => expect(within(item).getByText('💬 1')).toBeInTheDocument());
    await waitFor(() => expect(item.querySelector('.ws-c-ghost')).toBeNull());
    expect(within(item).getAllByText('Once.')).toHaveLength(1);
    expect(posts(calls, 'comment')).toHaveLength(2);
    // A second answer for the same words (the server found the comment already posted) adds none.
    expect(within(item).getAllByText('Once.')).toHaveLength(1);
  });

  it('keeps one 👍 while a first vote is on its way', async () => {
    let release: () => void = () => {};
    const { item, calls } = await open('#part-b', GUARD, {
      me: owner,
      routes: { 'POST vote': () => new Promise((resolve) => { release = () => resolve(ok({ ok: true, up: 1, n: 43 })); }) },
    });
    const vote = within(item).getByRole('button', { name: /0 votes/ });
    fireEvent.click(vote);
    fireEvent.click(vote);
    await act(async () => release());
    expect(posts(calls, 'vote')).toHaveLength(1);
  });

  it('is shown again, still with no thread, when the server refuses a first vote', async () => {
    const { item } = await open('#part-b', GUARD, { me: owner, routes: { 'POST vote': () => refused(429, 'Too many new threads were started on the Workshop this hour. Try again later.') } });
    await userEvent.click(within(item).getByRole('button', { name: /0 votes/ }));
    expect(await screen.findByText('Vote: not saved. Too many new threads were started on the Workshop this hour. Try again later.')).toBeInTheDocument();
    expect(within(item).getByRole('button', { name: /0 votes/ })).toHaveAttribute('aria-pressed', 'false');
    expect(within(item).getByText('No comments yet.')).toBeInTheDocument();
  });

  it('takes its thread when the state shown already has it, as it does any row\'s', async () => {
    const { item } = await open('#part-b', GUARD, { me: owner, state: () => ok(liveState({ 50: issue({ entry: 'leak-guard', title: GUARD, up: 5, comments: 1 }) })) });
    expect(within(item).getByRole('button', { name: /5 votes/ })).toBeInTheDocument();
    expect(within(item).getByText('💬 1')).toBeInTheDocument();
    expect(await within(item).findByRole('link', { name: 'GitHub #50 ↗' })).toBeInTheDocument();
  });

  it('keeps the room of the link to GitHub, so a thread arriving under an open panel moves nothing', async () => {
    const { item } = await open('#part-b', GUARD);
    const held = item.querySelector('.ws-talk .ws-gh-alt.ws-room');
    expect(held).not.toBeNull();
    expect(held).toHaveAttribute('aria-hidden', 'true');
  });

  it('keeps the room of the link to GitHub inside the comment box, signed in', async () => {
    const { item } = await open('#part-b', GUARD, { me: owner });
    const held = await waitFor(() => item.querySelector('.ws-talk form .ws-gh.ws-room') as HTMLElement);
    expect(held).toHaveAttribute('aria-hidden', 'true');
  });
});

describe('a feature written from a suggestion', () => {
  const state = () => ok(liveState(
    { 31: issue({ kind: 'suggestion', title: 'Share a base computer', up: 14, comments: 1, author: 'mara-k', bot: false }) },
    [],
    { fork: 31 },
  ));

  it('shows once, as the feature, with that issue as its thread', async () => {
    history.replaceState(null, '', '/workshop#suggested');
    mockApi({ state });
    await renderApp();
    // Not among the suggestions: only the other one is.
    expect(screen.queryByRole('button', { name: /^Share a base computer/ })).toBeNull();
    expect(screen.getByRole('button', { name: /^Start a task from Slack/ })).toBeInTheDocument();
    history.replaceState(null, '', '/workshop#part-d');
    window.dispatchEvent(new HashChangeEvent('hashchange'));
    const row = await screen.findByRole('button', { name: /^Fork a computer/ });
    expect(within(row.closest('.ws-item') as HTMLElement).getByRole('link', { name: /14 votes/ })).toBeInTheDocument();
  });

  it('loads the comments of that issue when opened', async () => {
    const { calls, item } = await open('#part-d', 'Fork a computer', {
      state,
      routes: { 'GET issue': (url) => ok(detail(Number(url.searchParams.get('n')), { comments: [comment(5, 'devin-oss', 'Yes, please.')] })) },
    });
    expect(await within(item).findByText('Yes, please.')).toBeInTheDocument();
    expect(calls.filter((call) => call.path === 'issue').map((call) => call.body)).toEqual([null]);
    expect(calls.some((call) => call.path === 'issue')).toBe(true);
  });
});

describe('a closed thread', () => {
  const closedState = () => ok(liveState({ 12: issue({ entry: 'create', title: 'Create and delete a computer', up: 12, comments: 1, state: 'closed', reason: 'completed', closedAt: '2026-11-16T10:00:00Z' }) }));
  const routes = { 'GET issue': (url: URL) => ok(detail(Number(url.searchParams.get('n')), { comments: [comment(7, 'mara-k', 'Looks right.')] })) };

  it('shows its comments, no box, and says it is closed on GitHub with the link to the issue', async () => {
    const { item } = await open('#part-b', 'Create and delete a computer', { me: owner, state: closedState, routes });
    expect(await within(item).findByText('Looks right.')).toBeInTheDocument();
    expect(within(item).getByText(/This thread is closed on GitHub\./)).toBeInTheDocument();
    expect(within(item).getByRole('link', { name: 'GitHub #12 ↗' })).toHaveAttribute('href', 'https://github.com/amontlabs/silo/issues/12');
    expect(within(item).queryByRole('textbox')).toBeNull();
    expect(within(item).queryByText('No comments yet.')).toBeNull();
  });

  it('shows the same to a visitor who is not signed in', async () => {
    const { item } = await open('#part-b', 'Create and delete a computer', { state: closedState, routes });
    expect(await within(item).findByText('Looks right.')).toBeInTheDocument();
    expect(within(item).getByText(/This thread is closed on GitHub\./)).toBeInTheDocument();
    expect(within(item).queryByRole('link', { name: 'Sign in to comment' })).toBeNull();
  });

  it('is shown when the server refuses a comment as closed, with the comment kept in the box', async () => {
    const { item, calls } = await open('#part-b', 'Create and delete a computer', {
      me: owner,
      routes: { ...routes, 'POST comment': () => ({ status: 403, body: { error: 'closed', message: 'This thread is closed on GitHub.', n: 12 } }) },
    });
    await userEvent.type(await within(item).findByRole('textbox', { name: 'Comment' }), 'Too late?');
    await userEvent.click(within(item).getByRole('button', { name: 'Comment' }));
    expect(await within(item).findByText(/This thread is closed on GitHub\./)).toBeInTheDocument();
    expect(posts(calls, 'comment')).toHaveLength(1);
    expect(within(item).queryByRole('textbox')).toBeNull();
    expect(within(item).getByText('Looks right.')).toBeInTheDocument();
  });
});

describe('votes on comments', () => {
  const routes = {
    'GET issue': (url: URL) => ok(detail(Number(url.searchParams.get('n')), { comments: [comment(7, 'mara-k', 'First.', 2), comment(8, 'devin-oss', 'Second.', 0)] })),
    'GET vote': () => ok({ on: false, comments: [8] }),
  };

  it('shows each count; signed out, the button asks to sign in', async () => {
    const { item } = await open('#part-b', 'Create and delete a computer', { routes });
    await within(item).findByText('First.');
    const first = within(within(item).getByText('First.').closest('.ws-c') as HTMLElement);
    expect(first.getByRole('link', { name: /2 votes on this comment, sign in to vote/ })).toHaveAttribute('href', login('#part-b/create'));
    expect(within(within(item).getByText('Second.').closest('.ws-c') as HTMLElement).getByRole('link', { name: /0 votes on this comment/ })).toBeInTheDocument();
  });

  it('marks the comments the person voted for, from `GET vote`, asked once with the row\'s own vote', async () => {
    const { item, calls } = await open('#part-b', 'Create and delete a computer', { me: owner, routes });
    const second = within(await waitFor(() => within(item).getByText('Second.').closest('.ws-c') as HTMLElement));
    await waitFor(() => expect(second.getByRole('button', { name: /0 votes on this comment/ })).toHaveAttribute('aria-pressed', 'true'));
    const first = within(within(item).getByText('First.').closest('.ws-c') as HTMLElement);
    expect(first.getByRole('button', { name: /2 votes on this comment/ })).toHaveAttribute('aria-pressed', 'false');
    expect(calls.filter((call) => call.path === 'vote' && call.method === 'GET')).toHaveLength(1);
  });

  it('changes the count at once and sends the vote; a second click takes it back', async () => {
    let release: () => void = () => {};
    const { item, calls } = await open('#part-b', 'Create and delete a computer', {
      me: owner,
      routes: { ...routes, 'POST comment-vote': () => new Promise((resolve) => { release = () => resolve(ok({ ok: true, up: 3 })); }) },
    });
    const first = () => within(within(item).getByText('First.').closest('.ws-c') as HTMLElement);
    await waitFor(() => expect(first().getByRole('button', { name: /2 votes on this comment/ })).toBeEnabled());
    await waitFor(() => expect(calls.some((call) => call.path === 'vote')).toBe(true));
    await userEvent.click(first().getByRole('button', { name: /2 votes on this comment/ }));
    // The count is there before the server answers.
    expect(first().getByRole('button', { name: /3 votes on this comment/ })).toHaveAttribute('aria-pressed', 'true');
    expect(posts(calls, 'comment-vote')[0].body).toEqual({ n: 12, comment: 7, on: true });
    await act(async () => release());
    expect(first().getByRole('button', { name: /3 votes on this comment/ })).toHaveAttribute('aria-pressed', 'true');
    await userEvent.click(first().getByRole('button', { name: /3 votes on this comment/ }));
    expect(first().getByRole('button', { name: /2 votes on this comment/ })).toHaveAttribute('aria-pressed', 'false');
    await waitFor(() => expect(posts(calls, 'comment-vote')).toHaveLength(2));
    expect(posts(calls, 'comment-vote')[1].body).toEqual({ n: 12, comment: 7, on: false });
  });

  it('puts the count back and says so when the server refuses', async () => {
    const { item } = await open('#part-b', 'Create and delete a computer', {
      me: owner,
      routes: { ...routes, 'POST comment-vote': () => refused(429, 'GitHub is busy. Try again in a minute.') },
    });
    const first = () => within(within(item).getByText('First.').closest('.ws-c') as HTMLElement);
    await waitFor(() => expect(first().getByRole('button', { name: /2 votes on this comment/ })).toBeEnabled());
    await userEvent.click(first().getByRole('button', { name: /2 votes on this comment/ }));
    expect(await screen.findByText('Vote: not saved. GitHub is busy. Try again in a minute.')).toBeInTheDocument();
    expect(first().getByRole('button', { name: /2 votes on this comment/ })).toHaveAttribute('aria-pressed', 'false');
  });

  it('keeps the count the server answered over an older answer of the thread, for a minute only', async () => {
    const time = clock();
    let served = 2;
    // Each answer of the thread gives the second comment a count of its own, so the test can see that it was applied.
    let answers = 0;
    const { item, calls } = await open('#part-b', 'Create and delete a computer', {
      me: owner,
      routes: {
        ...routes,
        'GET issue': (url) => {
          answers += 1;
          return ok(detail(Number(url.searchParams.get('n')), { comments: [comment(7, 'mara-k', 'First.', served), comment(8, 'devin-oss', 'Second.', 100 + answers)] }));
        },
        'POST comment-vote': () => ok({ ok: true, up: 9 }),
      },
    });
    const first = () => within(within(item).getByText('First.').closest('.ws-c') as HTMLElement);
    const second = () => within(within(item).getByText('Second.').closest('.ws-c') as HTMLElement);
    const reads = () => calls.filter((call) => call.path === 'issue').length;
    const reopen = async (expected: number) => {
      time.advance(5000);
      await userEvent.click(rowButton('Create and delete a computer'));
      await userEvent.click(rowButton('Create and delete a computer'));
      await waitFor(() => expect(reads()).toBe(expected));
      await waitFor(() => expect(second().getByRole('button', { name: new RegExp(`^${100 + answers} votes on this comment`) })).toBeInTheDocument());
    };
    await waitFor(() => expect(first().getByRole('button', { name: /2 votes on this comment/ })).toBeEnabled());
    await userEvent.click(first().getByRole('button', { name: /2 votes on this comment/ }));
    await waitFor(() => expect(first().getByRole('button', { name: /9 votes on this comment/ })).toBeInTheDocument());
    // A second answer of the thread, still saying 2 (a cached one), does not lower it.
    const before = reads();
    await reopen(before + 1);
    expect(first().getByRole('button', { name: /9 votes on this comment/ })).toBeInTheDocument();
    // An answer fetched more than a minute after the write shows its own count.
    time.advance(61_000);
    served = 11;
    await reopen(before + 2);
    expect(first().getByRole('button', { name: /11 votes on this comment/ })).toBeInTheDocument();
  });

  it('takes back only its own change when the server refuses, from the count shown by then', async () => {
    const time = clock();
    let release: () => void = () => {};
    let served = 2;
    const { item, calls } = await open('#part-b', 'Create and delete a computer', {
      me: owner,
      routes: {
        ...routes,
        'GET issue': (url) => ok(detail(Number(url.searchParams.get('n')), { comments: [comment(7, 'mara-k', 'First.', served), comment(8, 'devin-oss', 'Second.', 0)] })),
        'POST comment-vote': () => new Promise((resolve) => { release = () => resolve(refused(429, 'GitHub is busy. Try again in a minute.')); }),
      },
    });
    const first = () => within(within(item).getByText('First.').closest('.ws-c') as HTMLElement);
    await waitFor(() => expect(first().getByRole('button', { name: /2 votes on this comment/ })).toBeEnabled());
    await userEvent.click(first().getByRole('button', { name: /2 votes on this comment/ }));
    expect(first().getByRole('button', { name: /3 votes on this comment/ })).toBeInTheDocument();
    // A newer answer of the thread arrives while the vote is on its way: others voted too.
    served = 5;
    const before = calls.filter((call) => call.path === 'issue').length;
    time.advance(5000);
    await userEvent.click(rowButton('Create and delete a computer'));
    await userEvent.click(rowButton('Create and delete a computer'));
    await waitFor(() => expect(calls.filter((call) => call.path === 'issue').length).toBe(before + 1));
    await wait(30);
    expect(first().getByRole('button', { name: /5 votes on this comment/ })).toBeInTheDocument();
    await act(async () => release());
    expect(await screen.findByText(/Vote: not saved/)).toBeInTheDocument();
    expect(first().getByRole('button', { name: /4 votes on this comment/ })).toHaveAttribute('aria-pressed', 'false');
  });
});

describe('Latest', () => {
  it('is always on the page, in the strip and as a place, whatever the state', async () => {
    history.replaceState(null, '', '/workshop');
    mockApi({ state: () => ok(liveState({}, [], {})) });
    await renderApp();
    const link = screen.getByRole('link', { name: 'Latest' });
    await userEvent.click(link);
    expect(await screen.findByRole('heading', { name: 'Latest' })).toBeInTheDocument();
  });
});

describe('what an older page remembered', () => {
  it('is not shown when it holds no `threads`, and the page still draws from the plan and the server\'s answer', async () => {
    const old = liveState();
    delete old.threads;
    localStorage.setItem('silo-workshop-state', JSON.stringify(old));
    history.replaceState(null, '', '/workshop#part-b');
    mockApi();
    await renderApp();
    const row = itemOf('Create and delete a computer');
    expect(within(row).getByRole('link', { name: /12 votes/ })).toBeInTheDocument();
    expect(JSON.parse(localStorage.getItem('silo-workshop-state') as string)).toHaveProperty('threads');
  });

  it('keeps an older list of its own writes without `opens`', async () => {
    localStorage.setItem('silo-workshop-writes-2', JSON.stringify({ writes: [{ n: 12, at: Date.now(), set: { up: 30 } }], comments: [] }));
    history.replaceState(null, '', '/workshop#part-b');
    mockApi();
    await renderApp();
    expect(within(itemOf('Create and delete a computer')).getByRole('link', { name: /30 votes/ })).toBeInTheDocument();
    void apiConfig;
    void visitor;
  });
});

/** The thread the server holds already: three comments by others, on the issue `n`. */
const held3 = (url: URL) => ok(detail(Number(url.searchParams.get('n')), { comments: [comment(21, 'lin-qi', 'Already one.'), comment(22, 'devin-oss', 'Already two.'), comment(23, 'mara-k', 'Already three.')] }));
const seeAll = async (item: HTMLElement) => {
  for (const text of ['Already one.', 'Already two.', 'Already three.']) expect(await within(item).findByText(text)).toBeInTheDocument();
  expect(item.querySelector('.ws-c-ghost')).toBeNull();
};

describe('a row that takes a thread that already holds comments', () => {
  it('shows them all after its first comment, with the person\'s own', async () => {
    localStorage.setItem('silo-workshop-state', JSON.stringify(liveState()));
    const { item } = await open('#part-b', GUARD, { me: owner, routes: { 'GET issue': held3, 'POST comment': () => ok({ ok: true, id: 901, comments: 4, n: 60 }) } });
    await userEvent.type(await within(item).findByRole('textbox', { name: 'Comment' }), 'Mine.');
    await userEvent.click(within(item).getByRole('button', { name: 'Comment' }));
    await seeAll(item);
    expect(within(item).getAllByText('Mine.')).toHaveLength(1);
  });

  it('shows them all after its first 👍', async () => {
    localStorage.setItem('silo-workshop-state', JSON.stringify(liveState()));
    const { item, calls } = await open('#part-b', GUARD, { me: owner, routes: { 'GET issue': held3, 'POST vote': () => ok({ ok: true, up: 7, n: 60 }) } });
    await userEvent.click(within(item).getByRole('button', { name: /0 votes/ }));
    await seeAll(item);
    expect(calls.filter((call) => call.path === 'vote' && call.method === 'GET')).toHaveLength(1);
  });

  it('loads the thread, and the person\'s votes, when the state gives it to a row already open', async () => {
    let release: (value: ReturnType<typeof ok>) => void = () => {};
    let asked = 0;
    const calls = mockApi({
      me: owner,
      state: () => {
        asked += 1;
        if (asked === 1) return ok({ ...liveState(), old: true });
        return new Promise((resolve) => { release = resolve; });
      },
      routes: { 'GET issue': held3 },
    });
    history.replaceState(null, '', '/workshop#part-b');
    await renderApp(buildData(withLists()));
    await userEvent.click(rowButton(GUARD));
    const item = itemOf(GUARD);
    expect(within(item).getByText('No comments yet.')).toBeInTheDocument();
    await act(async () => release(ok(liveState({ 60: issue({ entry: 'leak-guard', title: GUARD, up: 2, comments: 3 }) }))));
    await seeAll(item);
    await waitFor(() => expect(calls.filter((call) => call.path === 'vote' && call.method === 'GET')).toHaveLength(1));
  });
});

describe('writes to a decision or a feature', () => {
  /** Answers as the server does: a write that names an entry whose thread is closed is refused 403 `closed` with `n`; one that names an issue is refused plainly. */
  const closedFor = (n: number): Route => (_url, body) => (body && 'entry' in body
    ? { status: 403, body: { error: 'closed', message: 'This thread is closed on GitHub.', n } }
    : { status: 403, body: { error: 'forbidden', message: 'Not allowed.' } });

  it('name the entry, whether or not the page knows the thread', async () => {
    const { item, calls } = await open('#part-b', 'Create and delete a computer', { me: owner, routes: { 'POST comment': () => ok({ ok: true, id: 9, comments: 3, n: 12 }) } });
    await userEvent.type(await within(item).findByRole('textbox', { name: 'Comment' }), 'Known thread.');
    await userEvent.click(within(item).getByRole('button', { name: 'Comment' }));
    await waitFor(() => expect(posts(calls, 'comment')).toHaveLength(1));
    expect(posts(calls, 'comment')[0].body).toEqual({ entry: 'create', text: 'Known thread.' });
  });

  it('show a thread the server says is closed as closed, when the page knew it', async () => {
    const { item } = await open('#part-b', 'Create and delete a computer', { me: owner, routes: { 'POST comment': closedFor(12) } });
    await userEvent.type(await within(item).findByRole('textbox', { name: 'Comment' }), 'Too late?');
    await userEvent.click(within(item).getByRole('button', { name: 'Comment' }));
    expect(await within(item).findByText(/This thread is closed on GitHub\./)).toBeInTheDocument();
    expect(within(item).queryByRole('textbox')).toBeNull();
  });

  it('take the thread the server names when it refuses a first comment as closed, and show it closed with the link', async () => {
    const { item } = await open('#part-b', GUARD, { me: owner, routes: { 'POST comment': closedFor(60), 'GET issue': held3 } });
    await userEvent.type(await within(item).findByRole('textbox', { name: 'Comment' }), 'First?');
    await userEvent.click(within(item).getByRole('button', { name: 'Comment' }));
    expect(await within(item).findByText(/This thread is closed on GitHub\./)).toBeInTheDocument();
    expect(within(item).getByRole('link', { name: 'GitHub #60 ↗' })).toHaveAttribute('href', 'https://github.com/amontlabs/silo/issues/60');
    expect(within(item).queryByRole('textbox')).toBeNull();
    await seeAll(item);
  });

  it('keep the count of the comment sent last when the answers come in the other order', async () => {
    const releases: Record<string, () => void> = {};
    const answers: Record<string, number> = { 'Comment A.': 3, 'Comment B.': 4 };
    const { item } = await open('#part-b', 'Create and delete a computer', {
      me: owner,
      routes: { 'POST comment': (_url, body) => new Promise((resolve) => { const text = String(body?.text); releases[text] = () => resolve(ok({ ok: true, id: text === 'Comment A.' ? 31 : 32, comments: answers[text], n: 12 })); }) },
    });
    const field = await within(item).findByRole('textbox', { name: 'Comment' });
    for (const text of ['Comment A.', 'Comment B.']) {
      await userEvent.type(field, text);
      await userEvent.click(within(item).getByRole('button', { name: 'Comment' }));
    }
    await act(async () => releases['Comment B.']());
    await act(async () => releases['Comment A.']());
    await waitFor(() => expect(item.querySelectorAll('.ws-pending')).toHaveLength(0));
    expect(within(item).getByText('💬 4')).toBeInTheDocument();
  });
});

describe('an address that names a suggestion that became a feature', () => {
  it('opens the feature, where it lives, and rewrites the address', async () => {
    mockApi({
      state: () => ok(liveState({ 31: issue({ kind: 'suggestion', title: 'Share a base computer', up: 14, author: 'mara-k', bot: false }) }, [], { fork: 31 })),
    });
    history.replaceState(null, '', '/workshop#suggested/issue-31');
    await renderApp();
    await waitFor(() => expect(rowButton('Fork a computer')).toHaveAttribute('aria-expanded', 'true'));
    expect(location.hash).toBe('#part-d/fork');
  });
});

describe('an open row while the state is not shown yet', () => {
  it('holds the room of "No comments yet." and the box, and keeps that shape when the answer says no thread', async () => {
    let release: (value: ReturnType<typeof ok>) => void = () => {};
    mockApi({ state: () => new Promise((resolve) => { release = resolve; }) });
    history.replaceState(null, '', '/workshop#part-b');
    render(<App data={buildData(withLists())} />);
    await waitFor(() => expect(rowButton(GUARD)).toBeInTheDocument());
    fireEvent.click(rowButton(GUARD));
    const item = itemOf(GUARD);
    const shape = () => [...(item.querySelector('.ws-talk') as HTMLElement).children].map((node) => node.tagName + (node.classList.contains('ws-none') ? '.none' : node.classList.contains('ws-box') ? '.box' : ''));
    const waiting = shape();
    expect(waiting).toEqual(['P.none', 'DIV.box', 'SPAN']);
    expect(item.querySelector('.ws-talk .ws-none')).toHaveAttribute('aria-hidden', 'true');
    await act(async () => release(ok(liveState())));
    await waitFor(() => expect(document.querySelector('.ws')).toHaveAttribute('data-live', 'answered'));
    expect(shape()).toEqual(waiting);
    expect(within(item).getByText('No comments yet.')).not.toHaveAttribute('aria-hidden');
  });
});
