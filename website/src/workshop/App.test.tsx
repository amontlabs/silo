import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { App, OPEN_GUARD_MS } from './App';
import type { IssueDetail, Me } from './api';
import { buildData, issue, liveState } from './fixtures';
import type { LiveState } from './plan';

type Call = { method: string; path: string; body: Record<string, unknown> | null; headers: Record<string, string> };
type Reply = { status: number; body: unknown } | Promise<{ status: number; body: unknown }>;
type Route = (url: URL, body: Record<string, unknown> | null) => Reply;

const ok = (body: unknown = {}) => ({ status: 200, body });
const refused = (status: number, message: string) => ({ status, body: { error: 'refused', message } });

function detail(n: number, fields: Partial<IssueDetail> = {}): IssueDetail {
  return { number: n, title: `Issue ${n}`, body: `Body of ${n}.`, url: `https://github.com/amontlabs/silo/issues/${n}`, up: 0, viewerUp: false, comments: [], ...fields };
}

function mockApi(options: { state?: Route; me?: Me | null; routes?: Record<string, Route> } = {}) {
  const calls: Call[] = [];
  const routes: Record<string, Route> = {
    'GET state': options.state ?? (() => ok(liveState())),
    'GET me': () => (options.me ? ok(options.me) : refused(401, 'Not signed in.')),
    'GET issue': (url) => ok(detail(Number(url.searchParams.get('n')))),
    'POST verdict': () => ok(),
    'POST comment': () => ok(),
    'POST vote': () => ok(),
    'POST decide': () => ok(),
    'POST logout': () => ok(),
    ...options.routes,
  };
  vi.stubGlobal('fetch', vi.fn(async (input: string, init?: RequestInit) => {
    const url = new URL(input, 'http://localhost');
    const method = init?.method ?? 'GET';
    const path = url.pathname.replace('/api/workshop/', '');
    const body = init?.body ? (JSON.parse(String(init.body)) as Record<string, unknown>) : null;
    calls.push({ method, path, body, headers: (init?.headers ?? {}) as Record<string, string> });
    const route = routes[`${method} ${path}`];
    const reply = route ? await route(url, body) : refused(404, 'Not found.');
    return new Response(JSON.stringify(reply.body), { status: reply.status, headers: { 'Content-Type': 'application/json' } });
  }));
  return calls;
}

const posts = (calls: Call[], path: string) => calls.filter((call) => call.method === 'POST' && call.path === path);
const owner: Me = { login: 'polarzero', avatar: '', canDecide: true };
const visitor: Me = { login: 'mara-k', avatar: '', canDecide: false };

function rowButton(name: string) {
  return screen.getByRole('button', { name: new RegExp(`^${name}`) });
}

function itemOf(name: string) {
  return rowButton(name).closest('.ws-item') as HTMLElement;
}

async function renderApp() {
  const view = render(<App data={buildData()} />);
  await waitFor(() => expect(screen.queryAllByText(/^👍/).length + screen.queryAllByText('Live comments are unavailable.').length).toBeGreaterThan(0));
  return view;
}

function clock() {
  let now = 1_000_000;
  vi.spyOn(Date, 'now').mockImplementation(() => now);
  return { advance(ms: number) { now += ms; } };
}

