// The page's height after going back, late rows from the address, the owner's tally, view transitions
// overtaken by a navigation, and a failed early fetch.
import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { App } from './App';
import { buildData, liveState } from './fixtures';
import { detail, itemOf, mockApi, ok, owner, ownerOn, part, refused, renderApp, rowButton, tile } from './test-support';

/** A wide screen with motion, as a desktop browser reports it. */
function desktop(wide = true) {
  vi.stubGlobal('matchMedia', (query: string) => ({
    matches: query.includes('min-width') ? wide : false, media: query, addEventListener() {}, removeEventListener() {},
  }));
}
const wait = (ms: number) => act(async () => { await new Promise((resolve) => setTimeout(resolve, ms)); });

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

describe('a row named by the address', () => {
  it('opens late beside the row the person opened meanwhile, which stays the current one', async () => {
    let release: (value: { status: number; body: unknown }) => void = () => {};
    mockApi({ state: () => new Promise((resolve) => { release = resolve; }) });
    history.replaceState(null, '', '/workshop#computers/exec');
    render(<App data={buildData()} />);
    await userEvent.click(rowButton('Create and delete a computer'));
    expect(location.hash).toBe('#computers/computers');
    await act(async () => release(ok(liveState())));
    await waitFor(() => expect(rowButton('Run a command')).toHaveAttribute('aria-expanded', 'true'));
    expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'true');
    expect(location.hash).toBe('#computers/computers');
  });
});

describe('the owner\'s tally', () => {
  it('counts an entry decided by its file as planned, never as to decide', async () => {
    mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#engine/calls');
    await renderApp();
    await ownerOn('Every call defined once');
    expect(document.querySelector('.ws-detail .ws-tally')).toHaveTextContent(/^2 planned$/);
    expect(within(itemOf('Every call defined once')).queryByText('to decide')).not.toBeInTheDocument();
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

describe('a failed early fetch', () => {
  it('shows held places, not the error, while the opening tries again', async () => {
    let calls = 0;
    mockApi({ routes: { 'GET issue': (url) => { calls += 1; return calls === 1 ? refused(502, 'GitHub failed.') : new Promise(() => detail(Number(url.searchParams.get('n')))); } } });
    history.replaceState(null, '', '/workshop#computers');
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

describe('two rows opened one after the other', () => {
  it('each arm when their own guard has passed', async () => {
    mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#computers');
    await renderApp();
    await userEvent.click(rowButton('Create and delete a computer'));
    await wait(300);
    await userEvent.click(rowButton('Run a command'));
    const take = (name: string) => within(itemOf(name)).getByRole('button', { name: /^Take/ });
    await wait(500);
    expect(take('Create and delete a computer')).not.toHaveClass('ws-unarmed');
    expect(take('Run a command')).toHaveClass('ws-unarmed');
    await wait(300);
    expect(take('Run a command')).not.toHaveClass('ws-unarmed');
  });
});
