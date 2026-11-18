// Random sequences of an owner's actions and the server's answers, checked after every step against
// a reference model: a verdict is only ever sent for the row that was open and on screen, never inside the guard,
// never twice at once for one issue, and the page never shows as done what the server has not answered:
// a row waiting for an answer says "Saving…", everything else and the "to decide" counts show the server's answers
// over the plan file's. Enter sends a verdict only for an open row, past its own guard, with its verdict buttons in
// sight: the row whose header holds the focus, else the one opened last. On a closed row's header it opens that row.
import { act, cleanup, fireEvent, render } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import { App, OPEN_GUARD_MS } from './App';
import { buildData, liveState } from './fixtures';
import type { Verdict } from './plan';

type Display = 'undecided' | 'planned' | 'later' | 'dropped' | 'saving';
type Sent = { n: number; verdict: Verdict; resolve: (status: number) => void };

const ISSUE_OF: Record<string, number> = {
  'entry-engine': 10, 'entry-calls': 11, 'entry-computers': 12, 'entry-exec': 13, 'entry-fork': 14,
  'entry-shells': 15, 'entry-devices': 16, 'entry-reviewer': 17, 'issue-30': 30, 'issue-31': 31,
};
const FILE: Record<number, Verdict | null> = { 10: 'take', 11: 'take', 15: 'take', 17: 'drop' };
const LIVE: Record<number, Verdict | null> = { 10: 'take', 15: 'take' };
/** Entries in an area that wait for a verdict: no file verdict, no live one. */
const IN_AREAS = [12, 13, 14, 16];
// Computers holds the two entries that can be open together and still wait for a verdict, so it comes up more often.
const PLACES = ['#computers', '#computers', '#computers', '#forks', '#devices', '#engine', '#setup', '#left', '#suggested', '#decisions', '#latest', ''];

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
  return word === 'planned' ? 'planned' : word === 'later' ? 'later' : word === 'dropped' ? 'dropped' : 'undecided';
}

/** How often the sequences reached each case of Enter, so a rule is not checked on nothing. */
const reached = { closedHeader: 0, closedBesideReady: 0, otherHeader: 0, page: 0 };

