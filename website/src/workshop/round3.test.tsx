// Enter only for verdict buttons a person sees, a tap during a view transition ignored, and the owner's tally.
import { act, fireEvent, render, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { App, OPEN_GUARD_MS } from './App';
import { buildData, issue, liveState } from './fixtures';
import { itemOf, mockApi, ok, owner, posts, renderApp, rowButton, tile } from './test-support';
import type { Feature } from './plan';

const wait = (ms: number) => act(async () => { await new Promise((resolve) => setTimeout(resolve, ms)); });
const away = { top: -900, bottom: -860, left: 0, right: 800, width: 800, height: 40, x: 0, y: -900, toJSON() {} } as DOMRect;
const scrollIntoView = Element.prototype.scrollIntoView;
afterEach(() => { Element.prototype.scrollIntoView = scrollIntoView; });

describe('Enter', () => {
  it('never takes for a row whose verdict buttons are out of sight, and a row made current is brought into view and re-armed', async () => {
    const calls = mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#computers');
    await renderApp();
    const scrolled: string[] = [];
    Element.prototype.scrollIntoView = function scroll(this: Element) { scrolled.push(this.closest('.ws-item')?.id ?? ''); };
    await userEvent.click(rowButton('Create and delete a computer'));
    await wait(OPEN_GUARD_MS + 50);
    await userEvent.click(rowButton('Run a command'));
    await wait(OPEN_GUARD_MS + 50);
    // The first row's buttons are scrolled far above the window.
    const first = itemOf('Create and delete a computer').querySelector('.ws-btn.go')?.closest('.ws-state') as HTMLElement;
    first.getBoundingClientRect = () => away;
    // With the focus on its own row button, the out-of-sight row is not taken.
    act(() => rowButton('Create and delete a computer').focus());
    fireEvent.keyDown(rowButton('Create and delete a computer'), { key: 'Enter' });
    expect(posts(calls, 'verdict')).toHaveLength(0);
    // With the focus on the page, the row opened last is taken; the earlier one becomes current, is brought
    // into view and waits its guard again.
    act(() => (document.activeElement as HTMLElement).blur());
    scrolled.length = 0;
    fireEvent.keyDown(document.body, { key: 'Enter' });
    expect(posts(calls, 'verdict').map((call) => call.body?.n)).toEqual([13]);
    await wait(300);
    expect(scrolled).toContain('entry-computers');
    expect(location.hash).toBe('#computers/computers');
    fireEvent.keyDown(document.activeElement as HTMLElement, { key: 'Enter' });
    await wait(OPEN_GUARD_MS);
    // Armed again, but still out of sight here: nothing.
    fireEvent.keyDown(document.activeElement as HTMLElement, { key: 'Enter' });
    expect(posts(calls, 'verdict')).toHaveLength(1);
  });
});

describe('Enter on the header of a closed row', () => {
  it('sends no verdict for the open row and leaves the header to open its own row', async () => {
    const calls = mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#computers');
    await renderApp();
    await userEvent.click(rowButton('Create and delete a computer'));
    await wait(OPEN_GUARD_MS + 50);
    const b = rowButton('Run a command');
    act(() => b.focus());
    // The keyboard path of a button: keydown, then the browser's click unless the keydown was prevented.
    const event = fireEvent.keyDown(b, { key: 'Enter' });
    expect(event).toBe(true);
    if (event) await userEvent.click(b);
    expect(posts(calls, 'verdict')).toHaveLength(0);
    expect(b).toHaveAttribute('aria-expanded', 'true');
    expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'true');
  });
});

describe('a tap during a view transition', () => {
  function narrow() {
    vi.stubGlobal('matchMedia', (query: string) => ({ matches: false, media: query, addEventListener() {}, removeEventListener() {} }));
  }
  function transitions() {
    let pending: (() => void) | null = null;
    (document as unknown as { startViewTransition: unknown }).startViewTransition = (update: () => void) => {
      pending = update;
      return { finished: new Promise(() => {}), ready: new Promise(() => {}), skipTransition() { pending?.(); pending = null; } };
    };
    return () => { pending?.(); pending = null; };
  }
  afterEach(() => { delete (document as unknown as { startViewTransition?: unknown }).startViewTransition; });

  it('is ignored, and a press without a release leaves the next tap with one effect', async () => {
    narrow();
    const run = transitions();
    const calls = mockApi({ me: owner });
    await renderApp();
    await userEvent.click(tile('computers'));
    // A pan that starts on the transition's picture: the browser sends pointercancel, no pointerup.
    act(() => { fireEvent.pointerDown(document.documentElement, { pointerType: 'touch' }); fireEvent.pointerCancel(document.documentElement, { pointerType: 'touch' }); });
    act(() => run());
    await userEvent.click(rowButton('Create and delete a computer'));
    expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'true');
    await userEvent.click(within(itemOf('Run a command')).getByRole('button', { name: /4 votes/ }));
    await waitFor(() => expect(posts(calls, 'vote')).toHaveLength(1));
    await wait(50);
    expect(posts(calls, 'vote')).toHaveLength(1);
  });

  it('never turns a release of another button into a click', async () => {
    narrow();
    const run = transitions();
    mockApi();
    await renderApp();
    await userEvent.click(tile('engine'));
    act(() => run());
    const row = rowButton('The engine runs on its own');
    act(() => { fireEvent.pointerDown(document.documentElement, { button: 2 }); fireEvent.pointerUp(row, { button: 2 }); });
    await wait(30);
    expect(row).toHaveAttribute('aria-expanded', 'false');
  });
});

describe('the owner\'s tally', () => {
  it('counts only undecided entries with an issue, whatever their file says elsewhere', async () => {
    const data = buildData();
    const base = data.features.find((feature) => feature.id === 'exec') as Feature;
    // Entries whose file already holds a verdict never wait for the owner; undecided ones with an issue do.
    const decided = (['take', 'later', 'drop'] as const).map((verdict, i): Feature => ({ ...base, position: 30 + i, id: `c-${verdict}`, name: `Decided ${verdict}`, verdict }));
    const open = { ...base, position: 40, id: 'c-open', name: 'Open one', verdict: null } as Feature;
    data.features = [...data.features, ...decided, open];
    mockApi({ me: owner, state: () => ok(liveState(Object.fromEntries([...decided, open].map((feature, i) => [50 + i, issue({ entry: feature.id, title: feature.name })])))) });
    history.replaceState(null, '', '/workshop#computers');
    render(<App data={data} />);
    await waitFor(() => expect(document.querySelector('.ws-detail .ws-tally')).toHaveTextContent(/to decide/));
    // `computers`, `exec` and the new open one wait; the decided ones count as what their file says.
    expect(document.querySelector('.ws-detail .ws-tally')).toHaveTextContent('1 planned · 3 to decide · 1 later · 1 dropped');
  });
});
