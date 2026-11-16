// Random sequences of an owner's actions and the server's answers, checked after every step against
// a reference model: a verdict is only ever sent for the row that was open and on screen, never inside the guard,
// never twice at once for one issue, and the page never shows as done what the server has not answered:
// a row waiting for an answer says "Saving…", everything else and the "to decide" counts show the server's answers.
import { act, cleanup, fireEvent, render } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { App, OPEN_GUARD_MS } from './App';
import { buildData, liveState } from './fixtures';
import type { Verdict } from './plan';

type Display = 'undecided' | 'planned' | 'later' | 'dropped' | 'saving' | 'confirm';
type Sent = { n: number; verdict: Verdict; resolve: (status: number) => void };

const ISSUE_OF: Record<string, number> = {
  'entry-engine': 10, 'entry-calls': 11, 'entry-computers': 12, 'entry-exec': 13, 'entry-fork': 14,
  'entry-shells': 15, 'entry-devices': 16, 'entry-reviewer': 17, 'issue-30': 30, 'issue-31': 31,
};
const FILE: Record<number, Verdict | null> = { 10: 'take', 11: 'take', 15: 'take', 17: 'drop' };
const LIVE: Record<number, Verdict | null> = { 10: 'take', 15: 'take' };
/** Entries in an area that wait for a verdict until one is confirmed. */
const IN_AREAS = [12, 13, 14, 16];
const PLACES = ['#computers', '#forks', '#devices', '#engine', '#setup', '#left', '#suggested', '#decisions', ''];

