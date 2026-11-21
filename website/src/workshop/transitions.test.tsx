// Moves between places (opening a part or a tile, Map/Order, Full map, Escape, Back, an address followed), run in
// sequences on a clock that is faked, and taps during a view transition.
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { App } from './App';
import { buildData, fixtureNotes } from './fixtures';
import { partTitle, itemOf, mockApi, owner, part, posts, renderApp, rowButton, tile, wait } from './test-support';

type Mode = 'finishing' | 'failing' | 'absent';
type Move =
  | { kind: 'open'; part: string }
  | { kind: 'view'; to: 'map' | 'order' }
  | { kind: 'full-map' }
  | { kind: 'escape' }
  | { kind: 'back' }
  | { kind: 'address'; part: string };
type Step = Move & { after: number };
type Config = { motion: boolean; wide: boolean; mode: Mode };

const PARTS = ['part-b', 'part-d', 'part-a', 'part-e'];
/** Milliseconds between two moves: none, a few, and around the lengths of the page's own animations. */
const GAPS = [0, 3, 20, 60, 130, 140, 200, 500];

/** A screen: whether motion is wanted and whether the map fits, as `matchMedia` answers for the page. */
function screenOf({ motion, wide }: Config) {
  vi.stubGlobal('matchMedia', (query: string) => ({
    matches: query.includes('prefers-reduced-motion') ? !motion : query.includes('min-width: 1200px') ? wide : false,
    media: query, addEventListener() {}, removeEventListener() {},
  }));
}

/**
 * The browser's view transitions: the old picture is taken, and the update runs a little later; a transition ends
 * a while after that, or fails. Handlers are attached by the page in the call, so none is left unhandled.
 */
function viewTransitions(mode: Mode) {
  const doc = document as unknown as { startViewTransition?: unknown };
  if (mode === 'absent') {
    delete doc.startViewTransition;
    return;
  }
  doc.startViewTransition = (update: () => void) => {
    const end = (resolve: () => void, reject: (reason: Error) => void) => setTimeout(() => (mode === 'failing' ? reject(new Error('skipped')) : resolve()), 280);
    setTimeout(update, 30);
    return {
      finished: new Promise<void>((resolve, reject) => end(resolve, reject)),
      ready: mode === 'failing' ? Promise.reject(new Error('skipped')) : Promise.resolve(),
      updateCallbackDone: Promise.resolve(),
      skipTransition() {},
    };
  };
}

/** The window's own `scrollY`, put back after a test that sets it. */
const SCROLL_Y = Object.getOwnPropertyDescriptor(window, 'scrollY');
const restoreScrollY = () => { if (SCROLL_Y) Object.defineProperty(window, 'scrollY', SCROLL_Y); };
const elapse = (ms: number) => act(async () => { await vi.advanceTimersByTimeAsync(ms); });
const settleAll = () => act(async () => { await vi.runAllTimersAsync(); });
const rootOf = () => document.querySelector('.ws') as HTMLElement;
const classesOf = () => [...rootOf().classList].sort();
const hashOf = (part: string) => (part ? `#${part}` : '');

/** The part the page shows ('' for the full map), read from the open tile of the strip. */
function shownPart(): string {
  if (!document.querySelector('.ws-detail')) return '';
  return document.querySelector('.ws-tile.on')?.getAttribute('data-part') ?? '?';
}
const shownView = () => (rootOf().classList.contains('v-order') ? 'order' : 'map');
const addressView = () => (new URLSearchParams(location.search).get('view') === 'order' ? 'order' : 'map');

/** The app on a fresh page, with the clock faked from the start so that every timer it makes can be run. */
async function mount() {
  history.replaceState(null, '', '/workshop');
  vi.useFakeTimers();
  mockApi();
  const view = render(<App data={buildData()} loadNotes={() => Promise.resolve(fixtureNotes(buildData()))} />);
  await elapse(600);
  expect(rootOf()).toHaveAttribute('data-live', 'answered');
  return view;
}

/**
 * What the root's classes are when nothing is moving, for an address: read once from a page that was opened on it.
 * Whatever else a settled page carries is a state that was left behind.
 */
async function atRest(hash: string, search: string) {
  history.replaceState(null, '', `/workshop${search}${hash}`);
  vi.useFakeTimers();
  mockApi();
  render(<App data={buildData()} loadNotes={() => Promise.resolve(fixtureNotes(buildData()))} />);
  await elapse(600);
  await settleAll();
  const classes = classesOf();
  cleanup();
  vi.useRealTimers();
  return classes;
}

/** True when the element, or one that holds it, is taken out of sight by an inline style or `hidden`. */
function hiddenInline(node: Element | null): boolean {
  for (let at = node; at && at !== document.body; at = at.parentElement) {
    const style = (at as HTMLElement).style;
    if (at.hasAttribute('hidden') || style.display === 'none' || style.visibility === 'hidden' || style.opacity === '0') return true;
  }
  return false;
}

