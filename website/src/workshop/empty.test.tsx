// The Workshop while the plan holds no part, as the real build draws it: no map, no threads.
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
  40: issue({ entry: 'gone-feature', title: 'Gone', state: 'closed', reason: 'not_planned', closedAt: '2026-11-18T00:00:00Z' }),
});

describe('the Workshop with an empty plan', () => {
  it('is drawn with Silo\'s own map, which holds no part', () => {
    expect(empty().map).toBe(SILO_MAP);
    expect(Object.keys(SILO_MAP.places)).toEqual([]);
  });

  it('is the normal page with nothing in it: the title row, the tiles without a map, sign-in, the lists, and the live reading', async () => {
    const calls = mockApi({ state: () => ok(withGoneEntries()) });
    await renderApp(empty());
    expect(screen.getByRole('heading', { level: 1, name: 'Workshop' })).toBeInTheDocument();
    // No map: no Map and Order control, no drawing, only the tiles.
    expect(screen.queryByRole('button', { name: 'Map' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Order' })).toBeNull();
    expect(document.querySelector('.ws-swap-map, .ws-stage')).toBeNull();
    expect(document.querySelector('.ws')).toHaveClass('v-order');
    expect([...document.querySelectorAll<HTMLElement>('.ws-tile')].map((tile) => tile.dataset.part)).toEqual(['suggested']);
    expect(screen.queryByText('After that')).toBeNull();
    expect(screen.getByRole('link', { name: 'Sign in with GitHub' })).toBeInTheDocument();
    expect(calls.some((call) => call.method === 'GET' && call.path === 'state')).toBe(true);
    // The issue of a feature the plan no longer holds shows nowhere; people's suggestions still count.
    expect(screen.queryByText('Gone')).toBeNull();
    expect(screen.getByRole('link', { name: '2 suggested' })).toHaveAttribute('href', '#suggested');
    // Entries have no threads in the real build: nothing to show under Latest, so no link to it.
    expect(screen.queryByRole('link', { name: 'Latest' })).toBeNull();
    expect(screen.queryByRole('link', { name: /Left out/ })).toBeNull();
    expect(screen.getByRole('link', { name: 'Suggest a feature' })).toBeInTheDocument();

    await userEvent.click(screen.getByRole('link', { name: 'The whole product' }));
    expect(location.hash).toBe('#product');
    expect(screen.getByRole('heading', { level: 2, name: 'The whole product' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /← All parts/ })).toBeInTheDocument();
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