describe('a visitor', () => {
  it('sees the title, the counts line, open stages and closed later stages', async () => {
    mockApi();
    await renderApp();
    expect(screen.getByRole('heading', { level: 1, name: 'Workshop' })).toBeInTheDocument();
    expect(screen.getByText('Silo is being rebuilt from scratch. This is the plan, in build order.')).toBeInTheDocument();
    expect(screen.getByText('3 planned · 4 undecided')).toBeInTheDocument();
    expect(screen.getByText('2 of 4')).toBeInTheDocument();
    expect(rowButton('Run a command')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /^Fork a computer/ })).not.toBeInTheDocument();
    const loop = screen.getByRole('button', { name: /The loop/ });
    expect(loop).toHaveAttribute('aria-expanded', 'false');
    await userEvent.click(loop);
    expect(rowButton('Fork a computer')).toBeInTheDocument();
    expect(screen.getByText('USABLE EVERY DAY FROM HERE')).toBeInTheDocument();
  });

  it('sees status words, votes and comment counts on rows', async () => {
    mockApi();
    await renderApp();
    expect(within(itemOf('The engine runs on its own')).getByText('planned')).toBeInTheDocument();
    expect(within(itemOf('Create and delete a computer')).queryByText('planned')).not.toBeInTheDocument();
    expect(within(itemOf('Create and delete a computer')).getByText('👍 12')).toBeInTheDocument();
    expect(within(itemOf('Create and delete a computer')).getByText('💬 2')).toBeInTheDocument();
    expect(within(itemOf('Run a command')).queryByText(/💬/)).not.toBeInTheDocument();
  });

  it('is sent to sign in for votes, comments, suggestions and the header', async () => {
    history.replaceState(null, '', '/workshop#entry-computers');
    mockApi();
    await renderApp();
    const login = '/api/workshop/login?return=%2Fworkshop%23entry-computers';
    expect(screen.getByRole('link', { name: 'Sign in with GitHub' })).toHaveAttribute('href', login);
    expect(screen.getByRole('link', { name: 'Suggest a feature' })).toHaveAttribute('href', login);
    expect(within(itemOf('Create and delete a computer')).getByRole('link', { name: /12 votes/ })).toHaveAttribute('href', login);
    expect(await screen.findByRole('link', { name: 'Sign in to comment' })).toHaveAttribute('href', login);
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /^Take/ })).not.toBeInTheDocument();
  });

  it('opens one row at a time in place, loads its comments and links it', async () => {
    const calls = mockApi({ routes: { 'GET issue': (url) => ok(detail(Number(url.searchParams.get('n')), { comments: [{ id: 1, author: 'mara-k', avatar: '', bot: false, createdAt: '2026-11-14T10:00:00Z', text: 'Keep stop separate from pause.' }] })) } });
    await renderApp();
    await userEvent.click(rowButton('Create and delete a computer'));
    expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByText('Make a Linux computer.')).toBeInTheDocument();
    expect(screen.getByText('Not decided yet.')).toBeInTheDocument();
    expect(await screen.findByText('Keep stop separate from pause.')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'GitHub #12 ↗' })).toHaveAttribute('href', 'https://github.com/amontlabs/silo/issues/12');
    expect(calls.some((call) => call.path === 'issue' && call.method === 'GET')).toBe(true);
    expect(location.hash).toBe('#entry-computers');
    expect(document.querySelector('video')).toHaveAttribute('src', '/workshop/recordings/create.mp4');
    expect(screen.getByText('Create and delete a computer, open')).toBeInTheDocument();

    await userEvent.click(rowButton('Run a command'));
    expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'false');
    expect(screen.queryByText('Make a Linux computer.')).not.toBeInTheDocument();
    expect(location.hash).toBe('#entry-exec');
    await userEvent.click(rowButton('Run a command'));
    expect(location.hash).toBe('');
  });

  it('shows a carried-over verdict as planned, never as a click', async () => {
    mockApi();
    await renderApp();
    await userEvent.click(rowButton('Every call defined once'));
    expect(screen.getByText('Planned · carried over')).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /^Confirm/ })).not.toBeInTheDocument();
  });

  it('lists dropped entries in one closed fold, struck through', async () => {
    mockApi();
    await renderApp();
    const fold = screen.getByText('1 dropped').closest('details') as HTMLDetailsElement;
    expect(fold.open).toBe(false);
    expect(within(fold).getByText('A hosted reviewer')).toHaveClass('ws-name');
  });
});

