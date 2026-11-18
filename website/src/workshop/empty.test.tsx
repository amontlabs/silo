// The Workshop while the plan holds no part: its normal page, with nothing in the map, the tiles or the lists.
import { act, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { buildDataFrom } from './data';
import { emptyPlanFiles, issue, liveState } from './fixtures';
import { SILO_MAP } from './map';
import { mockApi, ok, posts, renderApp, visitor } from './test-support';

const empty = () => buildDataFrom(emptyPlanFiles());

// Issues of features the plan no longer holds, as sync leaves them: closed, with their label.
const withGoneEntries = () => liveState({
  40: issue({ entry: 'gone-feature', title: 'Gone', verdict: 'take', state: 'closed', reason: 'not_planned', closedAt: '2026-11-18T00:00:00Z' }),
});

describe('the Workshop with an empty plan', () => {
  it('is drawn with Silo\'s own map, which holds no part', () => {
    expect(empty().map).toBe(SILO_MAP);
    expect(Object.keys(SILO_MAP.places)).toEqual([]);
  });

  it('is the normal page with nothing in it: the title row, Map and Order, sign-in, the lists, and the live reading', async () => {
    const calls = mockApi({ state: () => ok(withGoneEntries()) });
    await renderApp(empty());
    expect(screen.getByRole('heading', { level: 1, name: 'Workshop' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Map' })).toHaveAttribute('aria-pressed', 'true');
    expect(screen.getByRole('link', { name: 'Sign in with GitHub' })).toBeInTheDocument();
    expect(calls.some((call) => call.method === 'GET' && call.path === 'state')).toBe(true);
    // The map says it has no part, in one line, instead of an empty drawing.
    const map = screen.getByRole('region', { name: 'Silo, part by part' });
    expect(within(map).getByText('No part yet.')).toBeInTheDocument();
    expect(document.querySelectorAll('.ws-part, .ws-wires path, .ws-zone')).toHaveLength(0);
    // The issue of a feature the plan no longer holds shows nowhere; people's suggestions still count.
    expect(screen.queryByText('Gone')).toBeNull();
    expect(screen.getByRole('link', { name: '2 suggested' })).toHaveAttribute('href', '#suggested');
    expect(screen.getByRole('link', { name: 'Latest' })).toHaveAttribute('href', '#latest');
    expect(screen.queryByRole('link', { name: /Left out/ })).toBeNull();
    expect(screen.getByRole('link', { name: 'Suggest a feature' })).toBeInTheDocument();

    await userEvent.click(screen.getByRole('button', { name: 'Order' }));
    expect(location.search).toBe('?view=order');
    expect([...document.querySelectorAll<HTMLElement>('.ws-tile')].map((tile) => tile.dataset.area)).toEqual(['suggested']);
    expect(screen.queryByText('After that')).toBeNull();
    await userEvent.click(screen.getByRole('button', { name: 'Map' }));

    await userEvent.click(screen.getByRole('link', { name: 'Latest' }));
    expect(location.hash).toBe('#latest');
    expect(screen.queryByText('Gone')).toBeNull();
  });

  it('lets a signed-in person suggest a feature, which appears under Suggested by people', async () => {
    const calls = mockApi({ me: visitor, routes: { 'POST suggest': () => ok({ number: 77 }) } });
    await renderApp(empty());
    await userEvent.click(screen.getByRole('link', { name: 'Suggest a feature' }));
    await userEvent.type(screen.getByRole('textbox', { name: 'Title' }), 'Pause a computer');
    await userEvent.type(screen.getByRole('textbox', { name: 'Why' }), 'Stop is not pause.');
    await act(async () => userEvent.click(screen.getByRole('button', { name: 'Suggest' })));
    expect(posts(calls, 'suggest')[0].body).toEqual({ title: 'Pause a computer', text: 'Stop is not pause.' });
    const group = screen.getByRole('region', { name: 'Suggested by people' });
    expect(within(group).getByRole('button', { name: /^Pause a computer/ }).closest('.ws-item')).toHaveAttribute('id', 'issue-77');
  });
});
