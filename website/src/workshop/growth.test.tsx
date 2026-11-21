// An open row's panel grows and shrinks over the same 260 ms when Notes or History opens or closes, and when their
// chunk arrives after its placeholder: the rows below never jump.
import { act, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { buildData, fixtureHistories, fixtureNotes } from './fixtures';
import type { Histories, Notes } from './plan';
import type { HistorySource } from './data';
import { itemOf, mockApi, owner, renderApp, rowButton, wait } from './test-support';

/** A little over the time the panel takes to open, during which it has no growth of its own. */
const GROW_MS = 300;

const CORE = 'The engine runs on its own';
/** Three changes of `core`, so its row has a History. */
const changes: HistorySource = {
  shallow: false,
  changes: new Map([['core', [3, 2, 1].map((n) => ({ commit: n.toString(16).padStart(40, '0'), date: `2026-11-1${n}T09:00:00Z`, what: ['added' as const] }))]]),
};
const data = () => buildData(undefined, changes);

/**
 * A screen that wants motion, where a panel's height is what its style sets, or else 10 px per element it holds: jsdom
 * lays nothing out.
 */
function layout() {
  vi.stubGlobal('matchMedia', (query: string) => ({ matches: false, media: query, addEventListener() {}, removeEventListener() {} }));
  const plain = HTMLElement.prototype.getBoundingClientRect;
  vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
    if (!this.classList.contains('ws-more-in')) return plain.call(this);
    const height = this.style.height ? parseFloat(this.style.height) : this.querySelectorAll('*').length * 10;
    return { x: 0, y: 0, top: 0, left: 0, right: 600, bottom: height, width: 600, height, toJSON() {} } as DOMRect;
  });
}

/** Each height the panel's style is set to, in order. */
function heights(panel: HTMLElement) {
  const seen: string[] = [];
  const note = (height: string | undefined) => { if (height && seen[seen.length - 1] !== height) seen.push(height); };
  // The records are read in a batch: each one's old value is the style before it, the last style is the panel's now.
  const watcher = new MutationObserver((records) => {
    for (const record of records.slice(1)) note(/height:\s*([\d.]+px)/.exec(record.oldValue ?? '')?.[1]);
    note(panel.style.height);
  });
  watcher.observe(panel, { attributes: true, attributeFilter: ['style'], attributeOldValue: true });
  return { seen, stop: () => watcher.disconnect() };
}

function deferred<T>() {
  let release: (value: T) => void = () => {};
  const promise = new Promise<T>((resolve) => { release = resolve; });
  return { promise, release };
}

async function openCore(loadNotes: () => Promise<Notes>, loadHistory: () => Promise<Histories>) {
  layout();
  mockApi({ me: owner });
  history.replaceState(null, '', '/workshop#part-a/core');
  await renderApp(data(), loadNotes, undefined, loadHistory);
  await waitFor(() => expect(rowButton(CORE)).toHaveAttribute('aria-expanded', 'true'));
  const item = itemOf(CORE);
  // The panel's opening is over.
  await waitFor(() => expect(item.querySelector<HTMLElement>('.ws-more-in')!.style.height).toBe(''), { timeout: 1000 });
  await wait(GROW_MS);
  return { item, panel: item.querySelector<HTMLElement>('.ws-more-in')! };
}

describe('the panel of an open row', () => {
  it('grows to Notes, from its placeholder to the notes, and shrinks when they close', async () => {
    const notes = deferred<Notes>();
    const { item, panel } = await openCore(() => notes.promise, () => Promise.resolve(fixtureHistories(data())));
    const button = within(item).getByRole('button', { name: 'Notes' });
    let watch = heights(panel);
    await userEvent.click(button);
    expect(item.querySelector('.ws-ph-notes')).not.toBeNull();
    // From the height before, with the change of height animated.
    expect(watch.seen.length).toBeGreaterThanOrEqual(2);
    expect(parseFloat(watch.seen[0])).toBeLessThan(parseFloat(watch.seen[watch.seen.length - 1]));
    expect(panel.style.transition).toContain('height');
    watch.stop();
    watch = heights(panel);
    await act(async () => notes.release(fixtureNotes(data())));
    await waitFor(() => expect(item.querySelector('.ws-md')).not.toBeNull());
    expect(watch.seen.length).toBeGreaterThanOrEqual(2);
    expect(panel.style.transition).toContain('height');
    watch.stop();
    await waitFor(() => expect(panel.style.height).toBe(''), { timeout: 1000 });
    watch = heights(panel);
    await userEvent.click(button);
    expect(watch.seen.length).toBeGreaterThanOrEqual(2);
    expect(parseFloat(watch.seen[0])).toBeGreaterThan(parseFloat(watch.seen[watch.seen.length - 1]));
    watch.stop();
  });

  it('grows to History, and again when its changes arrive after their placeholders', async () => {
    const later = deferred<Histories>();
    const { item, panel } = await openCore(() => Promise.resolve(fixtureNotes(data())), () => later.promise);
    let watch = heights(panel);
    await userEvent.click(within(item).getByRole('button', { name: /^History/ }));
    expect(item.querySelector('.ws-h-ghost')).not.toBeNull();
    expect(watch.seen.length).toBeGreaterThanOrEqual(2);
    expect(panel.style.transition).toContain('height');
    watch.stop();
    await waitFor(() => expect(panel.style.height).toBe(''), { timeout: 1000 });
    watch = heights(panel);
    await act(async () => later.release(fixtureHistories(data())));
    await waitFor(() => expect(item.querySelector('.ws-h')).not.toBeNull());
    expect(watch.seen.length).toBeGreaterThanOrEqual(2);
    watch.stop();
  });
});

describe('a row closed and opened again while its Notes load', () => {
  it('lets the panel open to the notes that arrive meanwhile, with no height of its own pinned', async () => {
    const notes = deferred<Notes>();
    const { item, panel } = await openCore(() => notes.promise, () => Promise.resolve(fixtureHistories(data())));
    await userEvent.click(within(item).getByRole('button', { name: 'Notes' }));
    await waitFor(() => expect(panel.style.height).toBe(''), { timeout: 1000 });
    // Closed, then opened again before its body is taken away; the notes arrive while it opens.
    await userEvent.click(rowButton(CORE));
    await userEvent.click(rowButton(CORE));
    await waitFor(() => expect(item.querySelector('.ws-more')).toHaveClass('open'));
    expect(item.querySelector('.ws-ph-notes')).not.toBeNull();
    const watch = heights(panel);
    await act(async () => notes.release(fixtureNotes(data())));
    await waitFor(() => expect(item.querySelector('.ws-md')).not.toBeNull());
    expect(watch.seen).toEqual([]);
    expect(panel.style.height).toBe('');
    watch.stop();
  });
});
