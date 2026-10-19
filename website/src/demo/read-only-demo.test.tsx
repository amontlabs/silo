import { describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { ReadOnlyDemo } from './read-only-demo';
import { demoActions, readOnlyOperation } from './data';

describe('the embedded production UI', () => {
  it('navigates the real sidebar and back history without enabling computer actions', async () => {
    const user = userEvent.setup();
    render(<ReadOnlyDemo />);
    const sidebar = screen.getByRole('navigation', { name: 'Silo navigation' });
    expect(within(sidebar).getByRole('button', { name: 'Collapse Settings menu' })).toHaveAttribute('aria-expanded', 'true');
    expect(within(sidebar).getByRole('button', { name: 'Computers' })).toBeVisible();
    expect(screen.getByRole('button', { name: /Add/ })).toBeDisabled();
    await user.click(within(sidebar).getByRole('button', { name: /^Secrets/ }));
    expect(screen.getByText('PACKAGE_TOKEN')).toBeVisible();
    expect(screen.getByRole('button', { name: /Add secret/ })).toBeDisabled();
    await user.click(screen.getByRole('button', { name: 'Go back' }));
    expect(screen.getByRole('button', { name: /Add/ })).toBeDisabled();
    await user.click(screen.getByRole('button', { name: 'Go forward' }));
    expect(screen.getByText('PACKAGE_TOKEN')).toBeVisible();
  });

  it('shows fixture files, logs, network and settings without network requests', async () => {
    const network = vi.spyOn(globalThis, 'fetch').mockRejectedValue(new Error('The demo must not fetch live data'));
    const user = userEvent.setup();
    render(<ReadOnlyDemo />);
    const sidebar = screen.getByRole('navigation', { name: 'Silo navigation' });
    for (const name of ['Files', 'Logs', 'Network', 'Activity', 'GitHub', 'Secrets', 'Settings', 'Connections', 'Notifications']) {
      await user.click(within(sidebar).getByRole('button', { name: new RegExp(`^${name}`) }));
      const page = screen.getByRole('group', { name: 'Read-only sample data' });
      expect(page).toBeVisible();
      for (const control of page.querySelectorAll('button, input, select, textarea')) expect(control).toBeDisabled();
    }
    expect(network).not.toHaveBeenCalled();
    network.mockRestore();
  });
});


it.each([
  ['dev', 'This device', 'ssh -p 2222 silo@127.0.0.1'],
  ['personal', 'Office Mac', 'ssh -p 2224 silo@192.168.1.42'],
])('opens SSH details for %s while keeping native actions disabled', async (name, computer, endpoint) => {
  const actions = Object.entries(demoActions)
    .filter(([, action]) => action === readOnlyOperation)
    .map(([action]) => vi.spyOn(demoActions as unknown as Record<string, (...args: unknown[]) => unknown>, action));
  const network = vi.spyOn(globalThis, 'fetch').mockRejectedValue(new Error('The demo must not fetch live data'));
  const user = userEvent.setup();
  render(<ReadOnlyDemo />);
  expect(screen.queryByText('build-server')).not.toBeInTheDocument();
  expect(screen.getByRole('img', { name: 'Remote computer' })).toBeVisible();
  await user.click(screen.getByRole('button', { name: `More actions for ${name}` }));
  for (const item of screen.getAllByRole('menuitem').filter(item => !/^(Checkpoints|Storage|SSH) for /.test(item.getAttribute('aria-label') ?? ''))) {
    expect(item).toHaveAttribute('aria-disabled', 'true');
  }
  await user.click(screen.getByRole('menuitem', { name: `SSH for ${name}` }));
  expect(screen.getByRole('tab', { name: 'SSH' })).toHaveAttribute('aria-selected', 'true');
  expect(screen.getByText(endpoint)).toBeVisible();
  const switches = screen.getAllByRole('switch');
  expect(switches).toHaveLength(2);
  expect(screen.getByRole('switch', { name: `Allow SSH from ${computer}` })).toBeChecked();
  for (const control of switches) {
    expect(control).toBeDisabled();
  }
  expect(screen.getByRole('button', { name: name === 'dev' ? 'Copy SSH address' : 'Copy network SSH address' })).toBeDisabled();
  for (const control of screen.getAllByRole('button', { name: /^(Copy.*SSH address|More.*SSH actions|Open .* in |Stop |Restart )/ })) {
    expect(control).toBeDisabled();
  }
  await user.click(screen.getByRole('tab', { name: 'Overview' }));
  await user.click(screen.getByRole('tab', { name: 'SSH' }));
  expect(screen.getByText(endpoint)).toBeVisible();
  for (const action of actions) expect(action).not.toHaveBeenCalled();
  expect(network).not.toHaveBeenCalled();
});

it('offers the production command menu for safe navigation', async () => {
  const user = userEvent.setup();
  render(<ReadOnlyDemo />);
  await user.click(screen.getByRole('button', { name: 'Search or jump to' }));
  const search = screen.getByRole('combobox', { name: 'Search commands' });
  await user.type(search, 'Secrets');
  await user.keyboard('{Enter}');
  expect(screen.getByText('PACKAGE_TOKEN')).toBeVisible();
  expect(screen.queryByRole('dialog', { name: 'Commands' })).not.toBeInTheDocument();
});

it('shows computer menus and sample storage without allowing native operations', async () => {
  const user = userEvent.setup();
  render(<ReadOnlyDemo />);
  expect(screen.getByRole('button', { name: 'Open dev desktop' })).toBeDisabled();
  await user.click(screen.getByRole('button', { name: 'More actions for dev' }));
  expect(screen.getByRole('menuitem', { name: 'Restart dev' })).toHaveAttribute('aria-disabled', 'true');
  await user.click(screen.getByRole('menuitem', { name: 'Storage for dev' }));
  expect(await screen.findByText('18 GiB')).toBeVisible();
  expect(screen.getByRole('button', { name: 'Free up space' })).toBeDisabled();
  await user.click(screen.getByRole('button', { name: 'History, 1 attempt' }));
  expect(screen.getByLabelText('History entries')).toBeVisible();
});