function random(seed: number) {
  let a = seed;
  return () => {
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function display(verdict: Verdict | null): Display {
  return verdict === 'take' ? 'planned' : verdict === 'later' ? 'later' : verdict === 'drop' ? 'dropped' : 'undecided';
}

function shown(key: string): Display | null {
  const item = document.getElementById(key);
  if (!item) return null;
  const word = item.querySelector('.ws-word')?.textContent;
  if (word === 'Saving…') return 'saving';
  if (word === 'confirm') return 'confirm';
  return word === 'planned' ? 'planned' : word === 'later' ? 'later' : word === 'dropped' ? 'dropped' : 'undecided';
}

async function run(seed: number) {
  const rand = random(seed);
  const pick = <T,>(items: T[]): T => items[Math.floor(rand() * items.length)];
  let now = 1_000_000;
  vi.spyOn(Date, 'now').mockImplementation(() => now);
  document.cookie = 'workshop-hint=1; path=/';
  localStorage.clear();
  history.replaceState(null, '', '/workshop');
  const sent: Sent[] = [];
  const inFlight: Sent[] = [];
  vi.stubGlobal('fetch', vi.fn(async (input: string, init?: RequestInit) => {
    const path = new URL(input, 'http://localhost').pathname.replace('/api/workshop/', '');
    const reply = (status: number, body: unknown = {}) => new Response(JSON.stringify(body), { status });
    if (path === 'state') return reply(200, liveState());
    if (path === 'me') return reply(200, { login: 'owner', avatar: '', canDecide: true });
    if (path === 'issue') return reply(200, { number: 1, title: '', body: '', url: '', up: 0, comments: [] });
    if (path === 'vote') return reply(200, { on: false });
    if (path === 'verdict' && init?.method === 'POST') {
      const body = JSON.parse(String(init.body)) as { n: number; verdict: Verdict };
      return new Promise<Response>((resolve) => {
        const entry: Sent = { n: body.n, verdict: body.verdict, resolve: (status) => resolve(reply(status, { error: 'x', message: 'Refused.' })) };
        sent.push(entry);
        inFlight.push(entry);
      });
    }
    return reply(404, {});
  }));

  // The reference model: what the server holds, and what is pending.
  const server: Record<number, Verdict | null> = { ...LIVE };
  const pending = new Map<number, Verdict>();
  const confirmed = (n: number): Display => display(server[n] ?? FILE[n] ?? null);
  // A file verdict the server has not recorded yet is offered to the owner to confirm.
  const expected = (n: number): Display => (pending.has(n) ? 'saving' : (server[n] ?? null) === null && FILE[n] != null && n !== 10 && n !== 15 ? 'confirm' : confirmed(n));
  // Entries in areas the owner still has to decide, and the carried-over `calls` (11) to confirm.
  const toDecide = () => IN_AREAS.filter((n) => (server[n] ?? null) === null).length + ((server[11] ?? null) === null ? 1 : 0);
  let open: string | null = null;
  let openedAt = now;
  // Several rows may be open; the one opened last is in the address, and only it takes a verdict.
  const current$ = () => {
    const tail = decodeURIComponent(location.hash.slice(1)).split('/')[1];
    return tail ? (/^(issue|decided)-/.test(tail) ? tail : `entry-${tail}`) : null;
  };
  const observe = () => {
    const current = current$();
    if (current !== open) {
      open = current;
      openedAt = now;
    }
  };

  render(<App data={buildData()} />);
  await act(async () => {});
  observe();

  for (let step = 0; step < 40; step += 1) {
    const before = sent.length;
    const openBefore = open;
    const key = open;
    const n = key ? ISSUE_OF[key] : undefined;
    const elapsed = now - openedAt;
    const visible = key !== null && document.getElementById(key)?.querySelector('.ws-row')?.getAttribute('aria-expanded') === 'true';
    const awaits = n !== undefined && !pending.has(n) && confirmed(n) === 'undecided' && (server[n] ?? null) === null;
    const carried = n !== undefined && !pending.has(n) && (server[n] ?? null) === null && FILE[n] != null && n !== 10 && n !== 15;
    let predicted: { n: number; verdict: Verdict } | null = null;
    const choice = rand();

    if (choice < 0.18) {
      now += pick([100, 300, 650, 700, 1200]);
    } else if (choice < 0.38) {
      const active = document.activeElement as HTMLElement;
      const onRow = active === document.body || (active.classList.contains('ws-row') && active.closest('.ws-item')?.id === key);
      const repeat = rand() < 0.2;
      if (onRow && visible && !repeat && elapsed >= OPEN_GUARD_MS && n !== undefined) {
        if (awaits) predicted = { n, verdict: 'take' };
        else if (carried) predicted = { n, verdict: FILE[n] as Verdict };
      }
      act(() => { fireEvent.keyDown(active, { key: 'Enter', repeat }); });
    } else if (choice < 0.56) {
      const buttons = [...document.querySelectorAll<HTMLButtonElement>('.ws-item.open .ws-btn')]
        .filter((button) => /^(Take|Later|Drop|Confirm)/.test(button.textContent ?? ''));
      if (buttons.length) {
        const button = pick(buttons);
        const label = (button.textContent ?? '').replace(/\s*↵$/, '');
        const onCurrent = button.closest('.ws-item')?.id === key;
        if (onCurrent && n !== undefined && !button.disabled && !pending.has(n) && elapsed >= OPEN_GUARD_MS) {
          predicted = { n, verdict: label === 'Confirm' ? (FILE[n] as Verdict) : (label.toLowerCase() as Verdict) };
        }
        act(() => { fireEvent.click(button); });
      }
    } else if (choice < 0.7) {
      const rows = [...document.querySelectorAll<HTMLButtonElement>('.ws-list button.ws-row')];
      if (rows.length) {
        const row = pick(rows);
        act(() => { row.focus(); fireEvent.click(row); });
      }
    } else if (choice < 0.8) {
      // Go somewhere else: another area, the strip's lists, the decisions, the map, or the "to decide" link.
      const where = rand();
      // Back and Forward arrive as popstate; sent at once so the page and the model see the same address.
      if (where < 0.5) act(() => { history.pushState(null, '', `/workshop${pick(PLACES)}`); dispatchEvent(new PopStateEvent('popstate')); });
      else if (where < 0.7) act(() => { fireEvent.keyDown(document.body, { key: 'Escape' }); });
      else {
        const link = [...document.querySelectorAll<HTMLAnchorElement>('.ws-loglink')].find((a) => /^\d+ to decide$/.test(a.textContent ?? ''));
        // Opening a row from the link starts its guard again, even when it was already open.
        if (link) {
          act(() => { fireEvent.click(link); });
          open = null;
        }
      }
    } else if (inFlight.length) {
      const entry = inFlight.splice(Math.floor(rand() * inFlight.length), 1)[0];
      const accept = rand() < 0.6;
      pending.delete(entry.n);
      if (accept) server[entry.n] = entry.verdict;
      await act(async () => { entry.resolve(accept ? 200 : 403); });
    }
    await act(async () => {});

    const fresh = sent.slice(before);
    if (predicted) {
      expect(fresh.map(({ n: issue, verdict }) => ({ n: issue, verdict })), `step ${step} of seed ${seed}`).toEqual([predicted]);
      pending.set(predicted.n, predicted.verdict);
      expect(openBefore).toBe(key);
    } else {
      expect(fresh, `step ${step} of seed ${seed}: nothing should be sent`).toEqual([]);
    }
    const flying = inFlight.map((entry) => entry.n);
    expect(new Set(flying).size, `step ${step}: two verdicts in flight for one issue`).toBe(flying.length);
    observe();
    const counts = [...document.querySelectorAll('.ws-part .ws-todo')].reduce((sum, badge) => sum + Number(badge.textContent), 0);
    expect(counts, `step ${step} of seed ${seed}: to decide on the map`).toBe(toDecide());
    const link = [...document.querySelectorAll('.ws-loglink')].find((a) => /^\d+ to decide$/.test(a.textContent ?? ''));
    if (link) expect(link.textContent, `step ${step} of seed ${seed}: to decide link`).toBe(`${toDecide()} to decide`);
    for (const [rowKey, issue] of Object.entries(ISSUE_OF)) {
      const seen = shown(rowKey);
      // Rows of another place are not on the page.
      if (seen !== null) expect(seen, `step ${step} of seed ${seed}: ${rowKey}`).toBe(expected(issue));
    }
  }
}

describe('verdicts under random actions', () => {
  it('go to the open row only, respect the guard, and match the server or the pending write', async () => {
    for (let seed = 1; seed <= 30; seed += 1) {
      await run(seed);
      cleanup();
      history.replaceState(null, '', '/workshop');
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
    }
  }, 120_000);
});
