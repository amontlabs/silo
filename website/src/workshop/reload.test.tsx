// What a person typed survives the page's own reload: the drafts are kept for the tab as they are typed and put back
// in their boxes, the rows they belong to open again, and Reload waits for a write being sent.
import { act, cleanup, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from './App';
import { buildData, fixtureFacts, fixtureHistories, fixtureNotes } from './fixtures';
import { recallDrafts, rememberDrafts, takeReopen } from './memory';
import type { Drafts } from './memory';
import { itemOf, mockApi, ok, owner, posts, refused, renderApp, rowButton, wait } from './test-support';
import type { Reply } from './test-support';
import { pageConfig } from './useWorkshop';

// `core` has notes; `calls` is beside it in Part A.
const CORE = 'The engine runs on its own';
const CALLS = 'Every call defined once';
const commentBox = (item: HTMLElement) => within(item).getByRole('textbox', { name: 'Comment' });
const kept = () => JSON.parse(sessionStorage.getItem('silo-workshop-drafts') ?? 'null') as unknown;

let reloads = 0;
beforeEach(() => {
  reloads = 0;
  vi.spyOn(pageConfig, 'reload').mockImplementation(() => { reloads += 1; });
});
afterEach(() => vi.unstubAllGlobals());

/** The page after a reload: what the tab kept, at the address it had, with the rows the reload asked to open. */
async function reloaded(data = buildData()) {
  cleanup();
  const reopen = takeReopen();
  render(<App data={data} loadNotes={() => Promise.resolve(fixtureNotes(data))} loadFacts={() => Promise.resolve(fixtureFacts(data))} loadHistory={() => Promise.resolve(fixtureHistories(data))} reopen={reopen} />);
  await waitFor(() => expect(document.querySelector('.ws[data-live="answered"]')).not.toBeNull());
}

/** A failed notes chunk on `core`'s row, whose Reload is then pressed. */
async function pressReload(item: HTMLElement) {
  await userEvent.click(within(item).getByRole('button', { name: 'Notes' }));
  await userEvent.click(await within(item).findByRole('button', { name: 'Reload' }));
}

describe('a comment being written', () => {
  it('is kept for the tab as it is typed, and is back in its box, its row open, after a reload', async () => {
    mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#part-a/core');
    await renderApp();
    await userEvent.type(commentBox(itemOf(CORE)), 'Half a thought');
    expect(kept()).toEqual({ comments: { 'entry-core': 'Half a thought' } });
    // Nothing of it goes to the site: no write was sent.
    await reloaded();
    await waitFor(() => expect(rowButton(CORE)).toHaveAttribute('aria-expanded', 'true'));
    expect(commentBox(itemOf(CORE))).toHaveValue('Half a thought');
  });

  it('is gone from what is kept once sent, and is not put back after a reload', async () => {
    const calls = mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#part-a/core');
    await renderApp();
    await userEvent.type(commentBox(itemOf(CORE)), 'Sent once.');
    await userEvent.click(within(itemOf(CORE)).getByRole('button', { name: 'Comment' }));
    await waitFor(() => expect(posts(calls, 'comment')).toHaveLength(1));
    expect(kept()).toBeNull();
    await reloaded();
    await waitFor(() => expect(rowButton(CORE)).toHaveAttribute('aria-expanded', 'true'));
    expect(commentBox(itemOf(CORE))).toHaveValue('');
  });

  it('goes back into the box of its own row only', async () => {
    mockApi({ me: owner });
    rememberDrafts({ comments: { 'entry-calls': 'For the snapshot.' }, suggest: { title: '', text: '' } });
    history.replaceState(null, '', '/workshop#part-a/core');
    await renderApp();
    expect(commentBox(itemOf(CORE))).toHaveValue('');
    await userEvent.click(rowButton(CALLS));
    expect(commentBox(itemOf(CALLS))).toHaveValue('For the snapshot.');
  });

  it('types as ever when session storage refuses it', async () => {
    mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#part-a/core');
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => { throw new DOMException('full', 'QuotaExceededError'); });
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => { throw new DOMException('denied', 'SecurityError'); });
    await renderApp();
    await userEvent.type(commentBox(itemOf(CORE)), 'Still typed.');
    expect(commentBox(itemOf(CORE))).toHaveValue('Still typed.');
  });
});

describe('a suggestion being written', () => {
  it('is kept as it is typed and is back in its boxes after a reload', async () => {
    mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#suggested');
    await renderApp();
    await userEvent.type(await screen.findByRole('textbox', { name: 'Title' }), 'Pause');
    await userEvent.type(screen.getByRole('textbox', { name: 'Why' }), 'To come back later.');
    await reloaded();
    expect(await screen.findByRole('textbox', { name: 'Title' })).toHaveValue('Pause');
    expect(screen.getByRole('textbox', { name: 'Why' })).toHaveValue('To come back later.');
  });
});

