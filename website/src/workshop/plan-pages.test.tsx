// What the plan in plan/ adds to the page: notes, history lines, Latest, decisions and the status rule.
import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { App } from './App';
import type { Comment } from './api';
import { buildData, FEATURES, historyComment, issue, liveState, SHA } from './fixtures';
import type { Change, LiveLatest } from './plan';
import { detail, itemOf, mockApi, notesButton, ok, openCore, owner, refused, renderApp, rowButton, shownNotes, visitor } from './test-support';
import type { Route } from './test-support';

const NOW = Date.parse('2026-11-17T12:00:00Z');
const fixedNow = () => vi.spyOn(Date, 'now').mockReturnValue(NOW);
const change = (fields: string[], extra: Partial<Change> = {}): Change => ({ fields, before: SHA.a, commit: SHA.b, ...extra }) as Change;
const person = (id: number, text: string, createdAt: string): Comment => ({ id, author: 'mara-k', avatar: '', bot: false, createdAt, text });
describe('notes', () => {
  it('are closed by default, and an entry without notes has no toggle', async () => {
    await openCore();
    expect(notesButton()).toHaveAttribute('aria-expanded', 'false');
    expect(document.querySelector('.ws-md')).toBeNull();
    await userEvent.click(rowButton('Every call defined once'));
    expect(within(itemOf('Every call defined once')).queryByRole('button', { name: 'Notes' })).not.toBeInTheDocument();
  });

  it('never turn markup or a script link into elements', async () => {
    const notes = '## Notes\n\nSee <img src=x onerror=alert(1)> and <script>window.hacked=1</script>\n\n[x](javascript:alert(1)) and [y](data:text/html,hi)\n\n- <b>bold</b>\n\n## <i>head</i>\n';
    await openCore(buildData({ notes: { core: notes } }));
    await userEvent.click(notesButton());
    const md = await shownNotes();
    expect(md.querySelector('img, script, b, i')).toBeNull();
    expect(md.textContent).toContain('<img src=x onerror=alert(1)>');
    expect(md.textContent).toContain('<script>window.hacked=1</script>');
    expect(md.querySelector('a[href^="javascript"], a[href^="data"]')).toBeNull();
    expect(md.querySelectorAll('a')).toHaveLength(0);
    expect(md.textContent).toContain('x and y');
    expect((window as unknown as { hacked?: number }).hacked).toBeUndefined();
  });
});

describe('the notes chunk', () => {
  it('says so when it cannot load, and is tried again at the next reach', async () => {
    let fail = true;
    const loadNotes = vi.fn(() => (fail ? Promise.reject(new Error('offline')) : Promise.resolve({ core: 'Back.' })));
    mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#part-a/core');
    await renderApp(buildData(), loadNotes);
    await waitFor(() => expect(rowButton('The engine runs on its own')).toHaveAttribute('aria-expanded', 'true'));
    await userEvent.click(notesButton());
    expect(await screen.findByText('Notes could not load.')).toBeInTheDocument();
    fail = false;
    await userEvent.click(notesButton());
    await userEvent.click(notesButton());
    expect((await shownNotes()).textContent).toBe('Back.');
    const calls = loadNotes.mock.calls.length;
    await userEvent.click(notesButton());
    await userEvent.click(notesButton());
    expect(loadNotes).toHaveBeenCalledTimes(calls);
  });
});

describe('the line of Notes', () => {
  it('is there only on an entry with notes, for the owner as for anyone', async () => {
    mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#part-a/calls');
    await renderApp();
    await waitFor(() => expect(rowButton('Every call defined once')).toHaveAttribute('aria-expanded', 'true'));
    expect(itemOf('Every call defined once').querySelector('.ws-extras')).toBeNull();
  });

  it('is not there for a signed-out visitor on an entry without notes', async () => {
    mockApi();
    document.cookie = 'workshop-hint=; path=/; max-age=0';
    history.replaceState(null, '', '/workshop#part-a/calls');
    await renderApp();
    await waitFor(() => expect(rowButton('Every call defined once')).toHaveAttribute('aria-expanded', 'true'));
    expect(itemOf('Every call defined once').querySelector('.ws-extras')).toBeNull();
  });
});