async function run(seed: number): Promise<number> {
  const rand = random(seed);
  const pick = <T,>(items: T[]): T => items[Math.floor(rand() * items.length)];
  let now = 1_000_000;
  vi.spyOn(Date, 'now').mockImplementation(() => now);
  document.cookie = 'workshop-hint=1; path=/';
  localStorage.clear();
  history.replaceState(null, '', '/workshop#computers');
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
  // A verdict in the file counts like one on GitHub, the live one first.
  const expected = (n: number): Display => (pending.has(n) ? 'saving' : confirmed(n));
  // Entries in areas the owner still has to decide.
  const toDecide = () => IN_AREAS.filter((n) => (server[n] ?? null) === null).length;
  // Several rows may be open, each with its own guard from the moment it opened. The one opened last is in the
  // address; Enter on the page goes to it, Enter on a row to that row, and a click to the row of the button.
  const openAt = new Map<string, number>();
  let mostOpen = 0;
  const current$ = () => {
    const tail = decodeURIComponent(location.hash.slice(1)).split('/')[1];
    return tail ? (/^(issue|decided)-/.test(tail) ? tail : `entry-${tail}`) : null;
  };
  const observe = () => {
    const shown = new Set([...document.querySelectorAll('.ws-item.open')].map((item) => item.id));
    for (const key of [...openAt.keys()]) if (!shown.has(key)) openAt.delete(key);
    for (const key of shown) if (!openAt.has(key)) openAt.set(key, now);
    mostOpen = Math.max(mostOpen, shown.size);
  };
  /** What a verdict on this row would send, if anything. */
  const verdictFor = (key: string | null, label = 'Take'): { n: number; verdict: Verdict } | null => {
    const n = key ? ISSUE_OF[key] : undefined;
    if (!key || n === undefined || !openAt.has(key) || now - (openAt.get(key) as number) < OPEN_GUARD_MS || pending.has(n)) return null;
    // The server records verdicts on entries only.
    if (!key.startsWith('entry-') || confirmed(n) !== 'undecided') return null;
    if (label === 'Take') return { n, verdict: 'take' };
    return { n, verdict: label.toLowerCase() as Verdict };
  };

  // Rows whose verdict buttons are scrolled out of sight: Enter must never take them.
  const hidden = new Set<string>();
  const rect = Element.prototype.getBoundingClientRect;
  vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function measure(this: Element) {
    const item = this.closest('.ws-item');
    if (item && hidden.has(item.id) && this.closest('.ws-state')) return { top: -900, bottom: -860, left: 0, right: 800, width: 800, height: 40, x: 0, y: -900, toJSON() {} } as DOMRect;
    return rect.call(this);
  });

  render(<App data={buildData()} />);
  await act(async () => {});
  observe();

  for (let step = 0; step < 40; step += 1) {
    const before = sent.length;
    let predicted: { n: number; verdict: Verdict } | null = null;
    let reset: 'all' | 'current' | null = null;
    const openBefore = new Set(openAt.keys());
    // The rows of the place in the order they are listed, to find the next open one after a verdict.
    const listed = [...document.querySelectorAll('.ws-list .ws-item')].map((item) => item.id);
    const choice = rand();
    // Now and then a row's buttons go out of sight, or come back.
    if (rand() < 0.2) {
      const keys = Object.keys(ISSUE_OF);
      const key = pick(keys);
      if (hidden.has(key)) hidden.delete(key);
      else if (rand() < 0.3) hidden.add(key);
    }

    if (choice < 0.15) {
      now += pick([100, 300, 650, 700, 1200]);
    } else if (choice < 0.36) {
      // Now and then the focus is first moved to the header of any row, open or closed, which need not be the
      // row opened last: Enter goes to that row only. On a closed header it is the button's own Enter, which opens it.
      const headers = [...document.querySelectorAll<HTMLButtonElement>('.ws-list button.ws-row')];
      // An open header of a row that was not opened last is where its own guard and sight count, so it is favoured;
      // a closed one matters while another row is open and ready.
      const earlier = headers.filter((header) => header.getAttribute('aria-expanded') === 'true' && header.closest('.ws-item')?.id !== current$());
      const closedHeaders = headers.filter((header) => header.getAttribute('aria-expanded') === 'false');
      // Often a guard has passed by then.
      if (rand() < 0.4) now += 700;
      const where = rand();
      if (where < 0.15) act(() => (document.activeElement as HTMLElement).blur());
      else if (where < 0.55 && headers.length) {
        const group = rand() < 0.7 ? earlier : closedHeaders;
        act(() => pick(group.length ? group : headers).focus());
      }
      const active = document.activeElement as HTMLElement;
      const repeat = rand() < 0.2;
      const onHeader = active.classList.contains('ws-row');
      const item = active === document.body ? (current$() ? document.getElementById(current$() as string) : null)
        : onHeader ? active.closest('.ws-item') : null;
      const headerOpen = onHeader && active.getAttribute('aria-expanded') === 'true';
      if (item && !repeat && !hidden.has(item.id) && (!onHeader || headerOpen)) predicted = verdictFor(item.id);
      const lastOpened = current$();
      let proceeds = true;
      act(() => { proceeds = fireEvent.keyDown(active, { key: 'Enter', repeat }); });
      if (onHeader && !headerOpen) {
        expect(proceeds, `step ${step} of seed ${seed}: Enter on a closed row's header is left to the button`).toBe(true);
        reached.closedHeader += 1;
        // Another open row is ready for a verdict, and the key still must not go to it.
        if ([...openAt.keys()].some((key) => verdictFor(key) && !hidden.has(key))) reached.closedBesideReady += 1;
      }
      if (predicted) {
        expect(proceeds, `step ${step} of seed ${seed}: a verdict taken with Enter must not also press the header`).toBe(false);
        if (onHeader && item?.id !== lastOpened) reached.otherHeader += 1;
        if (!onHeader && document.querySelector('.ws-item.open')?.id !== lastOpened) reached.page += 1;
      }
      // The browser then presses the focused button, unless the page took the Enter.
      if (proceeds && onHeader) {
        act(() => { fireEvent.click(active); });
        if (!headerOpen) expect(active.getAttribute('aria-expanded'), `step ${step} of seed ${seed}: Enter on a closed header opens its row`).toBe('true');
      }
    } else if (choice < 0.46) {
      const buttons = [...document.querySelectorAll<HTMLButtonElement>('.ws-item.open .ws-btn')]
        .filter((button) => /^(Take|Later|Drop)/.test(button.textContent ?? ''));
      if (buttons.length) {
        const button = pick(buttons);
        const label = (button.textContent ?? '').replace(/\s*↵$/, '');
        if (!button.disabled) predicted = verdictFor(button.closest('.ws-item')?.id ?? null, label);
        act(() => { fireEvent.click(button); });
      }
    } else if (choice < 0.68) {
      const rows = [...document.querySelectorAll<HTMLButtonElement>('.ws-list button.ws-row')];
      // Mostly a closed row, which stays open beside the others: several rows open is where the rules differ.
      const closed = rows.filter((row) => row.getAttribute('aria-expanded') === 'false');
      if (rows.length) {
        // Rows opened at different moments have guards at different points.
        if (rand() < 0.8) now += pick([300, 700, 1200]);
        const row = pick(closed.length && rand() < 0.7 ? closed : rows);
        act(() => { row.focus(); fireEvent.click(row); });
      }
    } else if (choice < 0.76) {
      // Go somewhere else: another area, the strip's lists, the decisions, the map, or the "to decide" link.
      // Often a guard has passed by then.
      if (rand() < 0.4) now += 700;
      const where = rand();
      // Back and Forward arrive as popstate; sent at once so the page and the model see the same address.
      if (where < 0.5) {
        act(() => { history.pushState(null, '', `/workshop${pick(PLACES)}`); dispatchEvent(new PopStateEvent('popstate')); });
        reset = 'all';
      }
      else if (where < 0.7) act(() => { fireEvent.keyDown(document.body, { key: 'Escape' }); });
      else {
        const link = [...document.querySelectorAll<HTMLAnchorElement>('.ws-loglink')].find((a) => /^\d+ to decide$/.test(a.textContent ?? ''));
        // Opening a row from the link starts its guard again, even when it was already open.
        if (link) {
          act(() => { fireEvent.click(link); });
          reset = 'current';
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
      // Another open row still waiting, the next one listed, becomes the current one with its guard started again,
      // even when it already was the current one.
      const taken = Object.keys(ISSUE_OF).find((key) => ISSUE_OF[key] === predicted?.n) as string;
      const at = listed.indexOf(taken);
      const next = [...listed.slice(at + 1), ...listed.slice(0, at)].find((key) => openBefore.has(key) && key !== taken
        && key.startsWith('entry-') && confirmed(ISSUE_OF[key]) === 'undecided' && !pending.has(ISSUE_OF[key]));
      if (next) openAt.delete(next);
      pending.set(predicted.n, predicted.verdict);
    } else {
      expect(fresh, `step ${step} of seed ${seed}: nothing should be sent`).toEqual([]);
    }
    const flying = inFlight.map((entry) => entry.n);
    expect(new Set(flying).size, `step ${step}: two verdicts in flight for one issue`).toBe(flying.length);
    // A move to another place opens its row afresh; the link restarts the guard of the row it opens.
    if (reset === 'all') openAt.clear();
    if (reset === 'current' && current$()) openAt.delete(current$() as string);
    observe();
    // Inside its own guard a row's verdict buttons look unarmed (the other way round waits for a timer the clock here never fires).
    for (const [key, at] of openAt) {
      const take = document.getElementById(key)?.querySelector('.ws-btn.go');
      if (take && now - at < OPEN_GUARD_MS) expect(take.classList.contains('ws-unarmed'), `step ${step} of seed ${seed}: ${key} looks armed inside its guard`).toBe(true);
    }
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
  return mostOpen;
}

describe('verdicts under random actions', () => {
  it('go to the open row only, respect the guard, and match the server or the pending write', async () => {
    let mostOpen = 0;
    for (let seed = 1; seed <= 22; seed += 1) {
      mostOpen = Math.max(mostOpen, await run(seed));
      cleanup();
      history.replaceState(null, '', '/workshop');
      vi.restoreAllMocks();
      vi.unstubAllGlobals();
    }
    // The sequences reach several rows open at once, and each case of Enter.
    expect(mostOpen).toBeGreaterThanOrEqual(2);
    expect(reached.closedHeader, 'Enter on a closed header').toBeGreaterThan(0);
    expect(reached.otherHeader, 'a verdict by Enter from the header of a row not opened last').toBeGreaterThan(0);
    expect(reached.closedBesideReady, 'Enter on a closed header beside an open row ready for a verdict').toBeGreaterThan(0);
    expect(reached.page, 'a verdict by Enter with the focus on the page, the row opened last not the first open').toBeGreaterThan(0);
  }, 120_000);
});