describe('text written on GitHub', () => {
  it('never becomes HTML', async () => {
    const hostile = 'Look <img src=x onerror="window.hacked=1"> <script>window.hacked=1</script>\n\n[click](javascript:alert(1)) javascript:alert(1) and `<b>code</b>` and https://example.com/ok.';
    mockApi({ routes: { 'GET issue': (url) => ok(detail(Number(url.searchParams.get('n')), { body: hostile, comments: [{ id: 1, author: 'eve', avatar: 'javascript:alert(1)', bot: false, createdAt: '2026-11-14T10:00:00Z', text: hostile }] })) } });
    history.replaceState(null, '', '/workshop#issue-30');
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
      expect(screen.getByText('Live comments are unavailable.')).toBeInTheDocument();
      expect(screen.getByText('3 planned · 4 undecided')).toBeInTheDocument();
      expect(rowButton('Create and delete a computer')).toBeInTheDocument();
      expect(screen.queryByText(/^👍/)).not.toBeInTheDocument();
      expect(screen.queryByText(/Sign in/)).not.toBeInTheDocument();
      expect(screen.queryByText('Suggest a feature')).not.toBeInTheDocument();
      expect(screen.queryByText('polarzero')).not.toBeInTheDocument();
      await userEvent.click(rowButton('Create and delete a computer'));
      expect(screen.getByText('Not decided yet.')).toBeInTheDocument();
      expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
      expect(screen.queryByRole('button', { name: /^Take/ })).not.toBeInTheDocument();
      await userEvent.click(screen.getByRole('tab', { name: 'Decisions' }));
      expect(rowButton('The engine is written in Zig')).toBeInTheDocument();
    });
  }

  it('shows the plan while the state is still loading', async () => {
    let release: (value: { status: number; body: unknown }) => void = () => {};
    mockApi({ state: () => new Promise((resolve) => { release = resolve; }) });
    render(<App data={buildData()} />);
    expect(rowButton('Create and delete a computer')).toBeInTheDocument();
    expect(screen.queryByText('Live comments are unavailable.')).not.toBeInTheDocument();
    expect(screen.queryByText(/^👍/)).not.toBeInTheDocument();
    await act(async () => release(ok(liveState())));
    expect(await screen.findByText('👍 12')).toBeInTheDocument();
  });
});