describe('history lines', () => {
  const thread = () => [
    person(1, 'Keep stop separate from pause.', '2026-11-10T10:00:00Z'),
    historyComment(2, change(['verdict'], { verdict: 'take' }), '2026-11-11T10:00:00Z'),
    historyComment(3, change(['verdict'], { verdict: 'drop' }), '2026-11-12T10:00:00Z'),
    historyComment(4, change(['verdict'], { verdict: 'later' }), '2026-11-13T10:00:00Z'),
    historyComment(5, change(['verdict'], { verdict: null }), '2026-11-14T10:00:00Z'),
    historyComment(6, change(['built'], { built: '2026-11-15' }), '2026-11-15T10:00:00Z'),
    historyComment(7, change(['name', 'what']), '2026-11-16T10:00:00Z'),
    historyComment(8, change(['notes']), '2026-11-17T08:00:00Z'),
    historyComment(9, change(['needs']), '2026-11-17T10:00:00Z'),
    person(10, 'Thanks, noted.', '2026-11-17T11:00:00Z'),
  ];
  const withThread = (comments = thread()) => ({ 'GET issue': (url: URL) => ok(detail(Number(url.searchParams.get('n')), { comments })) });

  it('show each kind of change in its words, in time order with the comments, and no avatar or reply box of their own', async () => {
    fixedNow();
    mockApi({ me: visitor, routes: withThread() });
    history.replaceState(null, '', '/workshop#part-b/create');
    await renderApp();
    await screen.findByText('Thanks, noted.');
    const talk = itemOf('Create and delete a computer').querySelector('.ws-talk') as HTMLElement;
    const entries = [...talk.querySelectorAll(':scope > .ws-h, :scope > .ws-c')];
    expect(entries.map((node) => (node.classList.contains('ws-h') ? (node.querySelector('p') as HTMLElement).firstChild?.textContent ?? '' : 'comment'))).toEqual([
      'comment', 'Taken', 'Dropped', 'Verdict cleared', 'Built', 'Text changed', 'Notes edited', 'Needs changed', 'comment',
    ]);
    // The old "later" says nothing, so it draws no line.
    const lines = [...talk.querySelectorAll('.ws-h')];
    expect(lines).toHaveLength(7);
    // The line shows when it happened, and the change on GitHub.
    expect(within(lines[0] as HTMLElement).getByText('6 days ago')).toBeInTheDocument();
    expect(within(lines[6] as HTMLElement).getByText('2 hours ago')).toBeInTheDocument();
    const compare = within(lines[0] as HTMLElement).getByRole('link', { name: /^bbbbbbb/ });
    expect(compare).toHaveAttribute('href', `https://github.com/amontlabs/silo/compare/${SHA.a}...${SHA.b}`);
    expect(compare.getAttribute('rel')).toContain('noopener');
    // No avatar initials, no reply box, no text from a person's hand.
    for (const line of lines) {
      expect(line.querySelector('i')?.textContent).toBe('');
      expect(line.querySelector('textarea, input, form')).toBeNull();
    }
    expect(talk.querySelectorAll('.ws-c')).toHaveLength(2);
    expect(within(talk.querySelector('.ws-c') as HTMLElement).getByText('mk')).toBeInTheDocument();
    // The thread keeps one reply box.
    expect(talk.querySelectorAll('textarea')).toHaveLength(1);
    // Only "Text changed" can be opened.
    expect(within(talk).getAllByRole('button').map((button) => button.textContent)).toEqual(['Text changed', 'Comment']);
  });

  it('compare the text before and after a change, fetched at the two commits', async () => {
    fixedNow();
    const comments = [historyComment(1, change(['name', 'what']), '2026-11-16T10:00:00Z'), person(2, 'After it.', '2026-11-16T11:00:00Z')];
    mockApi({ routes: withThread(comments) });
    const files: Record<string, { name: string; what: string }> = {
      [SHA.a]: { name: 'Create a computer', what: 'Make one.' },
      [SHA.b]: { name: 'Create and delete a computer', what: 'Make a Linux computer.' },
    };
    const urls: string[] = [];
    const api = fetch;
    vi.stubGlobal('fetch', vi.fn(async (input: string, init?: RequestInit) => {
      if (!String(input).startsWith('https://raw.githubusercontent.com/')) return api(input, init);
      urls.push(String(input));
      const file = files[String(input).split('/')[5]];
      return file ? new Response(JSON.stringify({ ...file, part: 'part-b', needs: [] })) : new Response('', { status: 404 });
    }));
    history.replaceState(null, '', '/workshop#part-b/create');
    await renderApp();
    const button = await screen.findByRole('button', { name: 'Text changed' });
    expect(urls).toEqual([]);
    await userEvent.click(button);
    expect(await screen.findByText('Before')).toBeInTheDocument();
    expect(urls.sort()).toEqual([
      `https://raw.githubusercontent.com/amontlabs/silo/${SHA.a}/plan/features/create.json`,
      `https://raw.githubusercontent.com/amontlabs/silo/${SHA.b}/plan/features/create.json`,
    ].sort());
    const diff = document.querySelector('.ws-h-diff') as HTMLElement;
    const [before, after] = [...diff.children] as HTMLElement[];
    expect(before).toHaveTextContent('BeforeCreate a computerMake one.');
    expect(after).toHaveTextContent('AfterCreate and delete a computerMake a Linux computer.');
    expect(button).toHaveAttribute('aria-expanded', 'true');
    await userEvent.click(button);
    expect(document.querySelector('.ws-h-diff')).toBeNull();
  });

  it('say "not available" when a file cannot be fetched, and change nothing else', async () => {
    fixedNow();
    const comments = [person(1, 'Before it.', '2026-11-15T10:00:00Z'), historyComment(2, change(['name']), '2026-11-16T10:00:00Z'), historyComment(3, change(['verdict'], { verdict: 'take' }), '2026-11-16T11:00:00Z')];
    mockApi({ routes: withThread(comments) });
    const api = fetch;
    vi.stubGlobal('fetch', vi.fn(async (input: string, init?: RequestInit) => (String(input).startsWith('https://raw.githubusercontent.com/') ? new Response('gone', { status: 404 }) : api(input, init))));
    history.replaceState(null, '', '/workshop#part-b/create');
    await renderApp();
    // The row the address names opens only once the live state is in, after renderApp returns: the thread is
    // taken from the line found in it, not from the row at once.
    const button = await screen.findByRole('button', { name: 'Text changed' });
    const talk = button.closest('.ws-talk') as HTMLElement;
    await userEvent.click(button);
    expect(await screen.findByText('not available')).toBeInTheDocument();
    expect(document.querySelectorAll('.ws-h-diff')).toHaveLength(1);
    expect(within(talk).queryByText('Before')).not.toBeInTheDocument();
    expect(within(talk).getByText('Before it.')).toBeInTheDocument();
    expect(within(talk).getByText('Taken')).toBeInTheDocument();
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'true');
  });

  it('read the files from the host the page names, when it names one', async () => {
    fixedNow();
    const meta = document.createElement('meta');
    meta.name = 'workshop-raw';
    meta.content = 'https://raw.example.org/';
    document.head.append(meta);
    try {
      mockApi({ routes: withThread([historyComment(1, change(['what']), '2026-11-16T10:00:00Z')]) });
      const urls: string[] = [];
      const api = fetch;
      vi.stubGlobal('fetch', vi.fn(async (input: string, init?: RequestInit) => {
        if (String(input).startsWith('https://raw.example.org/')) {
          urls.push(String(input));
          return new Response(JSON.stringify({ name: 'N', what: String(input).includes(SHA.a) ? 'Old.' : 'New.' }));
        }
        return api(input, init);
      }));
      history.replaceState(null, '', '/workshop#part-b/create');
      await renderApp();
      await userEvent.click(await screen.findByRole('button', { name: 'Text changed' }));
      expect(await screen.findByText('Old.')).toBeInTheDocument();
      expect(screen.getByText('New.')).toBeInTheDocument();
      expect(urls).toContain(`https://raw.example.org/amontlabs/silo/${SHA.b}/plan/features/create.json`);
    } finally {
      meta.remove();
    }
  });

});