describe('moves between places', () => {
  afterEach(() => {
    vi.useRealTimers();
    delete (document as unknown as { startViewTransition?: unknown }).startViewTransition;
  });

  // Seeded, so a failure names a sequence that runs again the same.
  function random(seed: number) {
    let state = seed;
    return () => {
      state = (state + 0x6d2b79f5) | 0;
      let t = Math.imul(state ^ (state >>> 15), 1 | state);
      t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }
  function sequence(seed: number, length: number): Step[] {
    const rand = random(seed);
    const pick = <T,>(list: T[]) => list[Math.floor(rand() * list.length)];
    return Array.from({ length }, () => {
      const after = pick(GAPS);
      const roll = rand();
      if (roll < 0.35) return { kind: 'open', part: pick(PARTS), after } as Step;
      if (roll < 0.5) return { kind: 'view', to: pick(['map', 'order'] as const), after };
      if (roll < 0.62) return { kind: 'full-map', after };
      if (roll < 0.74) return { kind: 'escape', after };
      if (roll < 0.86) return { kind: 'back', after };
      return { kind: 'address', part: pick(['', ...PARTS]), after };
    });
  }
  const open = (part: string, after = 0): Step => ({ kind: 'open', part, after });
  const view = (to: 'map' | 'order', after = 0): Step => ({ kind: 'view', to, after });
  const named: [string, Step[]][] = [
    ['a part clicked while the view swaps', [view('order'), open('part-d')]],
    ['a part, then another after the first has landed', [open('part-b', 600), open('part-d', 600), view('order', 600), view('map')]],
    ['the full map taken again and again', [open('part-b', 600), { kind: 'full-map', after: 600 }, open('part-d', 400), { kind: 'full-map', after: 0 }, open('part-a'), { kind: 'full-map', after: 0 }, open('part-e', 20)]],
    ['a second move while the first waits for its view transition', [open('part-a'), open('part-b', 10), { kind: 'escape', after: 10 }, open('part-d', 0)]],
    ['an address that is followed while a view transition waits', [open('part-a'), { kind: 'address', part: 'part-b', after: 5 }]],
    ['Escape and Back after a view', [open('part-b', 600), view('order', 600), { kind: 'escape', after: 600 }, { kind: 'back', after: 600 }, view('map', 0), { kind: 'back', after: 0 }]],
    ['Order and a tile while the map unfolds', [open('part-b', 600), { kind: 'full-map', after: 0 }, view('order', 135), open('part-d')]],
    ['Back to an entry without the view', [open('part-b', 600), view('order', 600), { kind: 'back', after: 600 }]],
    ['Back to an entry without the view, then Order', [open('part-b', 600), view('order', 600), { kind: 'back', after: 600 }, view('order', 600)]],
    ['Back while the view swaps', [open('part-b', 600), view('order', 20), { kind: 'back', after: 600 }]],
    ['the part being left opened again while the map unfolds', [open('part-b', 600), { kind: 'full-map', after: 60 }, open('part-b', 600)]],
    ['the part being left opened again at once', [open('part-b', 600), { kind: 'escape', after: 10 }, open('part-b', 600)]],
    ['Escape again while the map unfolds', [open('part-b', 600), { kind: 'escape', after: 100 }, { kind: 'escape', after: 60 }]],
    ['Escape again while the view transition waits', [open('part-b', 600), { kind: 'escape', after: 20 }, { kind: 'escape', after: 20 }]],
  ];
  const handWritten = new Set(named.map(([name]) => name));
  const configs: Config[] = [true, false].flatMap((motion) => [true, false].flatMap((wide) => (['finishing', 'failing', 'absent'] as Mode[]).map((mode) => ({ motion, wide, mode }))));

  it('always ends on the place and the view the address names, with nothing left half done', async () => {
    // A view transition that fails is handled by the page: no rejection is left over.
    const unhandled: string[] = [];
    const record = (reason: unknown) => unhandled.push(String(reason));
    process.on('unhandledRejection', record);
    // What a settled page's root carries, for each of the four kinds of address.
    const rest: Record<string, string[]> = {
      'full-map map': await atRest('', ''),
      'full-map order': await atRest('', '?view=order'),
      'part map': await atRest('#part-b', ''),
      'part order': await atRest('#part-b', '?view=order'),
    };
    const moving = new Set<string>();
    try {
      for (const config of configs) {
        const sequences: [string, Step[]][] = [...named, ...[1, 2, 3, 4, 5, 6, 77].map((seed): [string, Step[]] => [`seed ${seed}`, sequence(seed, 7)])];
        for (const [name, steps] of sequences) {
          const where = `${name} (${JSON.stringify(config)}): ${steps.map((step) => `${step.kind}${'part' in step ? ` ${step.part}` : 'to' in step ? ` ${step.to}` : ''} +${step.after}`).join(', ')}`;
          screenOf(config);
          viewTransitions(config.mode);
          await mount();
          const push = vi.spyOn(history, 'pushState');
          // What the last move that took effect asks for; null where the address decides (after Back).
          let place: string | null = '';
          let wanted: 'map' | 'order' | null = 'map';
          let depth = 0;
          let heldHeight = false;
          // When the going-back under way lands: as it started, never later. Null when none is under way.
          let landsAt: number | null = null;
          const landed = () => {
            if (landsAt === null || Date.now() < landsAt) return;
            expect(shownPart(), `${where}: going back has landed at its time`).toBe('');
            landsAt = null;
          };
          // A new move lets go at once of the height a going-back kept, before anything has settled.
          const released = () => expect(rootOf().style.minHeight, `${where}: the page height is let go when a move starts`).toBe('');
          const note = () => { for (const name of classesOf()) if (!rest['full-map map'].includes(name) && !rest['part order'].includes(name)) moving.add(name); };
          for (const step of steps) {
            landed();
            const here = shownPart();
            const onMap = rootOf().classList.contains('v-map');
            // Going back from a part: the map unfolds for a while, a view transition waits for its update, or it is
            // done at once. Another going-back meanwhile changes nothing.
            const goBack = () => {
              place = '';
              if (landsAt !== null) return;
              const unfolds = config.motion && config.wide && onMap;
              const waits = config.motion && !unfolds && config.mode !== 'absent';
              landsAt = unfolds ? Date.now() + 140 : waits ? Date.now() + 30 : null;
              // A map that unfolds keeps the page's height meanwhile, so the page below does not jump.
              heldHeight = unfolds;
            };
            // Read before the move, so that the entries it pushes at once are counted too.
            const before = push.mock.calls.length;
            if (step.kind === 'open') {
              const tiles = !config.wide || rootOf().classList.contains('v-order');
              const button = tiles ? tile(step.part) : part(step.part);
              act(() => { fireEvent.click(button); });
              if (here !== step.part) {
                place = step.part;
                landsAt = null;
                released();
              } else if (landsAt !== null) {
                // The part being left is asked for again: it stays open, and the going-back is dropped.
                place = here;
                landsAt = null;
                released();
              } else goBack();
            } else if (step.kind === 'view') {
              act(() => { fireEvent.click(screen.getByRole('button', { name: step.to === 'order' ? 'Order' : 'Map' })); });
              wanted = step.to;
            } else if (step.kind === 'full-map' || step.kind === 'escape') {
              const back = document.querySelector<HTMLButtonElement>('.ws-back');
              if (step.kind === 'full-map' && back) act(() => { fireEvent.click(back); });
              else if (step.kind === 'escape') act(() => { fireEvent.keyDown(document.body, { key: 'Escape' }); });
              if (here !== '' && (step.kind === 'escape' || back)) goBack();
            } else if (step.kind === 'back') {
              // Each Back of a hand-written case has an entry to go back to; there, it is what is being checked.
              if (handWritten.has(name)) expect(depth, `${where}: a Back that goes back`).toBeGreaterThan(0);
              if (depth > 0) {
                depth -= 1;
                act(() => history.back());
                await elapse(1);
                place = null;
                wanted = null;
                landsAt = null;
                released();
              }
            } else {
              act(() => { history.pushState(null, '', `/workshop${location.search}${hashOf(step.part)}`); dispatchEvent(new PopStateEvent('popstate')); });
              place = step.part;
              // An address followed names its view, as on a fresh load.
              wanted = addressView();
              landsAt = null;
              released();
            }
            if (heldHeight) {
              expect(rootOf().style.minHeight, `${where}: the page height is kept while the map unfolds`).not.toBe('');
              heldHeight = false;
            }
            await elapse(step.after);
            // A push is one more entry to go back to; a pushed address also drops the ones ahead.
            depth += push.mock.calls.length - before;
            note();
          }
          landed();
          await settleAll();
          const address = location.href;
          const hash = location.hash;
          // The place and the view the address names are the ones shown, and the last move is the one that won.
          const named = hash === '' ? '' : decodeURIComponent(hash.slice(1));
          expect(shownPart(), `${where}: the place the address names`).toBe(named);
          if (named) expect(partTitle()?.textContent, `${where}: the title of the part`).toBe(document.querySelector(`.ws-tile[data-part="${named}"] .ws-title`)?.textContent);
          else expect(partTitle(), `${where}: no part is open`).toBeNull();
          if (place !== null) expect(named, `${where}: the last move decides the place`).toBe(place);
          // The view shown is the one the address names, and the one last chosen unless Back decided it.
          expect(shownView(), `${where}: the view the address names`).toBe(addressView());
          if (wanted !== null) expect(shownView(), `${where}: the view last chosen`).toBe(wanted);
          expect(screen.getByRole('button', { name: shownView() === 'order' ? 'Order' : 'Map' }), `${where}: the pressed button`).toHaveAttribute('aria-pressed', 'true');
          // Nothing is left half done: no state class beyond those of a page at rest, no height kept, all in sight.
          expect(classesOf(), `${where}: the classes of a page at rest`).toEqual(rest[`${named ? 'part' : 'full-map'} ${shownView()}`]);
          expect(rootOf().style.minHeight, `${where}: no height kept`).toBe('');
          expect(rootOf().style.height, `${where}: no height set`).toBe('');
          for (const node of [rootOf(), screen.getByRole('heading', { level: 1, name: 'Workshop' }), document.querySelector('.ws-swap-map'), document.querySelector('.ws-swap-order')]) {
            expect(node, `${where}: the title and the map are there`).not.toBeNull();
            expect(hiddenInline(node), `${where}: the title and the map are in sight`).toBe(false);
          }
          expect(document.querySelectorAll('.ws-swap-map .ws-part').length, `${where}: the map has its parts`).toBeGreaterThan(0);
          expect(document.querySelectorAll('.ws-swap-order .ws-tile').length, `${where}: the tiles are there`).toBeGreaterThan(0);
          // Later timers change nothing: the address stays what it was.
          await elapse(10_000);
          expect(location.href, `${where}: the address after the last move`).toBe(address);
          expect(shownPart(), `${where}: the place stays`).toBe(named);
          cleanup();
          vi.useRealTimers();
          vi.restoreAllMocks();
        }
      }
    } finally {
      process.off('unhandledRejection', record);
    }
    expect(unhandled).toEqual([]);
    // The check did see the page move: some state class was set on the way.
    expect(moving.size).toBeGreaterThan(0);
  }, 120_000);
});

describe('Back, Forward and Escape', () => {
  afterEach(() => {
    vi.useRealTimers();
    delete (document as unknown as { startViewTransition?: unknown }).startViewTransition;
  });

  it('moves the focus out of the view that Back or Forward hides, to the same part in the one shown', async () => {
    screenOf({ motion: false, wide: true, mode: 'absent' });
    await mount();
    act(() => { fireEvent.click(part('part-b')); });
    act(() => { fireEvent.click(screen.getByRole('button', { name: 'Order' })); });
    act(() => tile('part-b').focus());
    act(() => history.back());
    await elapse(1);
    expect(shownView()).toBe('map');
    expect(document.activeElement).toBe(part('part-b'));
    act(() => history.forward());
    await elapse(1);
    expect(shownView()).toBe('order');
    expect(document.activeElement).toBe(tile('part-b'));
    // The focus outside the panel that is hidden stays where it is: a link of the title row, the Order button.
    const latest = screen.getByRole('link', { name: 'Latest' });
    act(() => latest.focus());
    act(() => history.back());
    await elapse(1);
    expect(shownView()).toBe('map');
    expect(document.activeElement).toBe(latest);
    const order = screen.getByRole('button', { name: 'Order' });
    act(() => order.focus());
    act(() => history.forward());
    await elapse(1);
    expect(shownView()).toBe('order');
    expect(document.activeElement).toBe(order);
  });

  it('keeps the focus on a link of the title row that Back or Forward leaves on the page', async () => {
    screenOf({ motion: false, wide: true, mode: 'absent' });
    await mount();
    // The full map in Map, a part in Order, then the full map again in Order.
    act(() => { fireEvent.click(part('part-b')); });
    act(() => { fireEvent.click(screen.getByRole('button', { name: 'Order' })); });
    act(() => { fireEvent.keyDown(document.body, { key: 'Escape' }); });
    await elapse(1);
    expect(location.hash).toBe('');
    expect(shownView()).toBe('order');
    // Back to the full map in Map, then Forward to the full map in Order: "Suggest a feature" shows in both.
    act(() => history.go(-2));
    await elapse(1);
    expect(shownView()).toBe('map');
    const suggest = screen.getByRole('link', { name: 'Suggest a feature' });
    act(() => suggest.focus());
    act(() => history.go(2));
    await elapse(1);
    expect(location.hash).toBe('');
    expect(shownView()).toBe('order');
    expect(document.activeElement).toBe(suggest);
    // Back to the full map in Map with the focus on Latest; Forward to the part keeps the row on the page.
    act(() => history.go(-2));
    await elapse(1);
    expect(location.hash).toBe('');
    expect(shownView()).toBe('map');
    const latest = screen.getByRole('link', { name: 'Latest' });
    act(() => latest.focus());
    act(() => history.forward());
    await elapse(1);
    expect(location.hash).toBe('#part-b');
    expect(document.activeElement).toBe(latest);
  });

  it('drops a going-back when Back leads to an address that is not known yet', async () => {
    screenOf({ motion: true, wide: true, mode: 'finishing' });
    viewTransitions('finishing');
    await mount();
    // An entry the page has not seen yet, then a part opened over it.
    act(() => { history.pushState(null, '', '/workshop#suggested/issue-999999'); });
    act(() => { fireEvent.click(part('part-b')); });
    await elapse(600);
    act(() => { fireEvent.keyDown(document.body, { key: 'Escape' }); });
    await elapse(20);
    const push = vi.spyOn(history, 'pushState');
    act(() => history.back());
    await elapse(1000);
    expect(push).not.toHaveBeenCalled();
    expect(rootOf().classList.contains('ws-leaving')).toBe(false);
    expect(rootOf().style.minHeight).toBe('');
  });

  it('goes back once for a held Escape', async () => {
    screenOf({ motion: false, wide: true, mode: 'absent' });
    await mount();
    act(() => { fireEvent.click(part('part-b')); });
    act(() => { fireEvent.keyDown(document.body, { key: 'Escape', repeat: true }); });
    expect(shownPart()).toBe('part-b');
    act(() => { fireEvent.keyDown(document.body, { key: 'Escape' }); });
    expect(shownPart()).toBe('');
  });
});

describe('a tap during a view transition', () => {
  function narrow() {
    vi.stubGlobal('matchMedia', (query: string) => ({ matches: false, media: query, addEventListener() {}, removeEventListener() {} }));
  }
  function transitions() {
    let pending: (() => void) | null = null;
    (document as unknown as { startViewTransition: unknown }).startViewTransition = (update: () => void) => {
      pending = update;
      return { finished: new Promise(() => {}), ready: new Promise(() => {}), skipTransition() { pending?.(); pending = null; } };
    };
    return () => { pending?.(); pending = null; };
  }
  afterEach(() => { delete (document as unknown as { startViewTransition?: unknown }).startViewTransition; });

  it('is ignored, and a press without a release leaves the next tap with one effect', async () => {
    narrow();
    const run = transitions();
    const calls = mockApi({ me: owner });
    await renderApp();
    await userEvent.click(tile('part-b'));
    // A pan that starts on the transition's picture: the browser sends pointercancel, no pointerup.
    act(() => { fireEvent.pointerDown(document.documentElement, { pointerType: 'touch' }); fireEvent.pointerCancel(document.documentElement, { pointerType: 'touch' }); });
    act(() => run());
    await userEvent.click(rowButton('Create and delete a computer'));
    expect(rowButton('Create and delete a computer')).toHaveAttribute('aria-expanded', 'true');
    await userEvent.click(within(itemOf('Run a command')).getByRole('button', { name: /4 votes/ }));
    await waitFor(() => expect(posts(calls, 'vote')).toHaveLength(1));
    await wait(50);
    expect(posts(calls, 'vote')).toHaveLength(1);
  });

  it('never turns a release of another button into a click', async () => {
    narrow();
    const run = transitions();
    mockApi();
    await renderApp();
    await userEvent.click(tile('part-a'));
    act(() => run());
    const row = rowButton('The engine runs on its own');
    act(() => { fireEvent.pointerDown(document.documentElement, { button: 2 }); fireEvent.pointerUp(row, { button: 2 }); });
    await wait(30);
    expect(row).toHaveAttribute('aria-expanded', 'false');
  });
});

describe('the blind between the plain view and a place', () => {
  /** A phone that wants motion: the tiles, never the map. */
  function narrow() {
    vi.stubGlobal('matchMedia', (query: string) => ({ matches: false, media: query, addEventListener() {}, removeEventListener() {} }));
  }
  /** View transitions whose update and end the test runs, with what the page had scrolled when each update ran. */
  function transitions() {
    const started: { update: () => void; end: () => void; scrolls: unknown[]; skipped: boolean }[] = [];
    const scrolls: unknown[] = [];
    vi.spyOn(window, 'scrollTo').mockImplementation(((options: unknown) => { scrolls.push(options); }) as typeof window.scrollTo);
    (document as unknown as { startViewTransition: unknown }).startViewTransition = (update: () => void) => {
      let end: () => void = () => {};
      const finished = new Promise<void>((resolve) => { end = resolve; });
      const entry = { update: () => { const before = scrolls.length; update(); entry.scrolls = scrolls.slice(before); }, end: () => act(() => end()), scrolls: [] as unknown[], skipped: false };
      started.push(entry);
      // As the browser does: a transition skipped runs its update if it has not yet, and ends.
      return { finished, ready: Promise.resolve(), skipTransition() { entry.skipped = true; entry.update(); end(); } };
    };
    return started;
  }
  const marks = () => [...document.documentElement.classList].filter((name) => name.startsWith('vt-'));
  function scrolledTo(y: number) {
    Object.defineProperty(window, 'scrollY', { value: y, configurable: true });
  }
  afterEach(() => {
    delete (document as unknown as { startViewTransition?: unknown }).startViewTransition;
    restoreScrollY();
    document.documentElement.classList.remove('vt-open', 'vt-back');
  });

  it('runs only from or to the plain view, with the way marked on <html> until it ends', async () => {
    narrow();
    const started = transitions();
    mockApi();
    await renderApp();
    await userEvent.click(tile('part-b'));
    expect(started).toHaveLength(1);
    expect(marks()).toEqual(['vt-open']);
    act(() => started[0].update());
    expect(shownPart()).toBe('part-b');
    expect(marks()).toEqual(['vt-open']);
    await started[0].end();
    await waitFor(() => expect(marks()).toEqual([]));
    // From one place to another: no view transition, the new place at once.
    await userEvent.click(tile('part-d'));
    expect(started).toHaveLength(1);
    expect(shownPart()).toBe('part-d');
    expect(marks()).toEqual([]);
    // Back to the plain view: the other way.
    await userEvent.click(document.querySelector<HTMLElement>('.ws-back')!);
    expect(started).toHaveLength(2);
    expect(marks()).toEqual(['vt-back']);
    act(() => started[1].update());
    expect(shownPart()).toBe('');
    await started[1].end();
    await waitFor(() => expect(marks()).toEqual([]));
  });

  it('takes the mark off when another move overtakes the transition, and leaves the next one its own', async () => {
    narrow();
    const started = transitions();
    mockApi();
    await renderApp();
    await userEvent.click(tile('part-b'));
    expect(marks()).toEqual(['vt-open']);
    // Back of the browser during the move: at once, no transition, no mark.
    act(() => { history.pushState(null, '', '/workshop#part-a'); dispatchEvent(new PopStateEvent('popstate')); });
    expect(marks()).toEqual([]);
    act(() => started[0].update());
    expect(shownPart()).toBe('part-a');
    await userEvent.click(document.querySelector<HTMLElement>('.ws-back')!);
    expect(marks()).toEqual(['vt-back']);
    // The overtaken transition ending takes nothing off the one under way.
    await started[0].end();
    expect(marks()).toEqual(['vt-back']);
  });

  it('sets the scroll at once inside the update: to the strip on opening, to the tile left on going back', async () => {
    narrow();
    const started = transitions();
    mockApi();
    await renderApp();
    scrolledTo(900);
    await userEvent.click(tile('part-b'));
    act(() => started[0].update());
    // Past the stop (jsdom lays nothing out: the stop is 0), the page goes to it in the update itself, at once.
    expect(started[0].scrolls).toEqual([{ top: 0, behavior: 'instant' }]);
    await started[0].end();
    scrolledTo(500);
    // The tile of the part left is 200 px above the window once back: the page scrolls up just that, at once.
    const plain = HTMLElement.prototype.getBoundingClientRect;
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
      if (this.matches('.ws-tile[data-part="part-b"]')) return { x: 0, y: -200, top: -200, left: 0, right: 200, bottom: -120, width: 200, height: 80, toJSON() {} } as DOMRect;
      return plain.call(this);
    });
    await userEvent.click(document.querySelector<HTMLElement>('.ws-back')!);
    act(() => started[1].update());
    expect(started[1].scrolls).toEqual([{ top: 300, behavior: 'instant' }]);
    expect(document.activeElement).toBe(tile('part-b'));
  });
});