describe('Reload', () => {
  it('opens again the rows that were open and those with a draft, where the place shown holds them', async () => {
    mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#part-a/core');
    await renderApp();
    // A draft on the row of `calls`, which is then closed; the address names `core`, opened last.
    await userEvent.click(rowButton(CALLS));
    await userEvent.type(commentBox(itemOf(CALLS)), 'Left here.');
    await userEvent.click(rowButton(CALLS));
    await userEvent.click(rowButton(CORE));
    await userEvent.click(rowButton(CORE));
    expect(location.hash).toBe('#part-a/core');
    const failing = () => Promise.reject(new Error('offline'));
    cleanup();
    render(<App data={buildData()} loadNotes={failing} />);
    await waitFor(() => expect(rowButton(CORE)).toHaveAttribute('aria-expanded', 'true'));
    await pressReload(itemOf(CORE));
    expect(reloads).toBe(1);
    await reloaded();
    await waitFor(() => expect(rowButton(CALLS)).toHaveAttribute('aria-expanded', 'true'));
    expect(rowButton(CORE)).toHaveAttribute('aria-expanded', 'true');
    expect(commentBox(itemOf(CALLS))).toHaveValue('Left here.');
    // The rows are opened once: the next load opens only what its address names.
    await reloaded();
    await waitFor(() => expect(rowButton(CORE)).toHaveAttribute('aria-expanded', 'true'));
    expect(rowButton(CALLS)).toHaveAttribute('aria-expanded', 'false');
  });

  it('waits for a comment being sent; one that fails is back in its box, and kept, before the page leaves', async () => {
    let answer: (reply: Reply) => void = () => {};
    mockApi({ me: owner, routes: { 'POST comment': () => new Promise((resolve) => { answer = resolve; }) } });
    history.replaceState(null, '', '/workshop#part-a/core');
    cleanup();
    const data = buildData();
    render(<App data={data} loadNotes={() => Promise.reject(new Error('offline'))} loadFacts={() => Promise.resolve(fixtureFacts(data))} />);
    await waitFor(() => expect(rowButton(CORE)).toHaveAttribute('aria-expanded', 'true'));
    const item = itemOf(CORE);
    await userEvent.type(commentBox(item), 'Under way.');
    await userEvent.click(within(item).getByRole('button', { name: 'Comment' }));
    expect(commentBox(item)).toHaveValue('');
    await pressReload(item);
    await wait(20);
    expect(reloads).toBe(0);
    expect(within(item).getByRole('button', { name: 'Reload' })).toHaveAttribute('aria-busy', 'true');
    await act(async () => answer(refused(502, 'GitHub did not answer.')));
    await waitFor(() => expect(reloads).toBe(1));
    expect(commentBox(item)).toHaveValue('Under way.');
    expect(kept()).toEqual({ comments: { 'entry-core': 'Under way.' } });
    await reloaded(data);
    await waitFor(() => expect(rowButton(CORE)).toHaveAttribute('aria-expanded', 'true'));
    expect(commentBox(itemOf(CORE))).toHaveValue('Under way.');
  });

  it('waits for a comment being sent; one that is confirmed is not put back', async () => {
    let answer: (reply: Reply) => void = () => {};
    mockApi({ me: owner, routes: { 'POST comment': () => new Promise((resolve) => { answer = resolve; }) } });
    history.replaceState(null, '', '/workshop#part-a/core');
    cleanup();
    render(<App data={buildData()} loadNotes={() => Promise.reject(new Error('offline'))} />);
    await waitFor(() => expect(rowButton(CORE)).toHaveAttribute('aria-expanded', 'true'));
    const item = itemOf(CORE);
    await userEvent.type(commentBox(item), 'Confirmed.');
    await userEvent.click(within(item).getByRole('button', { name: 'Comment' }));
    await pressReload(item);
    await wait(20);
    expect(reloads).toBe(0);
    await act(async () => answer(ok({ n: 10, comments: 1 })));
    await waitFor(() => expect(reloads).toBe(1));
    expect(kept()).toBeNull();
  });
});

describe('the drafts kept for the tab', () => {
  // Random sequences of typing, sending (which empties a box) and storage going away and coming back, checked after
  // every step against a model: with storage, what is kept is exactly what the boxes hold; without it, typing goes on
  // and what was kept before stays as it was.
  it('match what the boxes hold after every step', () => {
    let seed = 7;
    const random = (n: number) => {
      seed = (seed * 1103515245 + 12345) % 2 ** 31;
      return seed % n;
    };
    const keys = ['entry-core', 'entry-core', 'suggestion-12'];
    const words = ['', 'a', 'Half a thought', ' ', 'x'.repeat(6000)];
    for (let run = 0; run < 40; run += 1) {
      sessionStorage.clear();
      vi.restoreAllMocks();
      let broken = false;
      const boxes: Drafts = { comments: {}, suggest: { title: '', text: '' } };
      let model: Drafts = { comments: {}, suggest: { title: '', text: '' } };
      for (let step = 0; step < 30; step += 1) {
        const op = random(5);
        if (op === 0) boxes.comments = { ...boxes.comments, [keys[random(keys.length)]]: words[random(words.length)] };
        else if (op === 1) boxes.comments = { ...boxes.comments, [keys[random(keys.length)]]: '' };
        else if (op === 2) boxes.suggest = { title: words[random(words.length)].slice(0, 120), text: boxes.suggest.text };
        else if (op === 3) boxes.suggest = { title: boxes.suggest.title, text: words[random(words.length)] };
        else {
          broken = !broken;
          vi.restoreAllMocks();
          if (broken) {
            vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => { throw new DOMException('denied', 'SecurityError'); });
            vi.spyOn(Storage.prototype, 'removeItem').mockImplementation(() => { throw new DOMException('denied', 'SecurityError'); });
          }
        }
        expect(() => rememberDrafts(boxes)).not.toThrow();
        if (!broken) {
          model = {
            comments: Object.fromEntries(Object.entries(boxes.comments).filter(([, text]) => text !== '').map(([key, text]) => [key, text.slice(0, 5000)])),
            suggest: { title: boxes.suggest.title, text: boxes.suggest.text.slice(0, 5000) },
          };
        }
        expect(recallDrafts(), `run ${run}, step ${step}`).toEqual(model);
      }
    }
  });
});