describe('Latest', () => {
  const at = (hours: number) => new Date(NOW - hours * 3_600_000).toISOString();
  const latest: LiveLatest[] = [
    { n: 12, entry: 'create', author: 'mara-k', bot: false, createdAt: at(1), text: 'Keep stop separate.' },
    { n: 10, entry: 'core', author: 'silo-workshop[bot]', bot: true, createdAt: at(5), change: change(['verdict'], { verdict: 'take' }) },
    { n: 14, entry: 'fork', author: 'devin-oss', bot: false, createdAt: at(30), text: 'Fork what, exactly?' },
  ];

  it('is reachable from the strip under the map, and lists the newest first, each linking to its entry', async () => {
    fixedNow();
    mockApi({ state: () => ok(liveState({}, latest)) });
    await renderApp();
    const strip = document.querySelector('.ws-fold-link[href="#latest"]') as HTMLAnchorElement;
    expect(strip).toHaveTextContent('Latest');
    await userEvent.click(strip);
    expect(location.hash).toBe('#latest');
    expect(screen.getByRole('heading', { level: 2, name: 'Latest' })).toHaveFocus();
    const rows = [...document.querySelectorAll<HTMLAnchorElement>('.ws-latest a.ws-row')];
    expect(rows.map((row) => row.getAttribute('href'))).toEqual(['#part-b/create', '#part-a/core', '#part-d/fork']);
    expect(rows.map((row) => row.querySelector('.ws-name')?.textContent)).toEqual([
      'Create and delete a computermara-k: Keep stop separate.',
      'The engine runs on its ownTaken',
      'Fork a computerdevin-oss: Fork what, exactly?',
    ]);
    expect(rows.map((row) => row.querySelector('.ws-word')?.textContent)).toEqual(['1 hour ago', '5 hours ago', 'yesterday']);
  });

  it('leaves a click with a modifier key to the browser, so an item opens in a new tab', async () => {
    fixedNow();
    mockApi({ state: () => ok(liveState({}, latest)) });
    history.replaceState(null, '', '/workshop#latest');
    await renderApp();
    const link = await screen.findByRole('link', { name: /^The engine runs on its own/ });
    for (const init of [{ metaKey: true }, { ctrlKey: true }, { shiftKey: true }, { altKey: true }, { button: 1 }]) {
      const event = new MouseEvent('click', { bubbles: true, cancelable: true, ...init });
      link.dispatchEvent(event);
      expect(event.defaultPrevented).toBe(false);
    }
    expect(location.hash).toBe('#latest');
    const plain = new MouseEvent('click', { bubbles: true, cancelable: true, button: 0 });
    await act(async () => { link.dispatchEvent(plain); });
    expect(plain.defaultPrevented).toBe(true);
    await waitFor(() => expect(location.hash).toBe('#part-a/core'));
  });

  it('opens the entry of a row in its part when clicked', async () => {
    fixedNow();
    mockApi({ state: () => ok(liveState({}, latest)) });
    history.replaceState(null, '', '/workshop#latest');
    await renderApp();
    await userEvent.click(await screen.findByRole('link', { name: /^The engine runs on its own/ }));
    expect(location.hash).toBe('#part-a/core');
    expect(screen.getByRole('heading', { level: 2, name: 'Part A' })).toBeInTheDocument();
    await waitFor(() => expect(rowButton('The engine runs on its own')).toHaveAttribute('aria-expanded', 'true'));
  });

  it('says so when nothing happened yet, and when the live state is offline', async () => {
    mockApi({ state: () => ok(liveState()) });
    history.replaceState(null, '', '/workshop#latest');
    const first = await renderApp();
    expect(await screen.findByText('Nothing yet.')).toBeInTheDocument();
    first.unmount();
    // A first visit: nothing remembered.
    localStorage.clear();
    mockApi({ state: () => refused(503, 'not configured') });
    await renderApp();
    expect(await screen.findByText('Latest is offline.')).toBeInTheDocument();
  });
});