describe('a view transition overtaken by another move', () => {
  function narrow() {
    vi.stubGlobal('matchMedia', (query: string) => ({ matches: false, media: query, addEventListener() {}, removeEventListener() {} }));
  }
  /** View transitions that wait for the test to run their update, and record being skipped. */
  function transitions() {
    const started: { update: () => void; end: () => void; skipped: boolean }[] = [];
    (document as unknown as { startViewTransition: unknown }).startViewTransition = (update: () => void) => {
      let end: () => void = () => {};
      const finished = new Promise<void>((resolve) => { end = resolve; });
      const entry = { update, end: () => end(), skipped: false };
      started.push(entry);
      return { finished, ready: Promise.resolve(), skipTransition() { entry.skipped = true; update(); end(); } };
    };
    return started;
  }
  afterEach(() => {
    delete (document as unknown as { startViewTransition?: unknown }).startViewTransition;
    document.documentElement.classList.remove('vt-open', 'vt-back');
  });

  it('is skipped when another place is opened from the strip before its update runs', async () => {
    narrow();
    const started = transitions();
    mockApi();
    history.replaceState(null, '', '/workshop#part-b');
    await renderApp();
    await userEvent.click(document.querySelector<HTMLElement>('.ws-back')!);
    expect(started).toHaveLength(1);
    await userEvent.click(tile('part-d'));
    expect(started[0].skipped).toBe(true);
    expect(shownPart()).toBe('part-d');
  });

  it('is skipped when the open chip is pressed again to keep the place', async () => {
    narrow();
    const started = transitions();
    mockApi();
    history.replaceState(null, '', '/workshop#part-b');
    await renderApp();
    await userEvent.click(document.querySelector<HTMLElement>('.ws-back')!);
    await userEvent.click(tile('part-b'));
    expect(started).toHaveLength(1);
    expect(started[0].skipped).toBe(true);
    expect(shownPart()).toBe('part-b');
  });

  it('is skipped by Back or Forward of the browser', async () => {
    narrow();
    const started = transitions();
    mockApi();
    await renderApp();
    await userEvent.click(tile('part-b'));
    act(() => { history.pushState(null, '', '/workshop#part-a'); dispatchEvent(new PopStateEvent('popstate')); });
    expect(started[0].skipped).toBe(true);
    expect(shownPart()).toBe('part-a');
  });

  it('is not skipped when it ends before the next move', async () => {
    narrow();
    const started = transitions();
    mockApi();
    await renderApp();
    await userEvent.click(tile('part-b'));
    act(() => started[0].update());
    await act(async () => started[0].end());
    // The next move, to another place: the transition that ended is left alone.
    await userEvent.click(tile('part-d'));
    expect(shownPart()).toBe('part-d');
    expect(started[0].skipped).toBe(false);
  });
});