describe('a signed-in person', () => {
  it('sees their login, signs out, comments and votes', async () => {
    const calls = mockApi({ me: visitor });
    await renderApp();
    expect(screen.getByText('mara-k', { selector: '.ws-login' })).toBeInTheDocument();
    await userEvent.click(rowButton('Create and delete a computer'));
    const box = await screen.findByRole('textbox', { name: 'Comment' });
    await userEvent.type(box, 'Please keep the defaults small.');
    await userEvent.click(screen.getByRole('button', { name: 'Comment' }));
    expect(screen.getByText('Please keep the defaults small.')).toBeInTheDocument();
    expect(within(itemOf('Create and delete a computer')).getByText('💬 3')).toBeInTheDocument();
    expect(posts(calls, 'comment')[0]).toMatchObject({ body: { n: 12, text: 'Please keep the defaults small.' }, headers: { 'X-Workshop': '1' } });

    const vote = within(itemOf('Create and delete a computer')).getByRole('button', { name: /12 votes/ });
    await userEvent.click(vote);
    await waitFor(() => expect(posts(calls, 'vote')).toHaveLength(1));
    expect(posts(calls, 'vote')[0].body).toEqual({ n: 12, on: true });
    expect(within(itemOf('Create and delete a computer')).getByRole('button', { name: /13 votes/ })).toHaveAttribute('aria-pressed', 'true');
    await userEvent.click(within(itemOf('Create and delete a computer')).getByRole('button', { name: /13 votes/ }));
    await waitFor(() => expect(posts(calls, 'vote')).toHaveLength(2));
    expect(posts(calls, 'vote')[1].body).toEqual({ n: 12, on: false });

    await userEvent.click(screen.getByRole('button', { name: 'Sign out' }));
    expect(await screen.findByRole('link', { name: 'Sign in with GitHub' })).toBeInTheDocument();
  });

  it('gets a refused comment back in the box with a plain message', async () => {
    mockApi({ me: visitor, routes: { 'POST comment': () => refused(429, 'GitHub is busy.') } });
    await renderApp();
    await userEvent.click(rowButton('Run a command'));
    await userEvent.type(await screen.findByRole('textbox', { name: 'Comment' }), 'Hello');
    await userEvent.click(screen.getByRole('button', { name: 'Comment' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Comment: not saved. GitHub is busy.');
    expect(screen.getByRole('textbox', { name: 'Comment' })).toHaveValue('Hello');
    expect(within(itemOf('Run a command')).queryByText(/💬/)).not.toBeInTheDocument();
  });

  it('rolls back a refused vote', async () => {
    mockApi({ me: visitor, routes: { 'POST vote': () => refused(502, 'GitHub failed.') } });
    await renderApp();
    await userEvent.click(within(itemOf('Run a command')).getByRole('button', { name: /4 votes/ }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Vote: not saved. GitHub failed.');
    expect(within(itemOf('Run a command')).getByRole('button', { name: /4 votes/ })).toHaveAttribute('aria-pressed', 'false');
  });

  it('suggests a feature that appears under Suggested by people', async () => {
    const calls = mockApi({ me: visitor, routes: { 'POST suggest': () => ok({ number: 77 }) } });
    await renderApp();
    await userEvent.click(screen.getByRole('button', { name: 'Suggest a feature' }));
    await userEvent.type(screen.getByRole('textbox', { name: 'Title' }), 'Pause a computer');
    await userEvent.type(screen.getByRole('textbox', { name: 'Why' }), 'Stop is not pause.');
    await userEvent.click(screen.getByRole('button', { name: 'Suggest' }));
    expect(posts(calls, 'suggest')[0].body).toEqual({ title: 'Pause a computer', text: 'Stop is not pause.' });
    const group = screen.getByRole('region', { name: 'Suggested by people' });
    await waitFor(() => expect(within(group).getByRole('button', { name: /^Pause a computer/ }).closest('.ws-item')).toHaveAttribute('id', 'issue-77'));
    expect(within(group).getAllByRole('button', { name: /votes/ }).map((b) => b.textContent)).toEqual(['👍 14', '👍 9', '👍 0']);
  });

  it('gets a refused suggestion back in the form', async () => {
    mockApi({ me: visitor, routes: { 'POST suggest': () => refused(400, 'The title is too long.') } });
    await renderApp();
    await userEvent.click(screen.getByRole('button', { name: 'Suggest a feature' }));
    await userEvent.type(screen.getByRole('textbox', { name: 'Title' }), 'Pause');
    await userEvent.click(screen.getByRole('button', { name: 'Suggest' }));
    expect(await screen.findByRole('alert')).toHaveTextContent('Suggestion: not saved. The title is too long.');
    expect(screen.getByRole('textbox', { name: 'Title' })).toHaveValue('Pause');
    expect(screen.queryByRole('button', { name: /^Pause/ })).not.toBeInTheDocument();
  });

  it('cannot decide', async () => {
    mockApi({ me: visitor });
    await renderApp();
    expect(document.querySelector('.ws-item.open')).toBeNull();
    await userEvent.click(rowButton('Create and delete a computer'));
    expect(screen.queryByRole('button', { name: /^Take/ })).not.toBeInTheDocument();
    expect(screen.getByText('Not decided yet.')).toBeInTheDocument();
  });
});

describe('a person who can decide', () => {
  it('starts on the first undecided entry with its actions', async () => {
    mockApi({ me: owner });
    await renderApp();
    await waitFor(() => expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'true'));
    for (const name of [/^Take/, /^Later$/, /^Drop$/, /^Refine…$/]) expect(screen.getByRole('button', { name })).toBeInTheDocument();
    expect(screen.getByText('Suggested: take. Key records by id.')).toBeInTheDocument();
    expect(document.activeElement).toBe(rowButton('Create and delete a computer'));
  });

  it('takes with Enter after the guard, then moves to the next undecided entry', async () => {
    const time = clock();
    const calls = mockApi({ me: owner });
    await renderApp();
    await waitFor(() => expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'true'));
    fireEvent.keyDown(document.activeElement as Element, { key: 'Enter' });
    expect(posts(calls, 'verdict')).toHaveLength(0);
    time.advance(OPEN_GUARD_MS);
    fireEvent.keyDown(document.activeElement as Element, { key: 'Enter', repeat: true });
    expect(posts(calls, 'verdict')).toHaveLength(0);
    fireEvent.keyDown(document.activeElement as Element, { key: 'Enter' });
    expect(posts(calls, 'verdict').map((call) => call.body)).toEqual([{ n: 12, verdict: 'take' }]);
    expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'false');
    expect(within(itemOf('Create and delete a computer')).getByText('planned')).toBeInTheDocument();
    expect(rowButton('Run a command')).toHaveAttribute('aria-expanded', 'true');
    expect(location.hash).toBe('#entry-exec');
    // A second fast Enter lands on the row that just opened and is refused.
    fireEvent.keyDown(document.activeElement as Element, { key: 'Enter' });
    expect(posts(calls, 'verdict')).toHaveLength(1);
    time.advance(OPEN_GUARD_MS);
    await userEvent.click(screen.getByRole('button', { name: 'Later' }));
    expect(posts(calls, 'verdict').map((call) => call.body)).toEqual([{ n: 12, verdict: 'take' }, { n: 13, verdict: 'later' }]);
    // The next undecided entry sits in a closed stage, which opens.
    expect(rowButton('Fork a computer')).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByText('4 planned · 2 undecided')).toBeInTheDocument();
  });

  it('ignores Enter in a field and on other buttons', async () => {
    const time = clock();
    const calls = mockApi({ me: owner });
    await renderApp();
    await waitFor(() => expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'true'));
    time.advance(OPEN_GUARD_MS);
    fireEvent.keyDown(await screen.findByRole('textbox', { name: 'Comment' }), { key: 'Enter' });
    fireEvent.keyDown(screen.getByRole('button', { name: 'Drop' }), { key: 'Enter' });
    expect(posts(calls, 'verdict')).toHaveLength(0);
  });

  it('records the verdict for the row it was given, and rolls back a refused one', async () => {
    const time = clock();
    let release: (value: { status: number; body: unknown }) => void = () => {};
    const calls = mockApi({ me: owner, routes: { 'POST verdict': () => new Promise((resolve) => { release = resolve; }) } });
    await renderApp();
    await waitFor(() => expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'true'));
    time.advance(OPEN_GUARD_MS);
    await userEvent.click(screen.getByRole('button', { name: 'Drop' }));
    // The row closed and moved to the fold while the request is in flight.
    expect(screen.getByText('2 dropped')).toBeInTheDocument();
    expect(rowButton('Run a command')).toHaveAttribute('aria-expanded', 'true');
    await act(async () => release(refused(403, 'Not allowed.')));
    expect(await screen.findByRole('alert')).toHaveTextContent('Create and delete a computer: not saved. Not allowed.');
    expect(screen.getByText('1 dropped')).toBeInTheDocument();
    expect(within(itemOf('Create and delete a computer')).queryByText('planned')).not.toBeInTheDocument();
    expect(posts(calls, 'verdict').map((call) => call.body)).toEqual([{ n: 12, verdict: 'drop' }]);
    expect(rowButton('Run a command')).toHaveAttribute('aria-expanded', 'true');
  });

  it('needs a note to refine', async () => {
    const time = clock();
    const calls = mockApi({ me: owner });
    await renderApp();
    await waitFor(() => expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'true'));
    time.advance(OPEN_GUARD_MS);
    await userEvent.click(screen.getByRole('button', { name: 'Refine…' }));
    const submit = screen.getByRole('button', { name: 'Refine' });
    expect(submit).toBeDisabled();
    await userEvent.type(screen.getByRole('textbox', { name: 'Note' }), 'Split create from delete.');
    await userEvent.click(submit);
    expect(posts(calls, 'verdict')[0].body).toEqual({ n: 12, verdict: 'refine', note: 'Split create from delete.' });
  });

  it('confirms a carried-over verdict', async () => {
    const time = clock();
    const calls = mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#entry-calls');
    await renderApp();
    expect(rowButton('Every call defined once')).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByText('Planned · carried over')).toBeInTheDocument();
    time.advance(OPEN_GUARD_MS);
    await userEvent.click(screen.getByRole('button', { name: /^Confirm/ }));
    expect(posts(calls, 'verdict')[0].body).toEqual({ n: 11, verdict: 'take' });
  });

  it('decides an open decision', async () => {
    const calls = mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#issue-42');
    await renderApp();
    await waitFor(() => expect(rowButton('After a task ends')).toHaveAttribute('aria-expanded', 'true'));
    expect(screen.getByRole('tab', { name: 'Decisions' })).toHaveAttribute('aria-selected', 'true');
    const decide = screen.getByRole('button', { name: 'Decide' });
    expect(decide).toBeDisabled();
    await userEvent.type(screen.getByRole('textbox', { name: 'Decision' }), 'It is deleted.');
    await userEvent.click(decide);
    expect(posts(calls, 'decide')[0].body).toEqual({ n: 42, text: 'It is deleted.' });
    const decided = screen.getByRole('region', { name: 'Decided' });
    expect(within(decided).getByRole('button', { name: /^After a task ends/ })).toBeInTheDocument();
  });
});

