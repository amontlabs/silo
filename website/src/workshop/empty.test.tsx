// The Workshop while the plan holds no part: a few lines in place of the map and the lists, and nothing live.
import { render, screen } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';
import { App } from './App';
import { buildDataFrom } from './data';
import { emptyPlanFiles } from './fixtures';
import { inject, prerender } from './prerender';
import { takeOver } from './takeover';
import { mockApi } from './test-support';

const empty = () => buildDataFrom(emptyPlanFiles());

afterEach(() => {
  document.body.innerHTML = '';
  document.documentElement.classList.remove('ws-bare');
});

describe('the Workshop with an empty plan', () => {
  it('says the plan is being rewritten, links the design and the earlier plan, and reads nothing live', () => {
    const calls = mockApi();
    render(<App data={empty()} accountSlot={null} />);
    expect(screen.getByRole('heading', { level: 1, name: 'Workshop' })).toBeInTheDocument();
    expect(screen.getByText('The plan is being rewritten from a new design, one part at a time.')).toBeInTheDocument();
    expect(screen.getByText('Parts appear here as the owner settles them.')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Read the design' })).toHaveAttribute('href', 'https://github.com/amontlabs/silo/blob/main/plan/DESIGN.md');
    expect(screen.getByRole('link', { name: 'plan-2026-11-18' })).toHaveAttribute('href', 'https://github.com/amontlabs/silo/tree/plan-2026-11-18/plan');
    expect(screen.getByRole('link', { name: 'plan-2026-11-18' }).parentElement).toHaveTextContent('The earlier plan is kept at the tag plan-2026-11-18.');
    // Nothing to vote on, suggest, sign in to or switch between.
    expect(screen.getAllByRole('link').map((link) => link.textContent)).toEqual(['Read the design', 'plan-2026-11-18']);
    expect(screen.queryAllByRole('button')).toEqual([]);
    expect(document.querySelector('.ws-part, .ws-tile, .ws-suggest, .ws-account-inline')).toBeNull();
    expect(calls).toEqual([]);
  });

  it('is built with an empty, unmarked account slot, and the script takes over without marking the page bare', () => {
    const data = empty();
    const parts = prerender(data);
    expect(parts.account).toBe('');
    const html = inject('<div class="ws-account-slot" id="workshop-account"></div><main><div id="workshop-root"></div></main>', parts);
    expect(html).toContain('<div class="ws-account-slot" id="workshop-account"></div>');
    document.body.innerHTML = html;
    const root = document.getElementById('workshop-root') as HTMLElement;
    const slot = document.getElementById('workshop-account') as HTMLElement;
    expect(root.textContent).toContain('Parts appear here as the owner settles them.');
    const before = root.innerHTML;
    takeOver(root, slot, <App data={data} accountSlot={slot} />);
    expect(root.innerHTML).toBe(before);
    expect(slot.childNodes).toHaveLength(0);
    expect(slot.hasAttribute('data-prerendered')).toBe(false);
    expect(document.documentElement.classList.contains('ws-bare')).toBe(false);
  });
});