describe('going back from a place reached by its address', () => {
  function narrow() {
    vi.stubGlobal('matchMedia', (query: string) => ({ matches: false, media: query, addEventListener() {}, removeEventListener() {} }));
  }
  afterEach(() => {
    delete (document as unknown as { startViewTransition?: unknown }).startViewTransition;
    document.documentElement.classList.remove('vt-open', 'vt-back');
  });

  it.each([['#latest', 'Latest'], ['#product', 'Product'], ['#left', /^Left out/], ['#suggested', 'Suggest a feature']] as const)('from %s gives the focus to the link of the title row that leads there, brought into view at once', async (hash, name) => {
    narrow();
    let update: () => void = () => {};
    (document as unknown as { startViewTransition: unknown }).startViewTransition = (run: () => void) => {
      update = run;
      return { finished: new Promise(() => {}), ready: Promise.resolve(), skipTransition() {} };
    };
    const scrolls: unknown[] = [];
    vi.spyOn(window, 'scrollTo').mockImplementation(((options: unknown) => { scrolls.push(options); }) as typeof window.scrollTo);
    mockApi();
    history.replaceState(null, '', `/workshop${hash}`);
    await renderApp();
    Object.defineProperty(window, 'scrollY', { value: 0, configurable: true });
    const plain = HTMLElement.prototype.getBoundingClientRect;
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
      if (this.matches('.ws-places a')) return { x: 0, y: 1200, top: 1200, left: 0, right: 80, bottom: 1220, width: 80, height: 20, toJSON() {} } as DOMRect;
      return plain.call(this);
    });
    await userEvent.click(document.querySelector<HTMLElement>('.ws-back')!);
    act(() => update());
    restoreScrollY();
    expect(document.activeElement).toBe(screen.getByRole('link', { name }));
    expect(scrolls).toEqual([{ top: 1220 - innerHeight, behavior: 'instant' }]);
  });

  it('gives the focus to the title when no link leads there', async () => {
    narrow();
    let update: () => void = () => {};
    (document as unknown as { startViewTransition: unknown }).startViewTransition = (run: () => void) => {
      update = run;
      return { finished: new Promise(() => {}), ready: Promise.resolve(), skipTransition() {} };
    };
    mockApi();
    history.replaceState(null, '', '/workshop#latest');
    await renderApp();
    await userEvent.click(document.querySelector<HTMLElement>('.ws-back')!);
    // The plain view the update draws has no link to Latest.
    const find = document.querySelector.bind(document);
    vi.spyOn(document, 'querySelector').mockImplementation(((selector: string) => (selector === '.ws-places a[href="#latest"]' ? null : find(selector))) as typeof document.querySelector);
    act(() => update());
    expect(document.activeElement).toBe(screen.getByRole('heading', { level: 1, name: 'Workshop' }));
  });
});

