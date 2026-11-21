// Reload, offered where a chunk failed: it reloads at once, unless the page holds text typed and not sent or a write
// being sent; then the first press says so and the second reloads.
import { act, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { App } from './App';
import { buildData, fixtureFacts } from './fixtures';
import { mockApi, owner, rowButton } from './test-support';
import type { Reply } from './test-support';
import { pageConfig } from './useWorkshop';

// `core` has notes; its notes chunk fails here.
const CORE = 'The engine runs on its own';
const UNSENT = 'Text you typed is not sent yet.';

let reloads = 0;
beforeEach(() => {
  reloads = 0;
  vi.spyOn(pageConfig, 'reload').mockImplementation(() => { reloads += 1; });
});

/** The page at `core`'s row, open, with its notes chunk failing; returns the row's panel. */
async function openCore(routes = {}) {
  mockApi({ me: owner, routes });
  history.replaceState(null, '', '/workshop#part-a/core');
  const data = buildData();
  render(<App data={data} loadNotes={() => Promise.reject(new Error('offline'))} loadFacts={() => Promise.resolve(fixtureFacts(data))} />);
  await screen.findByRole('textbox', { name: 'Comment' });
  expect(rowButton(CORE)).toHaveAttribute('aria-expanded', 'true');
  return document.getElementById('entry-core-more') as HTMLElement;
}

async function failNotes(panel: HTMLElement) {
  await userEvent.click(within(panel).getByRole('button', { name: 'Notes' }));
  return within(panel).findByRole('button', { name: 'Reload' });
}

describe('Reload', () => {
  it('reloads at once when nothing is typed and nothing is being sent', async () => {
    const panel = await openCore();
    await userEvent.click(await failNotes(panel));
    expect(reloads).toBe(1);
  });

  it('with a comment typed, says so at the first press and reloads at the second', async () => {
    const panel = await openCore();
    await userEvent.type(within(panel).getByRole('textbox', { name: 'Comment' }), 'Half a thought');
    await userEvent.click(await failNotes(panel));
    expect(reloads).toBe(0);
    expect(within(panel).getByText(UNSENT)).toBeInTheDocument();
    expect(within(panel).queryByText(/^Notes could not load\./)).not.toBeInTheDocument();
    await userEvent.click(within(panel).getByRole('button', { name: 'Reload anyway' }));
    expect(reloads).toBe(1);
  });

  it('with a suggestion drafted, says so at the first press', async () => {
    mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#suggested');
    const data = buildData();
    render(<App data={data} loadNotes={() => Promise.reject(new Error('offline'))} />);
    await userEvent.type(await screen.findByRole('textbox', { name: 'Title' }), 'Pause');
    await act(async () => { location.hash = '#part-a/core'; window.dispatchEvent(new HashChangeEvent('hashchange')); });
    await screen.findByRole('textbox', { name: 'Comment' });
    const panel = document.getElementById('entry-core-more') as HTMLElement;
    await userEvent.click(await failNotes(panel));
    expect(reloads).toBe(0);
    expect(within(panel).getByText(UNSENT)).toBeInTheDocument();
  });

  it('with a comment being sent, says so at the first press, though its box is empty', async () => {
    const panel = await openCore({ 'POST comment': () => new Promise<Reply>(() => {}) });
    await userEvent.type(within(panel).getByRole('textbox', { name: 'Comment' }), 'Under way.');
    await userEvent.click(within(panel).getByRole('button', { name: 'Comment' }));
    expect(within(panel).getByRole('textbox', { name: 'Comment' })).toHaveValue('');
    await userEvent.click(await failNotes(panel));
    expect(reloads).toBe(0);
    expect(within(panel).getByText(UNSENT)).toBeInTheDocument();
    await userEvent.click(within(panel).getByRole('button', { name: 'Reload anyway' }));
    expect(reloads).toBe(1);
  });
});
