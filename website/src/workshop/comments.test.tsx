// Entries opening and their comments loading: places held, hover fetches, retries and the growth of the panel.
import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { HOLD_MS } from './Entry';
import type { Change } from './plan';
import { historyComment, liveState, SHA } from './fixtures';
import { clock, detail, itemOf, mockApi, ok, refused, renderApp, rowButton, wait } from './test-support';

describe('comments that arrive late', () => {
  it('hold one place per expected comment while loading, with no loading line, and load on hover', async () => {
    let release: () => void = () => {};
    const calls = mockApi({ routes: { 'GET issue': (url) => new Promise((resolve) => { release = () => resolve(ok(detail(Number(url.searchParams.get('n')), { comments: [1, 2].map((id) => ({ id, author: 'mara-k', avatar: '', bot: false, createdAt: '2026-11-14T10:00:00Z', text: `Comment ${id}.` })) }))); }) } });
    history.replaceState(null, '', '/workshop#part-b');
    await renderApp();
    fireEvent.pointerEnter(rowButton('Create and delete a computer'));
    expect(calls.filter((call) => call.path === 'issue')).toHaveLength(1);
    await userEvent.click(rowButton('Create and delete a computer'));
    // The hover's fetch serves the opening.
    expect(calls.filter((call) => call.path === 'issue')).toHaveLength(1);
    const item = itemOf('Create and delete a computer');
    expect(item.querySelectorAll('.ws-c-ghost')).toHaveLength(2);
    expect(within(item).queryByText(/Loading/)).not.toBeInTheDocument();
    await act(async () => release());
    expect(item.querySelectorAll('.ws-c-ghost')).toHaveLength(0);
    expect(item.querySelectorAll('.ws-c')).toHaveLength(2);
  });

  it('are fetched again on every opening, and a failed load is tried again', async () => {
    let fail = true;
    const calls = mockApi({ routes: { 'GET issue': (url) => (fail ? refused(502, 'GitHub failed.') : ok(detail(Number(url.searchParams.get('n'))))) } });
    const time = clock();
    history.replaceState(null, '', '/workshop#part-b');
    await renderApp();
    const fetches = () => calls.filter((call) => call.path === 'issue').length;
    await userEvent.click(rowButton('Run a command'));
    expect(await screen.findByText('Comments could not load.')).toBeInTheDocument();
    const before = fetches();
    fail = false;
    await userEvent.click(rowButton('Run a command'));
    time.advance(10_000);
    await userEvent.click(rowButton('Run a command'));
    await waitFor(() => expect(screen.queryByText('Comments could not load.')).not.toBeInTheDocument());
    expect(fetches()).toBe(before + 1);
    await userEvent.click(rowButton('Run a command'));
    time.advance(10_000);
    await userEvent.click(rowButton('Run a command'));
    await waitFor(() => expect(fetches()).toBe(before + 2));
  });
});

describe('a failed early fetch', () => {
  it('shows held places, not the error, while the opening tries again', async () => {
    let calls = 0;
    mockApi({ routes: { 'GET issue': (url) => { calls += 1; return calls === 1 ? refused(502, 'GitHub failed.') : new Promise(() => detail(Number(url.searchParams.get('n')))); } } });
    history.replaceState(null, '', '/workshop#part-b');
    await renderApp();
    fireEvent.pointerEnter(rowButton('Create and delete a computer'));
    await waitFor(() => expect(calls).toBe(1));
    await wait(10);
    await userEvent.click(rowButton('Create and delete a computer'));
    const item = itemOf('Create and delete a computer');
    expect(within(item).queryByText('Comments could not load.')).not.toBeInTheDocument();
    expect(item.querySelectorAll('.ws-c-ghost')).toHaveLength(2);
    expect(calls).toBe(2);
  });
});

