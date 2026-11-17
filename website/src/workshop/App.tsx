// The Workshop: the drawing of Silo (tiles on narrow screens), an opened area under it, and its entries
// opening in place. Addresses follow routes.ts; the live state and every write live in useWorkshop.ts.
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { createPortal, flushSync } from 'react-dom';
import { loginUrl } from './api';
import { AreaPlace, LeftPlace } from './AreaView';
import { DecisionsPlace } from './Decisions';
import { LatestPlace } from './Latest';
import type { EntryContext } from './Entry';
import { MapView } from './MapView';
import { FOLD_HEIGHT, MAP_WIDTH } from './map';
import { awaitsVerdict, needsOwner, statusLine } from './plan';
import type { AreaState, BuildData, Notes, Row } from './plan';
import { formatHash, HOME, placeFrom, placeOf, resolveHash, rowsOf, samePlace } from './routes';
import type { Place } from './routes';
import { SuggestPlace } from './Suggest';
import { Tiles } from './Tiles';
import { OPEN_GUARD_MS, useWorkshop } from './useWorkshop';
import type { Nav } from './useWorkshop';

export { OPEN_GUARD_MS } from './useWorkshop';

export interface AppProps {
  data: BuildData;
  /** Loads the features' notes, a chunk apart: the first time a person reaches an entry's "Notes". */
  loadNotes?: () => Promise<Notes>;
  /** The element in the site header that holds sign-in and the signed-in login. */
  accountSlot?: Element | null;
}

const noNotes = () => Promise.resolve({});

type View = 'map' | 'order';
type Focus = { kind: 'title' } | { kind: 'area'; area: string } | { kind: 'selector'; selector: string } | { kind: 'row'; key: string; reveal?: boolean } | { kind: 'reveal'; key: string } | { kind: 'buttons'; key: string };

const LEAVE_MS = 140;
/** How long what just arrived is marked, for its fade in (about 150 ms). */
const ARRIVE_MS = 400;
const ACCOUNT_WIDTH = 'silo-workshop-account-width';
const SWAP_MS = 130;
/** How long the map takes to fold, and an entry to open. */
const FOLD_MS = 440;
const ROW_MS = 240;
const KEY: [AreaState, string, string][] = [
  ['done', 'ws-pip built', 'done'], ['now', 'ws-pip building', 'building now'], ['next', 'ws-pip next', 'next'],
  ['planned', 'ws-pip planned', 'planned'], ['open', 'ws-dash', 'not decided yet'],
];

function media(query: string, fallback: boolean): boolean {
  return typeof matchMedia === 'function' ? matchMedia(query).matches : fallback;
}
const still = () => media('(prefers-reduced-motion: reduce)', true);
/** The width from which the map shows; narrower, the tiles. */
const wide = () => media('(min-width: 1200px)', true);

/** A selector that finds the same control again after the page re-renders. */
function selectorOf(node: Element | null): string | null {
  if (!(node instanceof HTMLElement)) return null;
  const href = node.getAttribute('href');
  const name = [...node.classList][0];
  if (href && name) return `a.${name}[href="${href}"]`;
  const tile = node.closest<HTMLElement>('.ws-tile');
  if (tile?.dataset.area) return `.ws-tile[data-area="${tile.dataset.area}"] .ws-tile-hit`;
  return null;
}

/**
 * Entirely inside the part of the window a person sees: below the sticky header and the folded map when it is
 * stuck, above the bottom edge. An element without layout (no rendering engine) counts as seen.
 */
export function seen(node: Element): boolean {
  const box = node.getBoundingClientRect();
  if (box.width === 0 && box.height === 0) return true;
  let top = 0;
  for (const cover of document.querySelectorAll('.site-header, .ws-swap-map')) {
    if (getComputedStyle(cover).position !== 'sticky') continue;
    const area = cover.getBoundingClientRect();
    if (area.height > 0 && area.top <= box.top) top = Math.max(top, area.bottom);
  }
  return box.top >= top && box.bottom <= innerHeight;
}

/** A row's verdict buttons, where a person sees and presses them. */
const verdictButtons = (item: Element | null) => item?.querySelector('.ws-btn.go')?.closest('.ws-state') ?? null;

function isField(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName);
}

/**
 * True for a moment after `on` turns true, unless it was true in the first frames: what arrives then fades in.
 * Set before the browser paints, so nothing shows a frame at full opacity first.
 */
