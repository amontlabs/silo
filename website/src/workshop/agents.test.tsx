// An entry the agents decided: "decided by agents" and "dropped by agents" wherever the page says "decided" and
// "dropped", and the owner's own verdicts keep the plain words.
import { cleanup, screen, waitFor, within } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { buildData, DECISIONS, FEATURES } from './fixtures';
import type { HistorySource } from './data';
import type { Change } from './plan';
import { itemOf, mockApi, owner, renderApp, rowButton } from './test-support';

const ZIG = 'Which language is the core written in?';
const OLD_FORMAT = 'Which format does an export use?';

/** The fixture plan with the verdicts of the decisions `core-in-zig` and `old-format` written by the agents. Agents never
 * decide a feature, so every feature keeps the owner's verdict. */
const byAgents = (history: HistorySource | null = null) => buildData({
  features: FEATURES,
  decisions: DECISIONS.map((decision) => (decision.id === 'core-in-zig' || decision.id === 'old-format' ? { ...decision, by: 'agents' as const } : decision)),
}, history);

async function openFrom(url: string, name: string, data = byAgents()) {
  cleanup();
  mockApi({ me: owner });
  window.history.replaceState(null, '', url);
  await renderApp(data);
  await waitFor(() => expect(rowButton(name)).toHaveAttribute('aria-expanded', 'true'));
  return itemOf(name);
}

describe('an entry the agents decided', () => {
  it('says "decided by agents" by its pip, on its row and in its open view, on a decision', async () => {
    for (const [name, url] of [[ZIG, '/workshop#part-a/core-in-zig']]) {
      const item = await openFrom(url, name);
      expect(within(item).getByRole('img', { name: 'decided by agents' })).toHaveClass('ws-pip', 'decided');
      expect(rowButton(name).getAttribute('aria-label') ?? rowButton(name).textContent).toMatch(/decided by agents$/);
      expect(within(item).getByText('Decided by agents.')).toBeInTheDocument();
    }
  });

  it('says "dropped by agents" for a dropped decision, in All', async () => {
    cleanup();
    mockApi({ me: owner });
    window.history.replaceState(null, '', '/workshop?view=all&state=dropped');
    await renderApp(byAgents());
    for (const name of [OLD_FORMAT]) {
      await waitFor(() => expect(rowButton(name)).toBeInTheDocument());
      expect(within(itemOf(name)).getByRole('img', { name: 'dropped by agents' })).toHaveClass('ws-pip', 'dropped');
      expect(rowButton(name).getAttribute('aria-label') ?? rowButton(name).textContent).toMatch(/dropped by agents$/);
    }
  });

  it('keeps the plain words for the owner\'s verdicts, in the same plan', async () => {
    const item = await openFrom('/workshop#part-a/core-in-zig', ZIG);
    expect(within(item).queryByText('Decided.')).not.toBeInTheDocument();
    cleanup();
    const plain = await openFrom('/workshop#part-c/shells', 'Your shell inside', buildData());
    expect(within(plain).getByRole('img', { name: 'being built' })).toBeInTheDocument();
    const owners = await openFrom('/workshop#part-a/calls', 'Every call defined once', buildData());
    expect(within(owners).getByRole('img', { name: 'decided' })).toBeInTheDocument();
    expect(within(owners).getByText('Decided.')).toBeInTheDocument();
    expect(screen.queryByText(/by agents/)).not.toBeInTheDocument();
  });

  it('says "Decided by agents" in the closed History line and in the list of changes', async () => {
    const change = (agents: boolean): Change => ({ commit: '1'.repeat(40), date: '2026-11-20T12:00:00Z', what: ['taken'], ...(agents ? { agents: true as const } : {}) });
    const source = (agents: boolean): HistorySource => ({ changes: new Map([['core-in-zig', [change(agents)]]]), shallow: false });
    const item = await openFrom('/workshop#part-a/core-in-zig', ZIG, byAgents(source(true)));
    expect(within(item).getByRole('button', { name: /^History/ })).toHaveTextContent('History · Decided by agents, 20 Nov');
    cleanup();
    const plain = await openFrom('/workshop#part-a/core-in-zig', ZIG, byAgents(source(false)));
    expect(within(plain).getByRole('button', { name: /^History/ })).toHaveTextContent('History · Decided, 20 Nov');
  });

  it('says "Decided" in the History once the owner confirmed what the agents decided', async () => {
    const agentsChange: Change = { commit: '1'.repeat(40), date: '2026-11-20T12:00:00Z', what: ['taken'], agents: true };
    const confirmed: Change = { commit: '2'.repeat(40), date: '2026-11-21T12:00:00Z', what: ['taken'] };
    const source: HistorySource = { changes: new Map([['core-in-zig', [confirmed, agentsChange]]]), shallow: false };
    const item = await openFrom('/workshop#part-a/core-in-zig', ZIG, byAgents(source));
    const history = within(item).getByRole('button', { name: /^History/ });
    expect(history).toHaveTextContent('History · Decided, 21 Nov');
    expect(history).not.toHaveTextContent(/by agents/);
  });
});
