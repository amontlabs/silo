// A signed-out visitor can comment on the entry's GitHub issue instead of signing in here.
import { act, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import { App } from './App';
import { buildData } from './fixtures';
import { itemOf, mockApi, owner, refused, renderApp, rowButton, visitor } from './test-support';
import type { Route } from './test-support';

describe('or comment on GitHub', () => {
  it('shows under "Sign in to comment" as the one link to the issue, from the panel\'s first paint', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#computers');
    await renderApp();
    await userEvent.click(rowButton('Create and delete a computer'));
    const item = itemOf('Create and delete a computer');
    const link = within(item).getByRole('link', { name: 'or comment on GitHub' });
    expect(link).toHaveAttribute('href', 'https://github.com/amontlabs/silo/issues/12');
    expect(link).toHaveAttribute('target', '_blank');
    expect(link).toHaveAttribute('rel', 'noopener noreferrer');
    expect(link).toHaveAttribute('title', 'GitHub #12');
    expect(within(item).getByRole('link', { name: 'Sign in to comment' }).closest('.ws-box')?.nextElementSibling).toBe(link);
    expect(within(item).queryByRole('link', { name: /^GitHub #/ })).not.toBeInTheDocument();
  });

  it('is there for an open suggestion and decision too, and not on the suggest form', async () => {
    mockApi();
    history.replaceState(null, '', '/workshop#suggested/issue-31');
    await renderApp();
    expect(await screen.findByRole('link', { name: 'or comment on GitHub' })).toHaveAttribute('href', 'https://github.com/amontlabs/silo/issues/31');
    expect(screen.getByRole('link', { name: 'Sign in to suggest' })).toBeInTheDocument();
    expect(screen.getAllByRole('link', { name: 'or comment on GitHub' })).toHaveLength(1);
  });

  it('is not offered to a signed-in person, who keeps "GitHub #N"', async () => {
    mockApi({ me: visitor });
    history.replaceState(null, '', '/workshop#computers/exec');
    await renderApp();
    await screen.findByRole('textbox', { name: 'Comment' });
    expect(screen.getByRole('link', { name: 'GitHub #13 ↗' })).toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'or comment on GitHub' })).not.toBeInTheDocument();
  });

  it('is not shown when the live state is unavailable', async () => {
    mockApi({ state: () => refused(503, 'not configured') });
    history.replaceState(null, '', '/workshop#computers/exec');
    await renderApp();
    await waitFor(() => expect(rowButton('Run a command')).toHaveAttribute('aria-expanded', 'true'));
    expect(screen.queryByRole('link', { name: 'or comment on GitHub' })).not.toBeInTheDocument();
    expect(screen.queryByText('Sign in to comment')).not.toBeInTheDocument();
  });

  it('keeps the signed-in shape while `me` is asked, and changes once when the session was stale', async () => {
    let answer: (value: { status: number; body: unknown }) => void = () => {};
    const me: Route = () => new Promise((resolve) => { answer = resolve; });
    mockApi({ me: owner, routes: { 'GET me': me } });
    history.replaceState(null, '', '/workshop#computers/exec');
    render(<App data={buildData()} />);
    await waitFor(() => expect(rowButton('Run a command')).toHaveAttribute('aria-expanded', 'true'));
    expect(screen.getByRole('textbox', { name: 'Comment' })).toBeDisabled();
    expect(screen.queryByRole('link', { name: 'or comment on GitHub' })).not.toBeInTheDocument();
    await act(async () => answer(refused(401, 'Not signed in.')));
    expect(await screen.findByRole('link', { name: 'or comment on GitHub' })).toBeInTheDocument();
  });
});