function useArrival(on: boolean, boot: boolean): boolean {
  const [arriving, setArriving] = useState(false);
  const was = useRef(on);
  const timer = useRef<ReturnType<typeof setTimeout>>(undefined);
  useLayoutEffect(() => {
    const flipped = on && !was.current;
    was.current = on;
    if (!flipped || boot) return;
    setArriving(true);
    clearTimeout(timer.current);
    timer.current = setTimeout(() => setArriving(false), ARRIVE_MS);
  }, [on, boot]);
  useEffect(() => () => clearTimeout(timer.current), []);
  return arriving;
}

/** An item of the key; one that appears after the first frames fades in. */
function KeyItem({ mark, label, boot }: { mark: string; label: string; boot: boolean }) {
  const [fade] = useState(!boot);
  return <li className={fade ? 'ws-in' : undefined}><i className={mark} />{label}</li>;
}

const initialView = (): View => (new URLSearchParams(location.search).get('view') === 'order' ? 'order' : 'map');
const address = (hash = '') => `${location.pathname}${location.search}${hash}`;

export function App({ data, loadNotes = noNotes, accountSlot }: AppProps) {
  const [place, setPlace] = useState<Place>(HOME);
  // Rows open independently; `openKey` is the one opened last: the address, Enter and the guard follow it.
  const [openKey, setOpenKey] = useState<string | null>(null);
  const [openKeys, setOpenKeys] = useState<ReadonlySet<string>>(new Set());
  const [openings, setOpenings] = useState(0);
  const [view, setViewState] = useState<View>(initialView);
  const [leaving, setLeaving] = useState(false);
  const [swap, setSwap] = useState<'out' | 'in' | null>(null);
  const [armedTick, setArmed] = useState(0);
  // While the fold drives the scroll, the folded map is not pinned yet: pinned, it would jump down first.
  const [lifting, setLifting] = useState(false);
  // While the page keeps its height after going back, the footer waits hidden; it fades in at its place.
  const [held, setHeld] = useState(false);
  // The first frames take the state as it is, without animating into it.
  const [boot, setBoot] = useState(true);
  const nav = useRef<Nav>({ openKey: null, openedAt: new Map(), advanceFrom: () => {}, reopen: () => {} });
  const w = useWorkshop(data, openKey, openKeys, openings, nav, loadNotes);
  const { plan, rowByKey, live, canDecide } = w;
  /** No state is shown and the first answer is still to come. */
  const holding = !live.state && live.load === 'pending';
  const focusNext = useRef<Focus | null>(null);
  const wantedHash = useRef<string | null>(null);
  /** A row named by the address while the live state loads; it opens once the state is there. */
  const heldOpen = useRef<string | null>(null);
  /** The control that opened the suggestions, the left-out list or the decisions, to return focus to. */
  const opener = useRef<string | null>(null);
  const foldedAt = useRef(0);
  /** The scroll driven by the fold, from a scrolled full map. */
  const lift = useRef<{ frame: number; from: number; full: number } | null>(null);
  /** A count of navigations, so a view transition's update that has been overtaken never applies. */
  const navigations = useRef(0);
  /** Clears the page's held height after the map unfolds; only the next going back or unmounting cancels it. */
  const heldHeight = useRef<ReturnType<typeof setTimeout>>(undefined);
  // Once who is signed in is known, the account slot takes the width of what it holds (see workshop.css).
  useEffect(() => {
    if (!accountSlot) return;
    if (w.me === undefined) accountSlot.removeAttribute('data-account');
    else accountSlot.setAttribute('data-account', w.me ? 'in' : 'out');
    if (w.me === undefined) accountSlot.setAttribute('aria-busy', 'true');
    else accountSlot.removeAttribute('aria-busy');
    // The width this person's login and "Sign out" take: applied now when it differs from the reserved one (the
    // slot moves once, as for a stale cookie), and kept for their next visit (a convenience only).
    if (!w.me || !media('(min-width: 481px)', false)) return;
    const login = accountSlot.querySelector<HTMLElement>('.ws-login');
    const out = accountSlot.querySelector<HTMLElement>('.ws-link');
    if (!login || !out) return;
    const width = Math.min(200, Math.ceil(login.scrollWidth + 12 + out.offsetWidth));
    if (Math.abs(width - accountSlot.getBoundingClientRect().width) > 1) document.documentElement.style.setProperty('--ws-account-w', `${width}px`);
    try {
      localStorage.setItem(ACCOUNT_WIDTH, String(width));
    } catch {
      // Without storage the slot keeps its default width.
    }
  }, [accountSlot, w.me]);

  const timers = useRef<ReturnType<typeof setTimeout>[]>([]);
  const swapTimers = useRef<ReturnType<typeof setTimeout>[]>([]);
  const mainRef = useRef<HTMLDivElement>(null);
  useEffect(() => () => {
    [...timers.current, ...swapTimers.current].forEach(clearTimeout);
    clearTimeout(heldHeight.current);
    if (lift.current) cancelAnimationFrame(lift.current.frame);
  }, []);
  useEffect(() => {
    let frame = requestAnimationFrame(() => { frame = requestAnimationFrame(() => setBoot(false)); });
    return () => cancelAnimationFrame(frame);
  }, []);
  const later = (fn: () => void, ms: number) => { timers.current.push(setTimeout(fn, ms)); };
  // What arrives after the first frames fades in where it stands: the numbers, the person, the owner's marks.
  const arrivedState = useArrival(w.liveOk, boot);
  const arrivedPerson = useArrival(w.me !== undefined, boot);
  const arrivedOwner = useArrival(canDecide, boot);
  useLayoutEffect(() => {
    accountSlot?.classList.toggle('ws-arr-p', arrivedPerson);
  }, [accountSlot, arrivedPerson]);

  // Visitors never see the decisions: the address shows them the plan.
  const shown: Place = place.kind === 'decisions' && !canDecide ? HOME : place;
  const isHome = shown.kind === 'home';
  const onMap = () => view === 'map' && wide();

  const waiting = useCallback((row: Row) => needsOwner(row) && row.issue !== null && Boolean(live.state?.issues[row.issue]), [live.state]);
  const toDecide = useMemo(() => new Map(canDecide ? plan.areas.map((group) => [group.area.id, group.rows.filter(waiting).length]) : []), [canDecide, plan, waiting]);
  const totalToDecide = [...toDecide.values()].reduce((sum, n) => sum + n, 0);
  const byBuildOrder = useMemo(() => plan.areas.flatMap((group) => group.rows).sort((a, b) => (a.n ?? 0) - (b.n ?? 0)), [plan]);
  const free = (row: Row) => waiting(row) && !w.pendingRef.current.has(row.issue as number);

  // Each open row's buttons look armed once its own guard has passed.
  useEffect(() => {
    const now = Date.now();
    const waits = [...nav.current.openedAt.values()].map((at) => at + OPEN_GUARD_MS - now).filter((ms) => ms > 0);
    if (!waits.length) return;
    const timer = setTimeout(() => setArmed((n) => n + 1), Math.min(...waits) + 10);
    return () => clearTimeout(timer);
  }, [openKeys, openings, armedTick]);
  const armed = (key: string) => {
    const at = nav.current.openedAt.get(key);
    return at !== undefined && Date.now() - at >= OPEN_GUARD_MS;
  };

  /** Moves to a place, with at most the one row open. */
  function settle(next: Place, key: string | null) {
    timers.current.forEach(clearTimeout);
    timers.current = [];
    heldOpen.current = null;
    setLeaving(false);
    setPlace(next);
    setOpenKey(key);
    setOpenKeys(key ? new Set([key]) : new Set());
    if (key) setOpenings((n) => n + 1);
    nav.current.openKey = key;
    nav.current.openedAt = new Map(key ? [[key, Date.now()]] : []);
    navigations.current += 1;
  }

  function address$(next: Place, key: string | null, push = false) {
    const hash = formatHash(next, key);
    if (hash === location.hash) return;
    if (push) history.pushState(null, '', address(hash));
    else history.replaceState(null, '', address(hash));
  }

  function go(next: Place, key: string | null, options: { push?: boolean; focus?: Focus | null; announce?: string } = {}) {
    address$(next, key, options.push);
    if (next.kind === 'area' && shown.kind === 'home') foldedAt.current = Date.now();
    settle(next, key);
    w.setRowNotice((current) => (current?.key === key ? current : null));
    if (options.announce !== undefined) w.setAnnounce(options.announce);
    focusNext.current = options.focus ?? null;
  }

  /** Runs a change of place; off the map, the tile that was tapped travels to its place (view transitions). */
  function travel(update: () => void, hero: string | null) {
    const start = (document as Document & { startViewTransition?: (cb: () => void) => { finished: Promise<void> } }).startViewTransition?.bind(document);
    if (onMap() || still() || !start) {
      update();
      return;
    }
    const tile = hero ? document.querySelector<HTMLElement>(`.ws-tile[data-area="${hero}"]`) : null;
    if (tile) tile.style.viewTransitionName = 'hero';
    // The update waits for the old view's picture; a navigation made meanwhile wins over it.
    const mine = ++navigations.current;
    const vt = start(() => { if (navigations.current === mine) flushSync(update); }) as { finished: Promise<void>; ready?: Promise<void> };
    vt.ready?.catch(() => {});
    vt.finished.catch(() => {}).finally(() => { if (tile) tile.style.viewTransitionName = ''; });
  }

  /** Opens a row: next to the open ones in this place, or alone where it lives. */
  function openRow(key: string | null, options: { focus?: boolean; reveal?: boolean; push?: boolean; closing?: string } = {}) {
    const row = key ? rowByKey.get(key) : undefined;
    if (!row) {
      if (options.closing) closeRow(options.closing);
      return;
    }
    const target = placeFrom(row, shown, plan);
    if (!samePlace(target, shown)) {
      w.fetchNow(row);
      go(target, row.key, { push: options.push !== false, announce: `${row.name}, open`, focus: options.focus ? { kind: 'row', key: row.key, reveal: options.reveal !== false } : null });
      return;
    }
    // Asked for in the same event as the opening, so a failed early fetch shows as loading, not as an error.
    w.fetchNow(row);
    address$(target, row.key);
    setOpenKeys((current) => {
      const next = new Set(current);
      if (options.closing) next.delete(options.closing);
      next.add(row.key);
      return next;
    });
    setOpenKey(row.key);
    setOpenings((n) => n + 1);
    nav.current.openKey = row.key;
    if (options.closing) nav.current.openedAt.delete(options.closing);
    nav.current.openedAt.set(row.key, Date.now());
    w.setRowNotice((current) => (current?.key === row.key ? current : null));
    w.setAnnounce(`${row.name}, open`);
    focusNext.current = options.focus ? { kind: 'row', key: row.key, reveal: options.reveal !== false } : null;
  }

  /** Closes one row and leaves the others as they are. */
  function closeRow(key: string) {
    setOpenKeys((current) => {
      const next = new Set(current);
      next.delete(key);
      return next;
    });
    nav.current.openedAt.delete(key);
    if (openKey === key || nav.current.openKey === key) {
      address$(shown, null);
      setOpenKey(null);
      nav.current.openKey = null;
    }
  }

  nav.current.advanceFrom = (row: Row) => {
    const group = rowsOf(plan, placeFrom(row, shown, plan));
    const at = group.findIndex((item) => item.key === row.key);
    const ordered = at === -1 ? group : [...group.slice(at + 1), ...group.slice(0, at)];
    // Another open entry here still waiting: it becomes the current one, with its own guard as it was.
    const waitingOpen = ordered.find((item) => item.key !== row.key && openKeys.has(item.key) && free(item));
    // It is treated as freshly opened: its guard starts again and its buttons are brought into view.
    if (waitingOpen) {
      closeRow(row.key);
      address$(shown, waitingOpen.key);
      setOpenKey(waitingOpen.key);
      setOpenings((n) => n + 1);
      nav.current.openKey = waitingOpen.key;
      nav.current.openedAt.set(waitingOpen.key, Date.now());
      focusNext.current = { kind: 'buttons', key: waitingOpen.key };
      w.setAnnounce(`${waitingOpen.name}, open`);
      return;
    }
    const fresh = (item: Row) => item.key !== row.key && free(item) && !openKeys.has(item.key);
    const next = ordered.find(fresh) ?? byBuildOrder.find(fresh);
    const take = document.getElementById(row.key)?.querySelector('.ws-btn.go')?.getBoundingClientRect().top;
    // With a mouse, the next Take comes under the pointer; on a touch screen, the row is only brought into view.
    const coarse = media('(pointer: coarse)', false);
    openRow(next?.key ?? null, { focus: true, reveal: coarse, push: false, closing: row.key });
    if (!coarse && next && take !== undefined && samePlace(placeFrom(next, shown, plan), shown)) {
      // Measured once the rows have finished opening and closing, then once more after the comments came in.
      const follow = () => {
        const now = document.getElementById(next.key)?.querySelector('.ws-btn.go')?.getBoundingClientRect().top;
        if (now !== undefined && Math.abs(now - take) > 1) scrollBy({ top: now - take, behavior: still() ? 'instant' : 'smooth' });
      };
      later(follow, ROW_MS);
      later(follow, ROW_MS + 420);
    }
  };
  nav.current.reopen = (row: Row) => openRow(row.key, { focus: true, push: false });

  function back() {
    if (isHome) return;
    const focus: Focus | null = shown.kind === 'area' ? { kind: 'area', area: shown.area } : opener.current ? { kind: 'selector', selector: opener.current } : shown.kind === 'suggested' ? { kind: 'area', area: 'suggested' } : null;
    const hero = shown.kind === 'area' ? shown.area : shown.kind === 'suggested' ? 'suggested' : null;
    const finish = () => go(HOME, null, { push: true, focus, announce: '' });
    if (still()) {
      finish();
      return;
    }
    if (!onMap()) {
      travel(finish, hero);
      return;
    }
    // The page keeps its height while the map unfolds, then scrolls to the top.
    const main = mainRef.current;
    if (main) {
      clearTimeout(heldHeight.current);
      main.style.minHeight = `${main.offsetHeight}px`;
      setHeld(true);
      heldHeight.current = setTimeout(() => {
        main.style.minHeight = '';
        setHeld(false);
      }, LEAVE_MS + 420);
    }
    if (scrollY > 0) scrollTo({ top: 0, behavior: 'smooth' });
    setLeaving(true);
    later(finish, LEAVE_MS);
  }

  function openPlace(next: Place) {
    if (samePlace(next, shown)) {
      back();
      return;
    }
    // From a scrolled full map, the fold drives the scroll back to the top: each frame the page is scrolled in
    // proportion to how far the map has folded, so every box travels one way during the fold.
    if (isHome && onMap() && scrollY > 0 && !still()) startLift();
    if (next.kind !== 'area') opener.current = selectorOf(document.activeElement);
    const title = next.kind === 'area' ? plan.areas.find((group) => group.area.id === next.area)?.area.title : null;
    const hero = next.kind === 'area' ? next.area : next.kind === 'suggested' ? 'suggested' : null;
    travel(() => go(next, null, { push: true, focus: { kind: 'title' }, announce: title ? `${title}, open` : '' }), hero);
  }

  function startLift() {
    const stage = document.querySelector<HTMLElement>('.ws-stage');
    if (!stage || lift.current) return;
    const full = stage.getBoundingClientRect().height;
    const folded = (stage.getBoundingClientRect().width * FOLD_HEIGHT) / MAP_WIDTH;
    const state = { frame: 0, from: scrollY, full };
    lift.current = state;
    setLifting(true);
    const started = Date.now();
    const step = () => {
      const node = document.querySelector<HTMLElement>('.ws-stage');
      const height = node?.getBoundingClientRect().height ?? folded;
      const progress = Math.min(1, Math.max(0, (state.full - height) / Math.max(1, state.full - folded)));
      scrollTo({ top: state.from * (1 - progress), behavior: 'instant' });
      // Done once folded, when the map unfolds again, or after twice the fold's time at most.
      if (progress >= 1 || !node?.classList.contains('fold') || Date.now() - started > 2 * FOLD_MS) {
        if (progress >= 1) scrollTo({ top: 0, behavior: 'instant' });
        lift.current = null;
        setLifting(false);
        return;
      }
      state.frame = requestAnimationFrame(step);
    };
    state.frame = requestAnimationFrame(step);
  }

  // Addresses: on load, on Back and Forward, and once rows that did not exist yet arrive.
  const follow = useCallback(() => {
    const resolved = resolveHash(location.hash, plan, rowByKey);
    if (!resolved) {
      wantedHash.current = location.hash;
      return;
    }
    wantedHash.current = null;
    const canonical = formatHash(resolved.place, resolved.key);
    if (location.hash && canonical !== location.hash) history.replaceState(history.state, '', address(canonical));
    // While the live state loads, the named row waits closed: it would grow when votes and comments arrive.
    const hold = resolved.key !== null && holdingRef.current;
    settle(resolved.place, hold ? null : resolved.key);
    if (hold) heldOpen.current = resolved.key;
    focusNext.current = resolved.key && !hold ? { kind: 'reveal', key: resolved.key } : null;
  }, [plan, rowByKey]);
  const holdingRef = useRef(holding);
  holdingRef.current = holding;
  const followRef = useRef(follow);
  followRef.current = follow;
  // The address is read before the first paint, so the first frame is already the place it names.
  useLayoutEffect(() => {
    followRef.current();
    const listener = () => followRef.current();
    addEventListener('popstate', listener);
    addEventListener('hashchange', listener);
    return () => {
      removeEventListener('popstate', listener);
      removeEventListener('hashchange', listener);
    };
  }, []);
  useEffect(() => {
    if (wantedHash.current === null) return;
    if (resolveHash(wantedHash.current, plan, rowByKey)) follow();
    else if (w.settled) {
      // A link to something that the state shown does not have falls back to the plan, once the load has ended
      // (the `fresh=1` ask included, when one was made: what was just suggested may be only in that answer).
      wantedHash.current = null;
      history.replaceState(history.state, '', address());
      settle(HOME, null);
    }
  }, [plan, rowByKey, w.settled, follow]);
  useEffect(() => {
    const key = heldOpen.current;
    if (holding || !key) return;
    heldOpen.current = null;
    if (!rowByKey.has(key)) return;
    nav.current.openedAt.set(key, Date.now());
    setOpenKeys((current) => new Set([...current, key]));
    // When the person opened another row meanwhile, that row stays the current one and keeps the address.
    if (nav.current.openKey !== null) return;
    setOpenKey(key);
    setOpenings((n) => n + 1);
    nav.current.openKey = key;
    focusNext.current = { kind: 'reveal', key };
  }, [holding, rowByKey]);
  // Once it is known that this person cannot decide, the decisions address is dropped.
  useEffect(() => {
    if (place.kind === 'decisions' && live.load !== 'pending' && w.meKnown && !canDecide) {
      history.replaceState(history.state, '', address());
      settle(HOME, null);
    }
  }, [place, live.load, w.meKnown, canDecide]);

  // Focus follows what the person asked for; a new area starts at its top, under the folded map.
  useLayoutEffect(() => {
    const target = focusNext.current;
    if (!target) return;
    focusNext.current = null;
    if (target.kind === 'buttons') {
      document.getElementById(target.key)?.querySelector<HTMLElement>('.ws-row')?.focus({ preventScroll: true });
      // Once the decided row above has closed, the buttons come fully into view if they are not already.
      later(() => {
        const buttons = verdictButtons(document.getElementById(target.key));
        if (buttons && !seen(buttons)) buttons.scrollIntoView?.({ block: 'center', behavior: still() ? 'instant' : 'smooth' });
      }, ROW_MS);
    } else if (target.kind === 'row' || target.kind === 'reveal') {
      const item = document.getElementById(target.key);
      if (target.kind === 'row') item?.querySelector<HTMLElement>('.ws-row')?.focus({ preventScroll: true });
      // While the map folds, the page under it moves: the row is brought into view once the fold is done.
      const wait = Math.max(0, foldedAt.current + FOLD_MS - Date.now());
      const reveal = () => document.getElementById(target.key)?.scrollIntoView?.({ block: 'nearest', behavior: still() ? 'instant' : 'smooth' });
      if (target.kind === 'row' && target.reveal === false) return;
      if (wait) later(reveal, wait);
      else reveal();
    } else if (target.kind === 'title') {
      const top = document.querySelector<HTMLElement>('.ws-top');
      const header = document.querySelector<HTMLElement>('.site-header');
      if (top && mainRef.current) {
        const stop = (onMap() ? top.offsetTop + top.offsetHeight : mainRef.current.offsetTop + top.offsetHeight) - (header?.offsetHeight ?? 0);
        if (scrollY > stop && !lift.current) scrollTo({ top: stop, behavior: still() ? 'instant' : 'smooth' });
      }
      document.getElementById('ws-area-title')?.focus({ preventScroll: true });
    } else {
      let selector: string;
      if (target.kind === 'selector') selector = target.selector;
      else selector = onMap() && target.area !== 'suggested' ? `.ws-part[data-part="${target.area}"]` : `.ws-tile[data-area="${target.area}"] .ws-tile-hit`;
      const node = document.querySelector<HTMLElement>(selector) ?? (target.kind === 'selector' ? document.querySelector<HTMLElement>('.ws-top h1') : null);
      if (node && !node.hasAttribute('tabindex') && node.tagName === 'H1') node.setAttribute('tabindex', '-1');
      node?.focus({ preventScroll: true });
    }
  });

  // The open tile of the strip stays in view on a phone.
  useEffect(() => {
    const board = document.querySelector<HTMLElement>('.ws-board');
    const on = board?.querySelector<HTMLElement>('.ws-tile.on');
    if (board && on && board.scrollWidth > board.clientWidth) board.scrollLeft = on.offsetLeft - (board.clientWidth - on.offsetWidth) / 2;
  }, [shown]);

  // The folded map's height, for the area's left column that sticks under it.
  useEffect(() => {
    const main = mainRef.current;
    if (!main || typeof ResizeObserver !== 'function') return;
    const observer = new ResizeObserver(() => main.style.setProperty('--ws-foldh', `${Math.round((main.clientWidth * FOLD_HEIGHT) / MAP_WIDTH)}px`));
    observer.observe(main);
    return () => observer.disconnect();
  }, []);

  // Map or Order. The swap runs on its own timers: a navigation in the middle never leaves it half done.
  function setView(next: View) {
    if (next === view) return;
    const apply = () => {
      setViewState(next);
      const params = new URLSearchParams(location.search);
      if (next === 'order') params.set('view', 'order');
      else params.delete('view');
      const search = params.toString();
      history.replaceState(history.state, '', `${location.pathname}${search ? `?${search}` : ''}${location.hash}`);
    };
    swapTimers.current.forEach(clearTimeout);
    if (still() || !wide()) {
      setSwap(null);
      apply();
      return;
    }
    setSwap('out');
    swapTimers.current = [setTimeout(() => {
      apply();
      setSwap('in');
      swapTimers.current.push(setTimeout(() => setSwap(null), 300));
    }, SWAP_MS)];
  }

  const mainAction = (row: Row) => {
    if (!w.verdictReady(row)) return;
    if (awaitsVerdict(row)) void w.sendVerdict(row, 'take');
  };

  const shownRows = rowsOf(plan, shown);
  const keyHandler = useRef<(event: KeyboardEvent) => void>(() => {});
  keyHandler.current = (event: KeyboardEvent) => {
    if (event.key === 'Escape' && !isHome && !isField(event.target)) {
      back();
      return;
    }
    if (event.key !== 'Enter' || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    if (!w.mayDecide || isField(event.target)) return;
    // One rule, whatever holds the focus: the open row that contains the focus, else the row opened last, takes
    // Enter only if it is armed, waits for a verdict and its verdict buttons are entirely where the person sees
    // them. Other buttons and links keep their own Enter.
    const target = event.target instanceof HTMLElement ? event.target : null;
    const control = target?.closest('button, a, summary');
    if (control && !control.classList.contains('ws-row')) return;
    // On a row's own header, that row is the only target: a closed one keeps the button's Enter, which opens it.
    // Only with the focus on no control at all does Enter go to the row opened last.
    const own = control ? control.closest<HTMLElement>('.ws-item') : null;
    if (control && !own?.classList.contains('open')) return;
    const item = own ?? target?.closest<HTMLElement>('.ws-item.open') ?? (openKey ? document.getElementById(openKey) : null);
    const row = item ? rowByKey.get(item.id) : undefined;
    const buttons = verdictButtons(item);
    if (!row || !needsOwner(row) || !openKeys.has(row.key) || !shownRows.includes(row) || !armed(row.key) || !buttons || !seen(buttons)) return;
    event.preventDefault();
    if (event.repeat) return;
    mainAction(row);
  };
  useEffect(() => {
    const listener = (event: KeyboardEvent) => keyHandler.current(event);
    document.addEventListener('keydown', listener);
    return () => document.removeEventListener('keydown', listener);
  }, []);

  const signIn = loginUrl(location.hash);
  const ctx: EntryContext = {
    w, signIn, mainAction, open: openKeys,
    onToggle: (row) => (openKeys.has(row.key) ? closeRow(row.key) : openRow(row.key)),
    armed: (row) => w.mayDecide && armed(row.key),
  };
  const account = w.me === undefined ? <span className="ws-ph ws-ph-account" aria-hidden="true" /> : w.me ? (
    <span className="ws-account">
      <span className="ws-login" title={w.me.login}>{w.me.login}</span>
      <button type="button" className="ws-link" onClick={() => void w.signOut()}>Sign out</button>
    </span>
  ) : (
    <span className="ws-signin-wrap">
      <a className="button small" href={signIn} aria-describedby="ws-signin-tip"><span>Sign in{' '}<span className="ws-wide">with GitHub</span></span></a>
      <span className="ws-signin-tip" id="ws-signin-tip" role="tooltip">or comment directly on GitHub</span>
    </span>
  );

  const notes = (
    <>
      {w.offline ? (
        <p className="ws-quiet ws-note">
          <span>Votes and comments are offline.</span>
          <button type="button" className="ws-link ws-retry" disabled={live.load === 'pending'} onClick={w.retry}>Try again</button>
        </p>
      ) : null}
      {w.signinFailed ? <p className="ws-quiet ws-note">Sign-in did not complete.</p> : null}
    </>
  );
  const present = new Set(plan.areas.map((group) => group.state));
  const suggested = plan.suggestions.filter((row) => row.status !== 'dropped').length;
  const current = shown.kind === 'area' ? shown.area : shown.kind === 'suggested' ? 'suggested' : null;
  const firstToDecide = byBuildOrder.find(free);
  const link = (target: Place) => (event: { preventDefault: () => void }) => {
    event.preventDefault();
    openPlace(target);
  };

  let detail = null;
  if (shown.kind === 'area') {
    const group = plan.areas.find((item) => item.area.id === shown.area);
    if (group) detail = <AreaPlace group={group} toDecide={toDecide.get(group.area.id) ?? 0} ctx={ctx} onBack={back} notes={notes} />;
  } else if (shown.kind === 'suggested') detail = <SuggestPlace ctx={ctx} onBack={back} notes={notes} />;
  else if (shown.kind === 'left') detail = <LeftPlace rows={plan.left} ctx={ctx} onBack={back} notes={notes} />;
  else if (shown.kind === 'decisions') detail = <DecisionsPlace ctx={ctx} onBack={back} notes={notes} />;
  else if (shown.kind === 'latest') detail = <LatestPlace ctx={ctx} onBack={back} notes={notes} />;

  const classes = ['ws', isHome ? 'is-sheet' : 'is-open', `v-${view}`, leaving ? 'leaving' : '', swap ? `swap-${swap}` : '', boot ? 'ws-boot' : '', canDecide ? 'ws-owner' : '', lifting ? 'ws-lifting' : '', held ? 'ws-held' : '',
    arrivedState ? 'ws-arr-s' : '', arrivedPerson ? 'ws-arr-p' : '', arrivedOwner ? 'ws-arr-o' : ''].filter(Boolean).join(' ');
  return (
    <>
      {accountSlot ? createPortal(account, accountSlot) : <div className="ws-account-inline">{account}</div>}
      <div className={classes} ref={mainRef} data-live={live.load}>
        <section className="ws-top joint">
          <div className="ws-head">
            <h1>Workshop</h1>
            <p className="ws-status">{statusLine(plan.areas)}</p>
          </div>
          <div className="ws-top-side">
            <div className="ws-tools">
              {canDecide && firstToDecide ? (
                <a className="ws-loglink ws-late-o" href={formatHash(placeOf(firstToDecide), firstToDecide.key)} onClick={(event) => { event.preventDefault(); openRow(firstToDecide.key, { focus: true }); }}>
                  <b>{totalToDecide} to decide</b>
                </a>
              ) : null}
              {canDecide ? (
                <a className="ws-loglink ws-late-o" href="#decisions" aria-current={shown.kind === 'decisions' ? 'page' : undefined} onClick={link({ kind: 'decisions' })}>
                  Decisions
                </a>
              ) : null}
              <div className="ws-seg" role="group" aria-label="Arrangement">
                <button type="button" aria-pressed={view === 'map'} onClick={() => setView('map')}>Map</button>
                <button type="button" aria-pressed={view === 'order'} onClick={() => setView('order')}>Order</button>
              </div>
            </div>
            <ul className="ws-key" aria-label="Key">
              {KEY.filter(([state]) => present.has(state)).map(([state, mark, label]) => <KeyItem key={state} mark={mark} label={label} boot={boot} />)}
            </ul>
          </div>
        </section>
        <div className="ws-swap ws-swap-map">
          <MapView groups={plan.areas} folded={!isHome && !leaving} current={current} toDecide={toDecide} onOpen={(area) => openPlace({ kind: 'area', area })} />
        </div>
        <div className="ws-swap ws-swap-order">
          <Tiles groups={plan.areas} current={current} toDecide={toDecide} suggested={suggested} onOpen={(area) => openPlace({ kind: 'area', area })} onSuggest={() => openPlace({ kind: 'suggested' })} />
        </div>
        {isHome ? (
          <div className="ws-under">
            <div className="ws-under-left">
              {plan.left.length ? <a className="ws-fold-link" href="#left" onClick={link({ kind: 'left' })}>Left out · {plan.left.length}</a> : null}
              <a className="ws-fold-link" href="#latest" onClick={link({ kind: 'latest' })}>Latest</a>
              {suggested ? <a className="ws-fold-link ws-late-s" href="#suggested" onClick={link({ kind: 'suggested' })}>{suggested} suggested</a> : null}
              {notes}
            </div>
            <a className="ws-btn ws-suggest" href="#suggested" onClick={link({ kind: 'suggested' })}>Suggest a feature</a>
          </div>
        ) : (
          <section className="ws-detail" key={formatHash(shown)} aria-labelledby="ws-area-title">{detail}</section>
        )}
        <p className="sr-only" aria-live="polite">{w.announce}</p>
        {w.notice ? <p className="ws-notice" role="alert">{w.notice}</p> : null}
      </div>
    </>
  );
}
