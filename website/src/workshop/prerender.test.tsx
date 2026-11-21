// The markup the build writes into the page (prerender.tsx) and the script's takeover of it (takeover.ts).
import { join } from 'node:path';
import { StrictMode, act } from 'react';
import type { ReactNode } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { App } from './App';
import { Boundary } from './Boundary';
import { buildDataFrom } from './data';
import { loadDir } from '../../../tools/plan/read/index.js';
import { buildData, emptyPlanFiles } from './fixtures';
import { NO_MAP } from './map';
import { inject, prerender } from './prerender';
import { NOSCRIPT_LINE } from './NoScript';
import { takeOver } from './takeover';
import { mockApi, owner } from './test-support';

const SAMPLE = join(process.cwd(), '../tools/plan/test/fixtures/sample');

/** The markup as a browser holds it: React's text separators dropped, adjacent texts joined, styles rewritten. */
function normal(html: string): string {
  const box = document.createElement('div');
  box.innerHTML = html;
  const walker = document.createTreeWalker(box, NodeFilter.SHOW_COMMENT);
  const comments: ChildNode[] = [];
  while (walker.nextNode()) comments.push(walker.currentNode as ChildNode);
  comments.forEach((node) => node.remove());
  box.normalize();
  // A style as the browser writes it back (the server writes `--x:24`, the browser `--x: 24;`).
  for (const node of box.querySelectorAll<HTMLElement>('[style]')) node.setAttribute('style', node.style.cssText);
  return box.innerHTML;
}

/** The markup as the build makes it: without a window, a document, an address, storage or media queries. */
function built(data = buildData()) {
  // Undefined, any read of these throws.
  for (const name of ['window', 'document', 'location', 'localStorage', 'matchMedia', 'history']) vi.stubGlobal(name, undefined);
  try {
    return prerender(data);
  } finally {
    vi.unstubAllGlobals();
  }
}

/** The built page's two containers, holding the prerendered markup. */
function page(data = buildData()) {
  const parts = built(data);
  document.body.innerHTML = `<div class="ws-account-slot" id="workshop-account" data-prerendered>${parts.account}</div><main><div id="workshop-root" data-prerendered>${parts.root}</div></main>`;
  return { parts, root: document.getElementById('workshop-root') as HTMLElement, slot: document.getElementById('workshop-account') as HTMLElement };
}

const workshop = (data: ReturnType<typeof buildData>, slot: Element | null): ReactNode => (
  <StrictMode>
    <Boundary data={data}>
      <App data={data} accountSlot={slot} />
    </Boundary>
  </StrictMode>
);

let unmount: (() => Promise<void>) | null = null;
afterEach(async () => {
  await unmount?.();
  unmount = null;
  document.body.innerHTML = '';
  document.documentElement.classList.remove('ws-hold', 'ws-bare');
});

/** Takes over the page; what the takeover leaves is read before anything else runs. */
function take(root: HTMLElement, slot: Element | null, node: ReactNode) {
  const react = takeOver(root, slot, node);
  const seen = { root: root.innerHTML, slot: slot?.innerHTML ?? '' };
  // The takeover renders outside act, so React leaves the work it schedules (effects, updates from answers) to its
  // scheduler, which runs it on a later turn of the event loop. Left pending at the end of the file, that work runs
  // after the test environment has removed `window` and fails the run: the test unmounts, then lets it run.
  unmount = async () => {
    act(() => react.unmount());
    await new Promise((done) => setTimeout(done, 0));
  };
  return seen;
}

