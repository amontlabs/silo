// Shared by the page's tests: a fake API on fetch, and ways to find what a person sees.
import { act, render, screen, waitFor } from '@testing-library/react';
import { expect, onTestFinished, vi } from 'vitest';
import { App } from './App';
import type { IssueDetail, Me } from './api';
import type { BuildData } from './plan';
import { buildData, fixtureFacts, fixtureHistories, fixtureNotes, liveState } from './fixtures';

export type Call = { method: string; path: string; body: Record<string, unknown> | null; headers: Record<string, string> };
/** `cut`: the connection drops while the body is read, after the status and headers came. */
type Answer = { status: number; body: unknown; headers?: Record<string, string>; cut?: boolean };
export type Reply = Answer | Promise<Answer>;
export type Route = (url: URL, body: Record<string, unknown> | null) => Reply;

export const ok = (body: unknown = {}) => ({ status: 200, body });
export const refused = (status: number, message: string) => ({ status, body: { error: 'refused', message } });

export function detail(n: number, fields: Partial<IssueDetail> = {}): IssueDetail {
  return { number: n, title: `Issue ${n}`, body: `Body of ${n}.`, url: `https://github.com/amontlabs/silo/issues/${n}`, up: 0, comments: [], ...fields };
}

export function mockApi(options: { state?: Route; me?: Me | null; routes?: Record<string, Route> } = {}) {
  const calls: Call[] = [];
  if (options.me) document.cookie = 'workshop-hint=1; path=/';
  const routes: Record<string, Route> = {
    'GET state': options.state ?? (() => ok(liveState())),
    'GET me': () => (options.me ? ok(options.me) : refused(401, 'Not signed in.')),
    'GET issue': (url) => ok(detail(Number(url.searchParams.get('n')))),
    'GET vote': () => ok({ on: false }),
    'POST comment': () => ok(),
    'POST vote': () => ok(),
    'POST logout': () => ok(),
    ...options.routes,
  };
  vi.stubGlobal('fetch', vi.fn(async (input: string, init?: RequestInit) => {
    const url = new URL(input, 'http://localhost');
    const method = init?.method ?? 'GET';
    const path = url.pathname.replace('/api/workshop/', '');
    const body = init?.body ? (JSON.parse(String(init.body)) as Record<string, unknown>) : null;
    calls.push({ method, path, body, headers: (init?.headers ?? {}) as Record<string, string> });
    const route = routes[`${method} ${path}`];
    const signal = init?.signal;
    const aborted = new Promise<never>((_, reject) => signal?.addEventListener('abort', () => reject(new DOMException('Aborted', 'AbortError'))));
    const reply: Answer = route ? await Promise.race([Promise.resolve(route(url, body)), aborted]) : refused(404, 'Not found.');
    const text = JSON.stringify(reply.body);
    const payload = reply.cut
      ? new ReadableStream({ start(controller) { controller.enqueue(new TextEncoder().encode(text.slice(0, 5))); controller.error(new TypeError('network error')); } })
      : text;
    return new Response(payload, { status: reply.status, headers: { 'Content-Type': 'application/json', ...reply.headers } });
  }));
  return calls;
}

export const posts = (calls: Call[], path: string) => calls.filter((call) => call.method === 'POST' && call.path === path);
export const owner: Me = { login: 'polarzero', avatar: '' };
export const visitor: Me = { login: 'mara-k', avatar: '' };

export function rowButton(name: string) {
  return screen.getByRole('button', { name: new RegExp(`^${name}`) });
}
/** The item of a row: All's rows (`.wa-item`) and the suggestions' (`.ws-item`). */
export function itemOf(name: string) {
  return rowButton(name).closest('.wa-item, .ws-item') as HTMLElement;
}
export function part(id: string) {
  return document.querySelector<HTMLButtonElement>(`.ws-part[data-part="${id}"]`) as HTMLButtonElement;
}
export function tile(id: string) {
  return document.querySelector<HTMLButtonElement>(`.ws-tile[data-part="${id}"] .ws-tile-hit`) as HTMLButtonElement;
}
export const partTitle = () => document.getElementById('ws-part-title');

