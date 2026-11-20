// What the plan in plan/ adds to the page: notes, Latest, decisions and the status rule.
import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { App } from './App';
import { buildData, FEATURES, issue, liveState } from './fixtures';
import type { LiveLatest } from './plan';
import { itemOf, mockApi, notesButton, ok, openCore, owner, refused, renderApp, rowButton, shownNotes } from './test-support';

const NOW = Date.parse('2026-11-17T12:00:00Z');
const fixedNow = () => vi.spyOn(Date, 'now').mockReturnValue(NOW);
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

describe('Latest', () => {
  const at = (hours: number) => new Date(NOW - hours * 3_600_000).toISOString();
  const latest: LiveLatest[] = [
    { n: 12, entry: 'create', author: 'mara-k', bot: false, createdAt: at(1), text: 'Keep stop separate.' },
    { n: 10, entry: 'core', author: 'silo-workshop[bot]', bot: true, createdAt: at(5), text: 'Posted by the App.' },
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
      'The engine runs on its ownsilo-workshop[bot]: Posted by the App.',
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
    // No row lists "Decisions" any more.
    await userEvent.click(rowButton('The engine runs on its own'));
    expect(screen.queryByRole('button', { name: /^Decisions/ })).not.toBeInTheDocument();
  });
});

describe('the status of an entry', () => {
  const built = (id: string) => buildData({ features: FEATURES.map((feature) => (feature.id === id ? { ...feature, verdict: 'take' as const, built: '2026-11-16' } : feature)) });

  it('is built when the file says so, whatever its live issue says', async () => {
    mockApi({ state: () => ok(liveState({ 14: issue({ entry: 'fork', state: 'closed', reason: 'not_planned', title: 'Fork a computer' }) })) });
    history.replaceState(null, '', '/workshop#part-d/fork');
    await renderApp(built('fork'));
    await waitFor(() => expect(rowButton('Fork a computer')).toHaveAttribute('aria-expanded', 'true'));
    expect(within(itemOf('Fork a computer')).getByText('built')).toBeInTheDocument();
    expect(screen.getByText('Built, 16 Nov.')).toBeInTheDocument();
  });
});