describe('an opening panel', () => {
  // What the live state announces for a row: its own comments and its history lines.
  const expectedOf = (name: string) => {
    const found = Object.entries(liveState().issues).find(([, live]) => live.title === name);
    const live = found![1];
    const updates = Math.max(0, Math.min(live.updates ?? 0, live.comments));
    return { n: Number(found![0]), comments: live.comments - updates, updates };
  };
  const threadOf = (name: string) => {
    const { comments, updates } = expectedOf(name);
    return [
      ...Array.from({ length: updates }, (_, i) => historyComment(100 + i, { fields: ['verdict'], before: SHA.a, commit: SHA.b, verdict: 'take' } as Change, '2026-11-12T10:00:00Z')),
      ...Array.from({ length: comments }, (_, i) => ({ id: i + 1, author: 'mara-k', avatar: '', bot: false, createdAt: '2026-11-14T10:00:00Z', text: `Comment ${i + 1}.` })),
    ];
  };
  // What the open panel shows right now: the whole thread, one placeholder per expected comment and history line, or
  // something that must never be seen (nothing, a lone line, part of the thread).
  const shape = (item: HTMLElement, want: { comments: number; updates: number }) => {
    const talk = item.querySelector('.ws-talk');
    if (!talk) return 'no panel';
    const ghostComments = talk.querySelectorAll('.ws-c-ghost').length;
    const ghostUpdates = talk.querySelectorAll('.ws-h-ghost').length;
    const comments = talk.querySelectorAll('.ws-c:not(.ws-pending)').length;
    const updates = talk.querySelectorAll('.ws-h').length;
    if (talk.querySelector('.ws-quiet') || /Loading|could not/i.test(talk.textContent ?? '')) return 'a lone line';
    if (ghostComments + ghostUpdates > 0) {
      return comments + updates === 0 && ghostComments === want.comments && ghostUpdates === want.updates ? 'placeholders' : `mixed: ${ghostComments}+${ghostUpdates} places, ${comments}+${updates} lines`;
    }
    return comments === want.comments && updates === want.updates ? 'thread' : `partial: ${comments}+${updates} of ${want.comments}+${want.updates}`;
  };

  const modes = ['cached', 'quick', 'slow', 'failed then retried'] as const;
  describe.each(['Create and delete a computer', 'Run a command'])('%s', (name) => {
    it.each(modes)('is never empty or lone, and grows once, when its comments are %s', async (mode) => {
      const want = expectedOf(name);
      const answer = (n: number) => ok(detail(n, { comments: threadOf(name) }));
      let calls = 0;
      let arrived = false;
      mockApi({
        routes: {
          'GET issue': (url) => {
            calls += 1;
            const n = Number(url.searchParams.get('n'));
            if (mode === 'cached') return answer(n);
            if (mode === 'failed then retried' && calls === 1) return refused(502, 'GitHub failed.');
            return new Promise((resolve) => setTimeout(() => { arrived = true; resolve(answer(n)); }, mode === 'quick' ? HOLD_MS / 4 : HOLD_MS + 100));
          },
        },
      });
      history.replaceState(null, '', '/workshop#part-b');
      await renderApp();
      if (mode === 'cached' || mode === 'failed then retried') {
        fireEvent.pointerEnter(rowButton(name));
        await waitFor(() => expect(calls).toBe(1));
        await wait(20);
      }
      const item = itemOf(name);
      const seen: string[] = [];
      const grown: boolean[] = [];
      const look = () => {
        if (rowButton(name).getAttribute('aria-expanded') !== 'true') return;
        const now = shape(item, want);
        if (seen[seen.length - 1] !== now) seen.push(now);
        const more = item.querySelector('.ws-more')!.classList.contains('open');
        if (grown[grown.length - 1] !== more) grown.push(more);
        // Whenever the panel is shown, it is in one of the two steady shapes.
        if (more) expect(['placeholders', 'thread']).toContain(now);
        // Comments that come within the hold are in before the panel opens, so it opens at its final size.
        if (more && mode === 'quick') expect(now).toBe('thread');
      };
      const watcher = new MutationObserver(look);
      watcher.observe(item, { subtree: true, childList: true, attributes: true, characterData: true });
      await userEvent.click(rowButton(name));
      // The row says it is open at once.
      expect(rowButton(name)).toHaveAttribute('aria-expanded', 'true');
      expect(item).toHaveClass('open');
      look();
      if (mode === 'cached') expect(item.querySelector('.ws-more')).toHaveClass('open');
      if (mode === 'slow' || mode === 'failed then retried') {
        // A slow panel opens no later than the hold, on its placeholders, and not before the row said it was open.
        await waitFor(() => expect(item.querySelector('.ws-more')).toHaveClass('open'), { timeout: HOLD_MS + 150 });
        if (want.comments + want.updates > 0) expect(arrived).toBe(false);
        await waitFor(() => expect(arrived).toBe(true), { timeout: 2000 });
      }
      await waitFor(() => expect(item.querySelector('.ws-more')).toHaveClass('open'), { timeout: HOLD_MS + 150 });
      await wait(HOLD_MS + 20);
      look();
      watcher.disconnect();
      expect(shape(item, want)).toBe('thread');
      expect(item.querySelector('.ws-more')).toHaveClass('open');
      // Every step was a steady shape, in one order: places, then the thread, once, never back.
      expect(seen.filter((shapeNow) => shapeNow !== 'placeholders' && shapeNow !== 'thread')).toEqual([]);
      expect(seen[seen.length - 1]).toBe('thread');
      expect(seen.filter((shapeNow) => shapeNow === 'thread')).toHaveLength(1);
      expect(seen.length).toBeLessThanOrEqual(2);
      // The panel opened once and stayed open.
      expect(grown.filter(Boolean)).toHaveLength(1);
      expect(grown[grown.length - 1]).toBe(true);
    });
  });
});
