// A decision that a later one replaced: its row, its open view with both lines and their links, the counts, Latest.
import { act, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { buildData, DECISIONS, SHA } from './fixtures';
import { itemOf, mockApi, ok, owner, renderApp, rowButton } from './test-support';

const OLD = 'Which logs does Silo keep?';
const CURRENT = 'How are logs kept honest?';
const LATER = 'Can a log be changed after the fact?';

async function openFrom(hash: string, name: string, data = buildData()) {
  mockApi({ me: owner });
  history.replaceState(null, '', `/workshop${hash}`);
  await renderApp(data);
  await waitFor(() => expect(rowButton(name)).toHaveAttribute('aria-expanded', 'true'));
  return itemOf(name);
}

describe('a replaced decision', () => {
  it('says "replaced" by its pip, where a decided one says "decided", and is struck through as dropped is', async () => {
    mockApi({ me: owner });
    history.replaceState(null, '', '/workshop#product');
    await renderApp();
    const old = itemOf(OLD);
    expect(within(old).getByRole('img', { name: 'replaced' })).toHaveClass('ws-pip', 'replaced');
    expect(old).toHaveClass('gone');
    expect(rowButton(OLD)).toHaveAccessibleName(`${OLD}, replaced`);
    expect(within(itemOf('How is security built?')).getByRole('img', { name: 'decided' })).toBeInTheDocument();
    expect(itemOf('How is security built?')).not.toHaveClass('gone');
    expect(screen.getByText(/^Decisions:/)).toHaveTextContent('Decisions: 1 decided · 1 not decided · 1 replaced');
  });

  it('keeps its place in its part, and is not among the dropped entries', async () => {
    mockApi({ me: owner });
    // The old address of the dropped entries opens All with that filter.
    history.replaceState(null, '', '/workshop#left');
    await renderApp();
    expect(location.search).toBe('?view=all&state=dropped');
    expect(screen.queryByRole('button', { name: new RegExp(`^${OLD}`) })).not.toBeInTheDocument();
    history.replaceState(null, '', '/workshop#product');
    window.dispatchEvent(new HashChangeEvent('hashchange'));
    await waitFor(() => expect(rowButton(OLD)).toBeInTheDocument());
    expect([...document.querySelectorAll('.ws-detail .wa-name > span:first-child')].map((name) => name.textContent)).toEqual(['How is security built?', 'Which host runs Silo first?', OLD]);
  });

  it('shows what it showed when taken and the line "Replaced by", linked to the decision where it lives', async () => {
    const item = await openFrom('#product/plain-logs', OLD);
    expect(within(item).getByText('Plain text logs.')).toBeInTheDocument();
    expect(within(item).getByText('Replaced.')).toBeInTheDocument();
    const line = item.querySelector('.ws-replace') as HTMLElement;
    expect(line).toHaveTextContent(`Replaced by ${CURRENT}`);
    expect(item.querySelectorAll('.ws-replace')).toHaveLength(1);
    const link = within(line).getByRole('link', { name: CURRENT });
    // The decision lives in another part.
    expect(link).toHaveAttribute('href', '#part-e/signed-logs');
    await userEvent.click(link);
    await waitFor(() => expect(location.hash).toBe('#part-e/signed-logs'));
    expect(rowButton(CURRENT)).toHaveAttribute('aria-expanded', 'true');
  });

  it('leaves the browser a click with a modifier key', async () => {
    const item = await openFrom('#product/plain-logs', OLD);
    const link = within(item).getByRole('link', { name: CURRENT });
    const event = new MouseEvent('click', { bubbles: true, cancelable: true, ctrlKey: true });
    link.dispatchEvent(event);
    expect(event.defaultPrevented).toBe(false);
    expect(location.hash).toBe('#product/plain-logs');
  });
});

describe('a decision that replaces another', () => {
  it('shows "Replaces" with the question of the decision, linked, once it is taken', async () => {
    const item = await openFrom('#part-e/signed-logs', CURRENT);
    const line = item.querySelector('.ws-replace') as HTMLElement;
    expect(line).toHaveTextContent(`Replaces ${OLD}`);
    expect(within(line).getByRole('link', { name: OLD })).toHaveAttribute('href', '#product/plain-logs');
    expect(item.querySelectorAll('.ws-replace')).toHaveLength(1);
    expect(within(item).getByText('Decided.')).toBeInTheDocument();
    await userEvent.click(within(line).getByRole('link', { name: OLD }));
    await waitFor(() => expect(location.hash).toBe('#product/plain-logs'));
    expect(rowButton(OLD)).toHaveAttribute('aria-expanded', 'true');
  });

  it('shows "Would replace" while it has no verdict', async () => {
    const item = await openFrom('#part-d/tamper-proof-logs', LATER);
    const line = item.querySelector('.ws-replace') as HTMLElement;
    expect(line).toHaveTextContent(`Would replace ${CURRENT}`);
    expect(within(line).getByRole('link', { name: CURRENT })).toHaveAttribute('href', '#part-e/signed-logs');
    expect(line).not.toHaveTextContent(/^Replaces/);
    expect(within(item).getByText('Not decided.')).toBeInTheDocument();
  });

  it('shows no such line when it is dropped', async () => {
    const decisions = DECISIONS.map((item) => (item.id === 'tamper-proof-logs' ? { ...item, verdict: 'drop' as const } : item));
    const item = await openFrom('#part-d/tamper-proof-logs', LATER, buildData({ decisions }));
    expect(item.querySelector('.ws-replace')).toBeNull();
    expect(within(item).getByText('Dropped.')).toBeInTheDocument();
  });

  it('shows no line for a decision that neither replaces nor is replaced', async () => {
    const item = await openFrom('#part-a/core-in-zig', 'Which language is the core written in?');
    expect(item.querySelector('.ws-replace')).toBeNull();
  });
});

describe('the latest changes', () => {
  it('say in words that what a decision would do changed, never the key', async () => {
    const at = new Date().toISOString();
    const data = buildData(undefined, { shallow: false, changes: new Map([['signed-logs', [{ commit: SHA.b, date: at, what: ['sets', 'replaced'], by: 'tamper-proof-logs' }]]]) });
    mockApi();
    history.replaceState(null, '', '/workshop#latest');
    await renderApp(data);
    await waitFor(() => expect(rowButton(CURRENT)).toBeInTheDocument());
    const row = itemOf(CURRENT).querySelector('.wa-change') as HTMLElement;
    expect(row).toHaveTextContent('What a yes would do changed · Replaced by');
    expect(row.textContent).not.toMatch(/\b(sets|replaces|replaced)\b/);
    await act(async () => {});
  });
});