export async function renderApp(data: BuildData = buildData(), loadNotes = () => Promise.resolve(fixtureNotes(data)), loadFacts = () => Promise.resolve(fixtureFacts(data)), loadHistory = () => Promise.resolve(fixtureHistories(data))) {
  const view = render(<App data={data} loadNotes={loadNotes} loadFacts={loadFacts} loadHistory={loadHistory} />);
  // Settled: the load has answered or failed, and the account shows who is signed in.
  await waitFor(() => expect(document.querySelector('.ws[data-live="answered"], .ws[data-live="failed"]')).not.toBeNull());
  await waitFor(() => expect(document.querySelector('.ws-account, .ws-account-inline a')).not.toBeNull());
  return view;
}

export async function ownerOn(name: string) {
  await waitFor(() => expect(rowButton(name)).toHaveAttribute('aria-expanded', 'true'));
}

export function clock() {
  let now = 1_000_000;
  vi.spyOn(Date, 'now').mockImplementation(() => now);
  return { advance(ms: number) { now += ms; } };
}

/**
 * Keeps the page's timers of exactly `ms` from firing on their own: the test fires them when it chooses, so a busy
 * machine cannot make one go off before a step the test takes. Nothing real is scheduled for a withheld timer, and
 * one the page clears is cleared as usual. `fire(count)` fails unless exactly `count` (one by default) are withheld,
 * runs them in the order they were set, and skips one an earlier one cleared. At the end of the test, a withheld
 * timer that was neither fired nor cleared fails it, unless the test says it `leaves` it.
 */
export function withheldTimers(ms: number, { leaves = false }: { leaves?: boolean } = {}) {
  const realSet = globalThis.setTimeout;
  const realClear = globalThis.clearTimeout;
  const held = new Map<unknown, () => void>();
  let ids = 2 ** 40;
  vi.spyOn(globalThis, 'setTimeout').mockImplementation(((fn: () => void, delay?: number, ...args: unknown[]) => {
    if (delay !== ms) return realSet(fn, delay, ...args);
    ids += 1;
    held.set(ids, fn);
    return ids;
  }) as typeof setTimeout);
  vi.spyOn(globalThis, 'clearTimeout').mockImplementation(((id?: Parameters<typeof clearTimeout>[0]) => {
    if (held.delete(id)) return;
    realClear(id);
  }) as typeof clearTimeout);
  onTestFinished(() => {
    if (!leaves && held.size > 0) throw new Error(`${held.size} timer(s) of ${ms} ms were withheld and neither fired nor cleared`);
  });
  return {
    fire: (count = 1) => act(async () => {
      if (held.size !== count) throw new Error(`${held.size} timer(s) of ${ms} ms are withheld, the test expects ${count}`);
      for (const [id, fn] of [...held]) {
        if (!held.delete(id)) continue;
        fn();
      }
    }),
  };
}

export const wait = (ms: number) => act(async () => { await new Promise((resolve) => setTimeout(resolve, ms)); });

export const notesButton = () => screen.getByRole('button', { name: 'Notes' });
/** The notes once their chunk has loaded. */
export const shownNotes = () => waitFor(() => {
  const md = document.querySelector('.ws-md');
  expect(md).not.toBeNull();
  return md as HTMLElement;
});

/** The row of `core` opened from the address, as the owner unless another person is given. */
export async function openCore(data = buildData(), me = owner) {
  mockApi({ me });
  history.replaceState(null, '', '/workshop#part-a/core');
  await renderApp(data);
  await waitFor(() => expect(rowButton('The engine runs on its own')).toHaveAttribute('aria-expanded', 'true'));
}
