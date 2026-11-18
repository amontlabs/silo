// Moves between places: the Map/Order swap, the page height kept while the map unfolds, a view transition that
// a navigation overtakes, and taps during one.
import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { itemOf, mockApi, owner, part, posts, renderApp, rowButton, tile, wait } from './test-support';

/** A wide screen with motion, as a desktop browser reports it. */
function desktop(wide = true) {
  vi.stubGlobal('matchMedia', (query: string) => ({
    matches: query.includes('min-width') ? wide : false, media: query, addEventListener() {}, removeEventListener() {},
  }));
}

describe('Map and Order', () => {
  it('end switched and visible when a part is clicked during the swap', async () => {
    vi.stubGlobal('matchMedia', (query: string) => ({ matches: query.includes('min-width'), media: query, addEventListener() {}, removeEventListener() {} }));
    mockApi();
    const { container } = await renderApp();
    await userEvent.click(screen.getByRole('button', { name: 'Order' }));
    await userEvent.click(part('forks'));
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 600)); });
    const root = container.querySelector('.ws') as HTMLElement;
    expect(root).not.toHaveClass('swap-out');
    expect(root).toHaveClass('v-order');
  });
});

describe('going back to the full map', () => {
  it('holds the page height only while the map unfolds, also after a second area', async () => {
    desktop();
    mockApi();
    const { container } = await renderApp();
    const root = container.querySelector('.ws') as HTMLElement;
    for (const area of ['computers', 'forks']) {
      await userEvent.click(part(area));
      await userEvent.click(screen.getByRole('button', { name: /Full map/ }));
      expect(root.style.minHeight).not.toBe('');
      await wait(400);
    }
    // A part clicked while the page leaves does not keep the height either.
    await userEvent.click(part('engine'));
    await userEvent.click(screen.getByRole('button', { name: /Full map/ }));
    await userEvent.click(part('devices'));
    await wait(1200);
    expect(root.style.minHeight).toBe('');
  });
});

describe('a view transition', () => {
  it('never overrides a navigation made while it waits, and a failed one is handled', async () => {
    desktop(false);
    let pending: (() => void) | null = null;
    // Made in the call, as a browser does: the page attaches its handlers at once, so none stays unhandled.
    (document as unknown as { startViewTransition: unknown }).startViewTransition = (update: () => void) => {
      pending = update;
      return { finished: Promise.reject(new Error('skipped')), ready: Promise.reject(new Error('skipped')), updateCallbackDone: Promise.resolve(), skipTransition() {} };
    };
    try {
      mockApi();
      await renderApp();
      await userEvent.click(tile('engine'));
      act(() => { history.pushState(null, '', '/workshop#computers'); dispatchEvent(new PopStateEvent('popstate')); });
      expect(screen.getByRole('heading', { level: 2, name: 'Computers' })).toBeInTheDocument();
      act(() => (pending as unknown as () => void)());
      await wait(20);
      expect(screen.getByRole('heading', { level: 2, name: 'Computers' })).toBeInTheDocument();
      expect(location.hash).toBe('#computers');
    } finally {
      delete (document as unknown as { startViewTransition?: unknown }).startViewTransition;
    }
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
