// Entries opening and their comments loading: places held, hover fetches, retries and the growth of the panel.
import { act, fireEvent, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { HOLD_MS } from './Entry';
import { clock, detail, itemOf, mockApi, ok, refused, renderApp, rowButton, wait } from './test-support';

describe('comments that arrive late', () => {
  it('hold one place per expected comment while loading, with no loading line, and load on hover', async () => {
    let release: () => void = () => {};
    const calls = mockApi({ routes: { 'GET issue': (url) => new Promise((resolve) => { release = () => resolve(ok(detail(Number(url.searchParams.get('n')), { comments: [1, 2].map((id) => ({ id, author: 'mara-k', avatar: '', bot: false, createdAt: '2026-11-14T10:00:00Z', text: `Comment ${id}.` })) }))); }) } });
    history.replaceState(null, '', '/workshop#computers');
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
    history.replaceState(null, '', '/workshop#computers');
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

describe('an opening panel', () => {
  it('shows the row open at once and grows when the comments are in', async () => {
    let release: () => void = () => {};
    const calls = mockApi({ routes: { 'GET issue': (url) => new Promise((resolve) => { release = () => resolve(ok(detail(Number(url.searchParams.get('n'))))); }) } });
    history.replaceState(null, '', '/workshop#computers');
    await renderApp();
    fireEvent.pointerDown(rowButton('Run a command'));
    expect(calls.filter((call) => call.path === 'issue')).toHaveLength(1);
    await userEvent.click(rowButton('Run a command'));
    const more = itemOf('Run a command').querySelector('.ws-more') as HTMLElement;
    expect(rowButton('Run a command')).toHaveAttribute('aria-expanded', 'true');
    expect(itemOf('Run a command')).toHaveClass('open');
    expect(more).not.toHaveClass('open');
    await act(async () => release());
    expect(more).toHaveClass('open');
  });

  it('grows after the hold when the comments are slow, with their places held', async () => {
    mockApi({ routes: { 'GET issue': () => new Promise(() => {}) } });
    history.replaceState(null, '', '/workshop#computers');
    await renderApp();
    await userEvent.click(rowButton('Create and delete a computer'));
    const item = itemOf('Create and delete a computer');
    expect(item.querySelector('.ws-more')).not.toHaveClass('open');
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, HOLD_MS + 20)); });
    expect(item.querySelector('.ws-more')).toHaveClass('open');
    expect(item.querySelectorAll('.ws-c-ghost')).toHaveLength(2);
  });

  it('opens at once when the comments are already there', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#computers');
    await renderApp();
    fireEvent.pointerEnter(rowButton('Run a command'));
    await waitFor(() => expect(document.querySelector('#entry-exec .ws-more-in')).not.toBeNull());
    await act(async () => { await new Promise((resolve) => setTimeout(resolve, 20)); });
    await userEvent.click(rowButton('Run a command'));
    expect(itemOf('Run a command').querySelector('.ws-more')).toHaveClass('open');
  });
});