describe('decisions from the plan', () => {
  it('are rows of their part: the asks, then the answer and the notes once open', async () => {
    mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#part-a/core-in-zig');
    await renderApp();
    await waitFor(() => expect(rowButton('Which language is the core written in?')).toHaveAttribute('aria-expanded', 'true'));
    expect(screen.getByText('Zig. It is the core piece.')).toBeInTheDocument();
    expect(screen.getByText('Decided.')).toBeInTheDocument();
    await userEvent.click(notesButton());
    expect(await screen.findByText('Zig keeps the engine small.')).toBeInTheDocument();
    // A decision is never offered for a verdict here, and no row lists "Decisions" any more.
    expect(screen.queryByRole('button', { name: /^Take/ })).not.toBeInTheDocument();
    await userEvent.click(rowButton('The engine runs on its own'));
    expect(screen.queryByRole('button', { name: /^Decisions/ })).not.toBeInTheDocument();
  });
});

describe('the status of an entry', () => {
  const built = (id: string) => buildData({ features: FEATURES.map((feature) => (feature.id === id ? { ...feature, verdict: 'take' as const, built: '2026-11-16' } : feature)) });

  it('is built when the file says so, whatever the live verdict label says', async () => {
    mockApi({ state: () => ok(liveState({ 14: issue({ entry: 'fork', verdict: 'drop', title: 'Fork a computer' }) })) });
    history.replaceState(null, '', '/workshop#part-d/fork');
    await renderApp(built('fork'));
    await waitFor(() => expect(rowButton('Fork a computer')).toHaveAttribute('aria-expanded', 'true'));
    expect(within(itemOf('Fork a computer')).getByText('built')).toBeInTheDocument();
    expect(screen.getByText('Built, 16 Nov.')).toBeInTheDocument();
  });
});