describe('going back off the map without a view transition', () => {
  afterEach(() => {
    delete (document as unknown as { startViewTransition?: unknown }).startViewTransition;
    restoreScrollY();
  });

  it.each([
    ['reduced motion', true],
    ['a browser without view transitions', false],
  ] as const)('with %s, brings what takes the focus into view at once', async (_, reduced) => {
    vi.stubGlobal('matchMedia', (query: string) => ({ matches: query.includes('prefers-reduced-motion') ? reduced : false, media: query, addEventListener() {}, removeEventListener() {} }));
    const started: unknown[] = [];
    if (reduced) (document as unknown as { startViewTransition: unknown }).startViewTransition = (update: () => void) => { started.push(update); update(); return { finished: Promise.resolve(), ready: Promise.resolve() }; };
    else delete (document as unknown as { startViewTransition?: unknown }).startViewTransition;
    const scrolls: unknown[] = [];
    vi.spyOn(window, 'scrollTo').mockImplementation(((options: unknown) => { scrolls.push(options); }) as typeof window.scrollTo);
    mockApi();
    await renderApp();
    // The tile of the part left is 200 px above the window once back; the link to Latest is 450 px below it.
    const plain = HTMLElement.prototype.getBoundingClientRect;
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
      if (this.matches('.ws-tile[data-part="part-b"]')) return { x: 0, y: -200, top: -200, left: 0, right: 200, bottom: -120, width: 200, height: 80, toJSON() {} } as DOMRect;
      if (this.matches('.ws-places a[href="#latest"]')) return { x: 0, y: 1200, top: 1200, left: 0, right: 80, bottom: 1218, width: 80, height: 18, toJSON() {} } as DOMRect;
      return plain.call(this);
    });
    await userEvent.click(tile('part-b'));
    Object.defineProperty(window, 'scrollY', { value: 500, configurable: true });
    scrolls.length = 0;
    await userEvent.click(document.querySelector<HTMLElement>('.ws-back')!);
    expect(shownPart()).toBe('');
    expect(document.activeElement).toBe(tile('part-b'));
    expect(scrolls).toEqual([{ top: 300, behavior: 'instant' }]);
    // A place reached by its address: the link of the title row that leads to it.
    history.replaceState(null, '', '/workshop#latest');
    act(() => { dispatchEvent(new PopStateEvent('popstate')); });
    expect(shownPart()).not.toBe('');
    scrolls.length = 0;
    await userEvent.click(document.querySelector<HTMLElement>('.ws-back')!);
    expect(document.activeElement).toBe(screen.getByRole('link', { name: 'Latest' }));
    expect(scrolls).toEqual([{ top: 500 + 1218 - innerHeight, behavior: 'instant' }]);
    expect(started).toEqual([]);
  });
});

