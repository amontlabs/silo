// The account slot's width decided before `me` answers, and panels that open at their final size.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { App } from './App';
import { HOLD_MS } from './Entry';
import { buildData } from './fixtures';
import { detail, itemOf, mockApi, ok, owner, refused, renderApp, rowButton } from './test-support';

describe('the account slot', () => {
  const marker = /<script>\s*\/\/ A readable marker[\s\S]*?<\/script>/.exec(readFileSync(join(process.cwd(), 'workshop/index.html'), 'utf8'))?.[0]
    .replace(/<\/?script>/g, '') ?? '';

  it('is marked for the signed-in width before first paint when the session marker is there', () => {
    document.documentElement.classList.remove('ws-session');
    new Function(marker)();
    expect(document.documentElement).not.toHaveClass('ws-session');
    document.cookie = 'workshop-hint=1; path=/';
    new Function(marker)();
    expect(document.documentElement).toHaveClass('ws-session');
    document.documentElement.classList.remove('ws-session');
  });

  it('takes the width of what it holds only once `me` has answered, and shows a long login whole on hover', async () => {
    const login = 'a-login-of-thirty-nine-characters-xxxxx';
    let answer: (value: { status: number; body: unknown }) => void = () => {};
    mockApi({ me: owner, routes: { 'GET me': () => new Promise((resolve) => { answer = resolve; }) } });
    const slot = document.createElement('div');
    document.body.append(slot);
    render(<App data={buildData()} accountSlot={slot} />);
    await act(async () => {});
    expect(slot).not.toHaveAttribute('data-account');
    await act(async () => answer(ok({ ...owner, login })));
    await waitFor(() => expect(slot).toHaveAttribute('data-account', 'in'));
    expect(screen.getByText(login)).toHaveAttribute('title', login);
    expect(screen.getByText(login)).toHaveClass('ws-login');
    slot.remove();
  });

  it('switches to the signed-out width when the marker was stale', async () => {
    mockApi({ me: owner, routes: { 'GET me': () => refused(401, 'Not signed in.') } });
    const slot = document.createElement('div');
    document.body.append(slot);
    render(<App data={buildData()} accountSlot={slot} />);
    await waitFor(() => expect(slot).toHaveAttribute('data-account', 'out'));
    slot.remove();
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