describe('the decisions tab', () => {
  it('lists open decisions then decided lines, newest first, with comment counts', async () => {
    mockApi();
    await renderApp();
    await userEvent.click(screen.getByRole('tab', { name: 'Decisions' }));
    const open = screen.getByRole('region', { name: 'Open' });
    expect(within(open).getAllByRole('button').map((b) => b.textContent)).toEqual([
      'After a task ends, what happens to its computer?',
      'Which Zig version to pin',
    ]);
    expect(within(open).getByText('💬 5')).toBeInTheDocument();
    expect(within(open).getByText('💬 0')).toBeInTheDocument();
    const decided = screen.getByRole('region', { name: 'Decided' });
    expect(within(decided).getAllByRole('button').map((b) => b.textContent)).toEqual([
      'The engine is written in Zig',
      'Merging changes main only',
      'Security is built properly',
    ]);
    expect(within(decided).getAllByText('16 Nov')).toHaveLength(2);
    expect(within(itemOf('The engine is written in Zig')).getByText('💬 2')).toBeInTheDocument();
    expect(within(itemOf('Merging changes main only')).queryByText(/💬/)).not.toBeInTheDocument();
    await userEvent.click(rowButton('Merging changes main only'));
    expect(screen.getByText('Merging changes main only. See the list.')).toBeInTheDocument();
    expect(screen.getByText("16 Nov · orchestrator, at the owner's word")).toBeInTheDocument();
    expect(location.hash).toBe('#decided-2');
  });
});