describe('Map and Order with a place open', () => {
  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('swaps the strip alone: the place stays in sight and slides from where it was to its new height', async () => {
    screenOf({ motion: true, wide: true, mode: 'absent' });
    await mount();
    act(() => { fireEvent.click(part('part-b')); });
    await elapse(600);
    // The folded map is 93 px taller than the strip of tiles: the place is that much lower in Map.
    const plain = HTMLElement.prototype.getBoundingClientRect;
    vi.spyOn(HTMLElement.prototype, 'getBoundingClientRect').mockImplementation(function (this: HTMLElement) {
      if (!this.matches('.ws-detail')) return plain.call(this);
      const shift = Number(this.style.transform.match(/translateY\((-?[\d.]+)px\)/)?.[1] ?? 0);
      const top = (rootOf().classList.contains('v-map') ? 353 : 260) + shift;
      return { x: 0, y: top, top, left: 0, right: 1000, bottom: top + 900, width: 1000, height: 900, toJSON() {} } as DOMRect;
    });
    const detail = document.querySelector<HTMLElement>('.ws-detail')!;
    const styles: string[] = [];
    const observer = new MutationObserver((records) => records.forEach((record) => styles.push(record.oldValue ?? '')));
    observer.observe(detail, { attributes: true, attributeFilter: ['style'], attributeOldValue: true });
    act(() => { fireEvent.click(screen.getByRole('button', { name: 'Order' })); });
    // The strip fades out; the place is not part of the swap.
    expect(classesOf()).toContain('strip-out');
    expect(classesOf()).not.toContain('swap-out');
    await elapse(130);
    expect(shownView()).toBe('order');
    expect(classesOf()).toContain('strip-in');
    expect(document.querySelector('.ws-detail')).toBe(detail);
    await act(async () => { await Promise.resolve(); });
    // It started 93 px lower, where it was, and slides to its place.
    expect([...styles, detail.getAttribute('style') ?? '']).toContainEqual(expect.stringContaining('translateY(93px)'));
    await elapse(300);
    expect(classesOf()).not.toContain('strip-in');
    expect(detail.style.transform).toBe('');
    expect(detail.style.transition).toBe('');
    // Pressed again in the middle of the slide, the swap back starts from where the place is.
    styles.length = 0;
    act(() => { fireEvent.click(screen.getByRole('button', { name: 'Map' })); });
    await elapse(130);
    act(() => { fireEvent.click(screen.getByRole('button', { name: 'Order' })); });
    await elapse(130);
    await elapse(400);
    expect(shownView()).toBe('order');
    expect(location.search).toBe('?view=order');
    expect(detail.style.transform).toBe('');
    expect(classesOf().filter((name) => /^(strip|swap)-/.test(name))).toEqual([]);
    observer.disconnect();
  });

  it('keeps the whole swap on the plain view', async () => {
    screenOf({ motion: true, wide: true, mode: 'absent' });
    await mount();
    act(() => { fireEvent.click(screen.getByRole('button', { name: 'Order' })); });
    expect(classesOf()).toContain('swap-out');
    expect(classesOf()).not.toContain('strip-out');
    await elapse(130);
    expect(classesOf()).toContain('swap-in');
  });
});