describe('the prerendered Workshop', () => {
  it.each([
    ['a plan with parts', buildData],
    ['a plan with no part', () => buildDataFrom(emptyPlanFiles(), { map: NO_MAP })],
    // A plan read from another folder, as the real build reads it: parts without a map, no verdicts.
    ['the plan tool\'s sample, as the real build reads it', () => buildDataFrom(loadDir(SAMPLE), { map: NO_MAP })],
  ])('is what the script renders first on a first visit without a session at the plain address, for %s', (_, plan) => {
    mockApi({ state: () => new Promise(() => {}) });
    const data = plan();
    const { parts, root, slot } = page(data);
    const seen = take(root, slot, workshop(data, slot));
    expect(normal(seen.root)).toBe(normal(parts.root));
    expect(normal(seen.slot)).toBe(normal(parts.account));
    expect(root.hasAttribute('data-prerendered')).toBe(false);
    expect(slot.hasAttribute('data-prerendered')).toBe(false);
    expect(slot.querySelectorAll('.ws-signin-wrap')).toHaveLength(1);
    // The script gives the page an account, so it is not marked bare.
    expect(document.documentElement.classList.contains('ws-bare')).toBe(false);
  });

  it('holds the title row, every part of the map and every tile, and sign-in in the header', () => {
    const data = buildData();
    const { root, slot } = page(data);
    expect(root.querySelector('.ws.is-sheet .ws-top h1')?.textContent).toBe('Workshop');
    expect([...root.querySelectorAll('.ws-top .ws-places a')].map((a) => a.textContent)).toEqual(['Product', 'Suggest a feature']);
    expect(root.querySelector('.ws-top .ws-places .wa-searchctl')?.getAttribute('aria-label')).toBe('Search');
    expect([...root.querySelectorAll('.ws-top .wa-view button')].map((button) => [button.textContent, button.getAttribute('aria-pressed')])).toEqual([['MapParts', 'true'], ['All', 'false']]);
    for (const part of data.parts) {
      expect(root.querySelector(`.ws-part[data-part="${part.id}"]`)).not.toBeNull();
      expect(root.querySelector(`.ws-tile[data-part="${part.id}"]`)).not.toBeNull();
    }
    expect(root.querySelector('.ws-account-inline, .ws-signin-wrap')).toBeNull();
    expect(slot.querySelector('a.button')?.getAttribute('href')).toBe('/api/workshop/login?return=%2Fworkshop');
  });

  it('is replaced in one step when the browser remembers who is signed in: their login, and no sign-in', () => {
    mockApi({ state: () => new Promise(() => {}), routes: { 'GET me': () => new Promise(() => {}) } });
    document.cookie = 'workshop-hint=1; path=/';
    localStorage.setItem('silo-workshop-me', JSON.stringify(owner));
    const data = buildData();
    const { parts, root, slot } = page(data);
    const seen = take(root, slot, workshop(data, slot));
    expect(normal(seen.slot)).not.toBe(normal(parts.account));
    expect(slot.children).toHaveLength(1);
    expect(slot.querySelector('.ws-login')?.textContent).toBe(owner.login);
    expect(slot.querySelector('.ws-signin-wrap')).toBeNull();
    expect(root.querySelector('.ws-account-inline')).toBeNull();
  });

  it('gives the address its own view in the same step, and lifts the hold on what the build drew', () => {
    mockApi({ state: () => new Promise(() => {}) });
    const data = buildData();
    history.replaceState(null, '', `/workshop#${data.parts[0].id}`);
    document.documentElement.classList.add('ws-hold');
    const { root, slot } = page(data);
    take(root, slot, workshop(data, slot));
    expect(root.querySelector('.ws.is-open #ws-part-title')).not.toBeNull();
    expect(document.documentElement.classList.contains('ws-hold')).toBe(false);
  });

  it.each([false, true])('keeps the drawn sign-in, shown, when the page fails to render (session marker: %s)', (marker) => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    if (marker) document.documentElement.classList.add('ws-session');
    const data = buildData();
    const { root, slot } = page(data);
    const Broken = () => { throw new Error('broken'); };
    take(root, slot, <Boundary data={data}><Broken /></Boundary>);
    expect(root.querySelector('.ws-plain h1')?.textContent).toBe('Workshop');
    expect(slot.querySelectorAll('.ws-signin-wrap')).toHaveLength(1);
    // ws-bare lifts the session marker's placeholder (workshop.css): the drawn link shows.
    expect(document.documentElement.classList.contains('ws-bare')).toBe(true);
    document.documentElement.classList.remove('ws-session');
  });

});

describe('the Workshop without JavaScript', () => {
  /** The list the build writes into the page's <noscript>, as a browser without JavaScript reads it. */
  function noscript(data = buildData()) {
    const box = document.createElement('div');
    box.innerHTML = built(data).noscript;
    return box;
  }
  const rowsOf = (section: Element | null) => [...(section?.querySelectorAll('li') ?? [])].map((li) => li.textContent);

  it('lists a part with its features, then its decisions: id, name and state, each group in the order of order.json', () => {
    const box = noscript();
    expect(box.querySelector('.ws-quiet')?.textContent).toBe(NOSCRIPT_LINE);
    const partA = box.querySelector('section[data-part="part-a"]');
    expect(partA?.querySelector('h2')?.textContent).toBe('Part A');
    expect(rowsOf(partA)).toEqual([
      'core The engine runs on its own Decided.',
      'calls Every call defined once Decided.',
      'core-in-zig Which language is the core written in? Decided.',
    ]);
    expect([...partA!.querySelectorAll('h3')].map((heading) => heading.textContent)).toEqual(['Features', 'Decisions']);
    expect(rowsOf(box.querySelector('section[data-part="part-b"]'))).toContain('create Create and delete a computer Not decided.');
    expect(rowsOf(box.querySelector('section[data-part="part-c"]'))).toContain('shells Your shell inside Being built.');
    expect([...box.querySelectorAll('section')].map((section) => section.getAttribute('data-part'))).toEqual([...buildData().parts.map((part) => part.id), 'product']);
  });

  it('lists the decisions of Product, and keeps them on a plan with no part', () => {
    for (const [data, ids] of [[buildData(), ['security-properly', 'one-host', 'plain-logs']], [buildDataFrom(emptyPlanFiles(), { map: NO_MAP }), ['security-properly', 'one-host']]] as const) {
      const product = noscript(data).querySelector('section[data-part="product"]');
      expect(product?.querySelector('h2')?.textContent).toBe('Product');
      expect(rowsOf(product)?.map((row) => row?.split(' ')[0])).toEqual(ids);
    }
  });

  it('says a decision is replaced, and by which one', () => {
    const box = noscript();
    expect(rowsOf(box.querySelector('section[data-part="product"]'))).toContain('plain-logs Which logs does Silo keep? Replaced. Replaced by signed-logs How are logs kept honest?');
    // Only the replaced decision has the line.
    expect(box.textContent?.match(/Replaced by/g)).toHaveLength(1);
  });

  it('goes into the page\'s <noscript> only, so the page drawn with JavaScript is unchanged', () => {
    const parts = built();
    const html = '<div class="ws-account-slot" id="workshop-account"></div><div id="workshop-root"></div><noscript id="workshop-noscript"><p>line</p></noscript>';
    const out = inject(html, parts);
    expect(out).toContain(`<noscript id="workshop-noscript">${parts.noscript}</noscript>`);
    expect(out).toContain(`<div id="workshop-root" data-prerendered>${parts.root}</div>`);
    expect(parts.root).not.toContain('ws-noscript');
    expect(() => inject(html.replace(/<noscript[\s\S]*<\/noscript>/, ''), parts)).toThrow('must hold');
  });
});