describe('deep links', () => {
  it('open a row inside a closed stage', async () => {
    history.replaceState(null, '', '/workshop#entry-devices');
    mockApi();
    await renderApp();
    expect(rowButton('A Linux server')).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByRole('button', { name: /Other devices/ })).toHaveAttribute('aria-expanded', 'true');
  });

  it('open a suggestion once the live state arrives, and a dropped entry in its fold', async () => {
    history.replaceState(null, '', '/workshop#issue-31');
    mockApi();
    await renderApp();
    await waitFor(() => expect(rowButton('Share a base computer')).toHaveAttribute('aria-expanded', 'true'));
    expect(await screen.findByText('Body of 31.')).toBeInTheDocument();
    act(() => {
      location.hash = '#entry-reviewer';
    });
    await waitFor(() => expect(rowButton('A hosted reviewer')).toHaveAttribute('aria-expanded', 'true'));
    expect((screen.getByText('1 dropped').closest('details') as HTMLDetailsElement).open).toBe(true);
  });

  it('take precedence over the first undecided entry', async () => {
    history.replaceState(null, '', '/workshop#entry-fork');
    mockApi({ me: owner });
    await renderApp();
    await waitFor(() => expect(rowButton('Fork a computer')).toHaveAttribute('aria-expanded', 'true'));
    await waitFor(() => expect(screen.getByRole('button', { name: /^Take/ })).toBeInTheDocument());
    expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'false');
  });
});

describe('live verdicts', () => {
  it('replace file verdicts on the page', async () => {
    const state: LiveState = liveState({ 11: issue({ entry: 'calls', verdict: 'later' }) });
    mockApi({ state: () => ok(state) });
    await renderApp();
    expect(within(itemOf('Every call defined once')).getByText('later')).toBeInTheDocument();
    expect(screen.getByText('2 planned · 4 undecided')).toBeInTheDocument();
  });
});