describe('going back from a scrolled part on the map', () => {
  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    vi.restoreAllMocks();
    restoreScrollY();
  });

  it('keeps the map pinned until the scroll has brought its place below the header, and never on the plain view after', async () => {
    screenOf({ motion: true, wide: true, mode: 'absent' });
    vi.spyOn(window, 'scrollTo').mockImplementation((() => {}) as typeof window.scrollTo);
    await mount();
    act(() => { fireEvent.click(part('part-b')); });
    await elapse(600);
    Object.defineProperty(window, 'scrollY', { value: 500, configurable: true });
    act(() => { fireEvent.click(document.querySelector<HTMLElement>('.ws-back')!); });
    await elapse(200);
    expect(shownPart()).toBe('');
    expect(classesOf()).toContain('ws-pinned');
    // The page is still scrolled past the map's place (here: the title row's bottom, 0 px in this document).
    Object.defineProperty(window, 'scrollY', { value: 40, configurable: true });
    act(() => { dispatchEvent(new Event('scroll')); });
    expect(classesOf()).toContain('ws-pinned');
    Object.defineProperty(window, 'scrollY', { value: 0, configurable: true });
    act(() => { dispatchEvent(new Event('scroll')); });
    expect(classesOf()).not.toContain('ws-pinned');
  });

  it('lets the map go after a while even if the scroll never gets there, and does not pin it from the top', async () => {
    screenOf({ motion: true, wide: true, mode: 'absent' });
    vi.spyOn(window, 'scrollTo').mockImplementation((() => {}) as typeof window.scrollTo);
    await mount();
    act(() => { fireEvent.click(part('part-b')); });
    await elapse(600);
    Object.defineProperty(window, 'scrollY', { value: 500, configurable: true });
    act(() => { fireEvent.click(document.querySelector<HTMLElement>('.ws-back')!); });
    await elapse(1600);
    expect(classesOf()).not.toContain('ws-pinned');
    // From the top of the page, nothing needs pinning.
    act(() => { fireEvent.click(part('part-b')); });
    await elapse(600);
    Object.defineProperty(window, 'scrollY', { value: 0, configurable: true });
    act(() => { fireEvent.click(document.querySelector<HTMLElement>('.ws-back')!); });
    expect(classesOf()).not.toContain('ws-pinned');
  });
});

describe('a place that opens with a row open', () => {
  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('fades in where it stands; a place opened alone rises in, and the first place drawn does neither', async () => {
    screenOf({ motion: true, wide: true, mode: 'absent' });
    await mount();
    act(() => { fireEvent.click(part('part-b')); });
    expect(document.querySelector('.ws-detail')?.className).toBe('ws-detail');
    act(() => { history.pushState(null, '', '/workshop#part-d/fork'); dispatchEvent(new PopStateEvent('popstate')); });
    await elapse(10);
    expect(document.querySelector('.ws-detail')?.className).toBe('ws-detail ws-fade');
    // Closing the row later does not change its entrance.
    act(() => { fireEvent.click(rowButton('Fork a computer')); });
    expect(document.querySelector('.ws-detail')?.className).toBe('ws-detail ws-fade');
    cleanup();
    vi.useRealTimers();
    history.replaceState(null, '', '/workshop#part-d/fork');
    vi.useFakeTimers();
    mockApi();
    render(<App data={buildData()} />);
    await elapse(600);
    expect(document.querySelector('.ws-detail')?.className).toBe('ws-detail ws-still');
  });
});
