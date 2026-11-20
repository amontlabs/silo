// The Workshop: the drawing of Silo (tiles on narrow screens), an opened part under it, and its entries
// opening in place. Addresses follow routes.ts; the live state and every write live in useWorkshop.ts.
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import { createPortal, flushSync } from 'react-dom';
import { loginUrl } from './api';
import type { Me } from './api';
import { LeftPlace, PartPlace, ProductPlace } from './PartView';
import { LatestPlace } from './Latest';
import type { EntryContext } from './Entry';
import { MapView } from './MapView';
import { FOLD_HEIGHT, hasMap, MAP_WIDTH } from './map';
import { statusLine } from './plan';
import type { PartState, BuildData, Facts, Histories, Notes } from './plan';
import { formatHash, HOME, placeFrom, resolveHash, samePlace } from './routes';
import type { Place } from './routes';
import { SuggestPlace } from './Suggest';
import { Tiles } from './Tiles';
import { useWorkshop } from './useWorkshop';

export interface AppProps {
  data: BuildData;
  /** Loads the features' notes, a chunk apart: the first time a person reaches an entry's "Notes". */
  loadNotes?: () => Promise<Notes>;
  /** Loads the statements in force and what the entries say of them, a chunk apart: the first time a row with something to show is reached. */
  loadFacts?: () => Promise<Facts>;
  /** Loads the changes of every entry, a chunk apart: the first time a person opens a History, or reaches Latest. */
  loadHistory?: () => Promise<Histories>;
  /**
   * The element in the site header that holds sign-in and the signed-in login; without one, they are drawn inline.
   * `false` when the build draws them into the header itself (see prerender.tsx).
   */
  accountSlot?: Element | null | false;
}

/** Sign-in with its tooltip that points to GitHub, the signed-in login with "Sign out", or a placeholder while it is not known who is signed in. */
export function Account({ me, signIn, onSignOut }: { me: Me | null | undefined; signIn: string; onSignOut?: () => void }) {
  // Each its own node (keys), so what replaces the placeholder arrives in place instead of the same node moving.
  if (me === undefined) return <span key="placeholder" className="ws-ph ws-ph-account" aria-hidden="true" />;
  if (me) {
    return (
      <span key="account" className="ws-account">
        <span className="ws-login" title={me.login}>{me.login}</span>
        <button type="button" className="ws-link" onClick={onSignOut}>Sign out</button>
      </span>
    );
  }
  return <SignIn key="signin" href={signIn} />;
}

const noNotes = () => Promise.resolve({});
const noFacts = () => Promise.resolve({ entries: {}, statements: {} });
const noHistory = () => Promise.resolve({});

type View = 'map' | 'order';
/** What the page uses of the browser's `ViewTransition`. */
type ViewTransitionLike = { finished: Promise<void>; ready?: Promise<void>; skipTransition?: () => void };
/** `bring`: what takes the focus is brought into view at once (going back off the map). */
type Focus = { kind: 'title' } | { kind: 'part'; part: string; bring?: boolean } | { kind: 'selector'; selector: string; bring?: boolean } | { kind: 'row'; key: string; reveal?: boolean } | { kind: 'reveal'; key: string };

const LEAVE_MS = 140;
/** How long what just arrived is marked, for its fade in (about 150 ms). */
const ARRIVE_MS = 400;
const ACCOUNT_WIDTH = 'silo-workshop-account-width';
const SWAP_MS = 130;
/** How long the map takes to fold, and an entry to open. */
const FOLD_MS = 440;
const KEY: [PartState, string, string][] = [
  ['done', 'ws-pip built', 'done'], ['next', 'ws-pip next', 'next'],
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
  if (tile?.dataset.part) return `.ws-tile[data-part="${tile.dataset.part}"] .ws-tile-hit`;
  return null;
}

/** Scrolls the page the least needed for the element to be in view under the sticky header, at once. */
function reveal(node: Element) {
  const header = document.querySelector<HTMLElement>('.site-header')?.offsetHeight ?? 0;
  const box = node.getBoundingClientRect();
  const room = innerHeight - header;
  let by = 0;
  if (box.top < header || box.height > room) by = box.top - header;
  else if (box.bottom > innerHeight) by = box.bottom - innerHeight;
  if (by) scrollTo({ top: scrollY + by, behavior: 'instant' });
}

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

/** The line under the title; a line that appears or changes after the first frames fades in. */
function Status({ text, boot }: { text: string; boot: boolean }) {
  const [fade] = useState(!boot);
  return <p className={fade ? 'ws-status ws-in' : 'ws-status'}>{text}</p>;
}

const SignInLabel = () => <span>Sign in{' '}<span className="ws-wide">with GitHub</span></span>;

/**
 * "Sign in with GitHub", and under it a tooltip for people who will not sign in here. It stays while the pointer is
 * over it; Escape closes it until the pointer or the focus leaves the button.
 */
function SignIn({ href }: { href: string }) {
  const [closed, setClosed] = useState(false);
  const [over, setOver] = useState(false);
  const [focused, setFocused] = useState(false);
  useEffect(() => {
    if (!over && !focused) {
      setClosed(false);
      return;
    }
    if (closed) return;
    const listener = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return;
      event.stopPropagation();
      setClosed(true);
    };
    document.addEventListener('keydown', listener, true);
    return () => document.removeEventListener('keydown', listener, true);
  }, [over, focused, closed]);
  return (
    <span className={closed ? 'ws-signin-wrap closed' : 'ws-signin-wrap'} onPointerEnter={() => setOver(true)} onPointerLeave={() => setOver(false)}>
      <a className="button small" href={href} aria-describedby="ws-signin-tip" onFocus={() => setFocused(true)} onBlur={() => setFocused(false)}>
        <SignInLabel />
      </a>
      <span className="ws-signin-tip" id="ws-signin-tip" role="tooltip">or comment directly on GitHub</span>
    </span>
  );
}

// At build time there is no address: the plain view. Without a map, the tiles are all there is.
const viewFrom = (map: boolean): View => (!map || (typeof location !== 'undefined' && new URLSearchParams(location.search).get('view') === 'order') ? 'order' : 'map');
const address = (hash = '') => `${location.pathname}${location.search}${hash}`;

export function App({ data, loadNotes = noNotes, loadFacts = noFacts, loadHistory = noHistory, accountSlot }: AppProps) {
  const mapped = hasMap(data.map);
  const initialView = () => viewFrom(mapped);
  const [place, setPlace] = useState<Place>(HOME);
  // Rows open independently; `openKey` is the one opened last: the address, Enter and the guard follow it.
  const [openKey, setOpenKey] = useState<string | null>(null);
  const [openKeys, setOpenKeys] = useState<ReadonlySet<string>>(new Set());
  const [openings, setOpenings] = useState(0);
  const [view, setViewState] = useState<View>(initialView);
  const [leaving, setLeaving] = useState(false);
  const [swap, setSwap] = useState<'out' | 'in' | null>(null);
  // While the fold drives the scroll, the folded map is not pinned yet: pinned, it would jump down first.
  const [lifting, setLifting] = useState(false);
  // While the page keeps its height after going back, the footer waits hidden; it fades in at its place.
  const [held, setHeld] = useState(false);
  // The first frames take the state as it is, without animating into it.
  const [boot, setBoot] = useState(true);
  // The row opened last, read by handlers that run after a render.
  const openKeyRef = useRef<string | null>(null);
  const w = useWorkshop(data, openKey, openKeys, openings, loadNotes, loadFacts, loadHistory);
  const { plan, rowByKey, live } = w;
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
  /** Clears the page's held height after the map unfolds; the next move or unmounting releases it sooner. */
  const heldHeight = useRef<ReturnType<typeof setTimeout>>(undefined);
  /** The count of the move that is a going-back; while no move came after it, that going-back is under way. */
  const goingBack = useRef(-1);
  /** The control to focus once Back or Forward has shown the other view, when the focus was in the one hidden. */
  const viewFocus = useRef<string | null>(null);
  // A link under the map that held the focus when Back or Forward moved: if the move takes it off the page, the title
  // takes the focus.
  const underFocus = useRef<Element | null>(null);
  /** The move whose view transition set the mark on `<html>` (`vt-open`, `vt-back`); -1 when none is set. */
  const markOwner = useRef(-1);
  /** True while a view transition's update runs: what it scrolls is set at once, before the new view is captured. */
  const inTransition = useRef(false);
  /** The browser's view transition of the last move that started one, until it ends or another move starts. */
  const transition = useRef<ViewTransitionLike | null>(null);
  // Once who is signed in is known, the account slot takes the width of what it holds (see workshop.css).
  useEffect(() => {
    if (!accountSlot) return;
    if (w.me === undefined) accountSlot.removeAttribute('data-account');
    else accountSlot.setAttribute('data-account', w.me ? 'in' : 'out');
    if (w.me === undefined) accountSlot.setAttribute('aria-busy', 'true');
    else accountSlot.removeAttribute('aria-busy');
    // The width this person's login and "Sign out" take, kept for their next visit, where the head script applies
    // it before first paint (a convenience only). It is not applied now: once the page has painted, the slot keeps
    // its width for a signed-in person, and the login sits right-aligned in it (truncated if it is longer).
    if (!w.me || !media('(min-width: 481px)', false)) return;
    const login = accountSlot.querySelector<HTMLElement>('.ws-login');
    const out = accountSlot.querySelector<HTMLElement>('.ws-link');
    if (!login || !out) return;
    const width = Math.min(200, Math.ceil(login.scrollWidth + 12 + out.offsetWidth));
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
    clearMark();
  }, []);
  useEffect(() => {
    let frame = requestAnimationFrame(() => { frame = requestAnimationFrame(() => setBoot(false)); });
    return () => cancelAnimationFrame(frame);
  }, []);
  const later = (fn: () => void, ms: number) => { timers.current.push(setTimeout(fn, ms)); };
  // What arrives after the first frames fades in where it stands: the numbers and the person.
  const arrivedState = useArrival(w.liveOk, boot);
  const arrivedPerson = useArrival(w.me !== undefined, boot);
  useLayoutEffect(() => {
    if (accountSlot) accountSlot.classList.toggle('ws-arr-p', arrivedPerson);
  }, [accountSlot, arrivedPerson]);

  const shown: Place = place;
  const isHome = shown.kind === 'home';
  const onMap = () => view === 'map' && wide();

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
    openKeyRef.current = key;
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
    if (next.kind === 'part' && shown.kind === 'home') foldedAt.current = Date.now();
    settle(next, key);
    if (options.announce !== undefined) w.setAnnounce(options.announce);
    focusNext.current = options.focus ?? null;
  }

  /** Takes off `<html>` the mark of the way a view transition goes. */
  function clearMark() {
    document.documentElement.classList.remove('vt-open', 'vt-back');
    markOwner.current = -1;
  }

  /**
   * A new move starts: what an earlier one left waiting (the end of going back, a view transition's update) never
   * lands after it, its view transition ends at once with its mark, so no picture of it covers this move, and the
   * height a going-back kept is let go at once. Returns this move's count.
   */
  function moveStarts(): number {
    // Counted first: the update of the transition skipped here, if the browser runs it now, finds it was overtaken.
    const mine = ++navigations.current;
    const overtaken = transition.current;
    transition.current = null;
    overtaken?.skipTransition?.();
    clearMark();
    timers.current.forEach(clearTimeout);
    timers.current = [];
    clearTimeout(heldHeight.current);
    if (mainRef.current) mainRef.current.style.minHeight = '';
    setHeld(false);
    setLeaving(false);
    return mine;
  }

  /** True while a going-back has started and neither landed nor been overtaken by another move. */
  const backUnderWay = () => goingBack.current === navigations.current;

  /**
   * Runs a change of place. Off the map, a move from or to the plain view (`way`) is a view transition: the sheet of
   * tiles rolls up into the strip over the place, or down again (workshop.css). `<html>` carries the way, `vt-open`
   * or `vt-back`, until the transition ends or another move starts. A move between two places has no `way`.
   */
  function travel(update: () => void, way: 'open' | 'back' | null): number {
    const mine = moveStarts();
    const start = (document as Document & { startViewTransition?: (cb: () => void) => ViewTransitionLike }).startViewTransition?.bind(document);
    if (!way || onMap() || still() || !start) {
      update();
      return mine;
    }
    document.documentElement.classList.add(`vt-${way}`);
    markOwner.current = mine;
    // The update waits for the old view's picture; a navigation made meanwhile wins over it. What it scrolls is set
    // in the same task (layout effects), so the new view is captured where it lands.
    const vt = start(() => {
      if (navigations.current !== mine) return;
      inTransition.current = true;
      try {
        flushSync(update);
      } finally {
        inTransition.current = false;
      }
    });
    transition.current = vt;
    vt.ready?.catch(() => {});
    vt.finished.catch(() => {}).finally(() => {
      if (transition.current === vt) transition.current = null;
      if (markOwner.current === mine) clearMark();
    });
    return mine;
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
    openKeyRef.current = row.key;
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
    if (openKey === key || openKeyRef.current === key) {
      address$(shown, null);
      setOpenKey(null);
      openKeyRef.current = null;
    }
  }

  function back() {
    // A going-back under way lands at its time: a second one (the button pressed twice, Escape again) changes nothing.
    if (isHome || backUnderWay()) return;
    // A place reached by its address has no opener: the link under the tiles that leads to it takes the focus, or the
    // page's title when there is none.
    // Off the map, with a view transition or without one, it is brought into view; the map has its own going-back.
    const bring = !onMap();
    const focus: Focus = shown.kind === 'part' ? { kind: 'part', part: shown.part, bring } : opener.current ? { kind: 'selector', selector: opener.current, bring } : shown.kind === 'suggested' ? { kind: 'part', part: 'suggested', bring } : { kind: 'selector', selector: `a.ws-fold-link[href="${formatHash(shown)}"]`, bring };
    const finish = () => go(HOME, null, { push: true, focus, announce: '' });
    if (still()) {
      finish();
      return;
    }
    if (!onMap()) {
      goingBack.current = travel(finish, 'back');
      return;
    }
    goingBack.current = moveStarts();
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
      // While going back from this place, its part or tile is a new move that keeps it open: the going-back is dropped.
      if (backUnderWay()) moveStarts();
      else back();
      return;
    }
    // From a scrolled full map, the fold drives the scroll back to the top: each frame the page is scrolled in
    // proportion to how far the map has folded, so every box travels one way during the fold.
    if (isHome && onMap() && scrollY > 0 && !still()) startLift();
    if (next.kind !== 'part') opener.current = selectorOf(document.activeElement);
    const title = next.kind === 'part' ? plan.parts.find((group) => group.part.id === next.part)?.part.title : null;
    travel(() => go(next, null, { push: true, focus: { kind: 'title' }, announce: title ? `${title}, open` : '' }), isHome ? 'open' : null);
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
  const moveStartsRef = useRef(moveStarts);
  moveStartsRef.current = moveStarts;
  const viewRef = useRef(view);
  viewRef.current = view;
  // The address is read before the first paint, so the first frame is already the place it names. On Back, Forward
  // and an address followed, the view is the one the address names too, as on a fresh load: a swap still under way
  // is dropped, and the view is set without one. It is a new move, even when the place it names is not known yet.
  useLayoutEffect(() => {
    followRef.current();
    const listener = () => {
      moveStartsRef.current();
      swapTimers.current.forEach(clearTimeout);
      swapTimers.current = [];
      setSwap(null);
      const next = initialView();
      // On a wide screen the other view's panel is taken out of sight, and Order also hides "Suggest a feature" under
      // the map: the focus on what gets hidden goes to the same part in the view now shown, else to the title, rather
      // than to the page itself.
      const hidden = document.querySelector(`.ws-swap-${viewRef.current}`);
      const focused = document.activeElement;
      const hides = !!focused && (!!hidden?.contains(focused) || (next === 'order' && focused.matches('.ws-under .ws-suggest')));
      underFocus.current = focused?.closest('.ws-under') ? focused : null;
      if (next !== viewRef.current && wide() && focused && hides) {
        const part = focused.closest<HTMLElement>('[data-part]');
        const id = part?.dataset.part;
        viewFocus.current = !id ? '' : next === 'map' ? `.ws-part[data-part="${id}"]` : `.ws-tile[data-part="${id}"] .ws-tile-hit`;
      }
      setViewState(next);
      followRef.current();
    };
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
    setOpenKeys((current) => new Set([...current, key]));
    // When the person opened another row meanwhile, that row stays the current one and keeps the address.
    if (openKeyRef.current !== null) return;
    setOpenKey(key);
    setOpenings((n) => n + 1);
    openKeyRef.current = key;
    focusNext.current = { kind: 'reveal', key };
  }, [holding, rowByKey]);
  // After Back or Forward showed the other view, the focus that was in the hidden one lands in the one shown; a link
  // under the map that the move took off the page hands the focus to the title.
  useLayoutEffect(() => {
    const under = underFocus.current;
    underFocus.current = null;
    if (under && !under.isConnected && viewFocus.current === null) viewFocus.current = '';
    const selector = viewFocus.current;
    if (selector === null) return;
    viewFocus.current = null;
    const node = (selector && document.querySelector<HTMLElement>(selector)) || document.querySelector<HTMLElement>('.ws-top h1');
    if (node?.tagName === 'H1' && !node.hasAttribute('tabindex')) node.setAttribute('tabindex', '-1');
    node?.focus({ preventScroll: true });
  });

  // Focus follows what the person asked for; a new part starts at its top, under the folded map.
  useLayoutEffect(() => {
    const target = focusNext.current;
    if (!target) return;
    focusNext.current = null;
    if (target.kind === 'row' || target.kind === 'reveal') {
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
        if (scrollY > stop && !lift.current) scrollTo({ top: stop, behavior: still() || inTransition.current ? 'instant' : 'smooth' });
      }
      document.getElementById('ws-part-title')?.focus({ preventScroll: true });
    } else {
      let selector: string;
      if (target.kind === 'selector') selector = target.selector;
      else selector = onMap() && target.part !== 'suggested' ? `.ws-part[data-part="${target.part}"]` : `.ws-tile[data-part="${target.part}"] .ws-tile-hit`;
      const node = document.querySelector<HTMLElement>(selector) ?? (target.kind === 'selector' ? document.querySelector<HTMLElement>('.ws-top h1') : null);
      if (node && !node.hasAttribute('tabindex') && node.tagName === 'H1') node.setAttribute('tabindex', '-1');
      node?.focus({ preventScroll: true });
      // Back on the plain view off the map, what takes the focus is brought into view at once, scrolled the least
      // needed and clear of the sticky header (inside a view transition, before the new view is captured).
      if (node && (inTransition.current || target.bring)) reveal(node.closest('.ws-tile') ?? node);
    }
  });

  // The open tile of the strip stays in view on a phone; set before paint, so a view transition captures it there.
  useLayoutEffect(() => {
    const board = document.querySelector<HTMLElement>('.ws-board');
    const on = board?.querySelector<HTMLElement>('.ws-tile.on');
    if (board && on && board.scrollWidth > board.clientWidth) board.scrollLeft = on.offsetLeft - (board.clientWidth - on.offsetWidth) / 2;
  }, [shown]);

  // The folded map's height, for the part's left column that sticks under it.
  useEffect(() => {
    const main = mainRef.current;
    if (!main || typeof ResizeObserver !== 'function') return;
    const observer = new ResizeObserver(() => main.style.setProperty('--ws-foldh', `${Math.round((main.clientWidth * FOLD_HEIGHT) / MAP_WIDTH)}px`));
    observer.observe(main);
    return () => observer.disconnect();
  }, []);

  // Map or Order. The swap runs on its own timers: a navigation in the middle never leaves it half done.
  function setView(next: View) {
    if (next === view || !mapped) return;
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

  const keyHandler = useRef<(event: KeyboardEvent) => void>(() => {});
  keyHandler.current = (event: KeyboardEvent) => {
    if (event.key === 'Escape' && !isHome && !isField(event.target)) {
      // A held key repeats: only the press itself goes back.
      if (!event.repeat) back();
    }
  };
  useEffect(() => {
    const listener = (event: KeyboardEvent) => keyHandler.current(event);
    document.addEventListener('keydown', listener);
    return () => document.removeEventListener('keydown', listener);
  }, []);

  const signIn = loginUrl(typeof location === 'undefined' ? '' : location.hash);
  const ctx: EntryContext = {
    w, signIn, open: openKeys,
    onToggle: (row) => (openKeys.has(row.key) ? closeRow(row.key) : openRow(row.key)),
  };
  const account = <Account me={w.me} signIn={signIn} onSignOut={() => void w.signOut()} />;

  const notes = (
    <>
      {w.offline ? (
        <p className="ws-quiet ws-note ws-offline">
          <span>Votes and comments are offline.</span>
          <button type="button" className="ws-link ws-retry" disabled={live.load === 'pending'} onClick={w.retry}>Try again</button>
        </p>
      ) : null}
      {w.signinFailed ? <p className="ws-quiet ws-note">Sign-in did not complete.</p> : null}
    </>
  );
  const present = new Set(plan.parts.map((group) => group.state));
  const suggested = plan.suggestions.filter((row) => row.status !== 'dropped').length;
  const current = shown.kind === 'part' ? shown.part : shown.kind === 'suggested' ? 'suggested' : null;
  const link = (target: Place) => (event: { preventDefault: () => void }) => {
    event.preventDefault();
    openPlace(target);
  };

  let detail = null;
  if (shown.kind === 'part') {
    const group = plan.parts.find((item) => item.part.id === shown.part);
    if (group) detail = <PartPlace group={group} ctx={ctx} onBack={back} notes={notes} />;
  } else if (shown.kind === 'suggested') detail = <SuggestPlace ctx={ctx} onBack={back} notes={notes} />;
  else if (shown.kind === 'left') detail = <LeftPlace rows={plan.left} ctx={ctx} onBack={back} notes={notes} />;
  else if (shown.kind === 'product') detail = <ProductPlace rows={plan.product} ctx={ctx} onBack={back} notes={notes} />;
  else if (shown.kind === 'latest') detail = <LatestPlace ctx={ctx} onBack={back} notes={notes} />;

  // The place in the first frame is drawn where it stands: it never runs its entrance afterwards. A place opened
  // later rises in as before.
  const firstPlace = useRef<string | null>(null);
  if (boot && !isHome) firstPlace.current = formatHash(shown);
  else if (!boot && firstPlace.current !== null && firstPlace.current !== formatHash(shown)) firstPlace.current = null;
  const classes = ['ws', isHome ? 'is-sheet' : 'is-open', `v-${view}`, leaving ? 'ws-leaving' : '', swap ? `swap-${swap}` : '', boot ? 'ws-boot' : '', lifting ? 'ws-lifting' : '', held ? 'ws-keeping' : '',
    arrivedState ? 'ws-arr-s' : '', arrivedPerson ? 'ws-arr-p' : ''].filter(Boolean).join(' ');
  return (
    <>
      {accountSlot ? createPortal(account, accountSlot) : accountSlot === false ? null : <div className="ws-account-inline">{account}</div>}
      <div className={classes} ref={mainRef} data-live={live.load}>
        <section className="ws-top joint">
          <div className="ws-head">
            <h1>Workshop</h1>
            <Status key={statusLine(plan.parts)} text={statusLine(plan.parts)} boot={boot} />
          </div>
          <div className="ws-top-side">
            <div className="ws-tools">
              {mapped ? (
                <div className="ws-seg" role="group" aria-label="Arrangement">
                  <button type="button" aria-pressed={view === 'map'} onClick={() => setView('map')}>Map</button>
                  <button type="button" aria-pressed={view === 'order'} onClick={() => setView('order')}>Order</button>
                </div>
              ) : null}
            </div>
            <ul className="ws-key" aria-label="Key">
              {KEY.filter(([state]) => present.has(state)).map(([state, mark, label]) => <KeyItem key={state} mark={mark} label={label} boot={boot} />)}
            </ul>
          </div>
        </section>
        {mapped ? (
          <div className="ws-swap ws-swap-map">
            <MapView layout={data.map} groups={plan.parts} folded={!isHome && !leaving} current={current} onOpen={(part) => openPlace({ kind: 'part', part })} />
          </div>
        ) : null}
        <div className="ws-swap ws-swap-order">
          <Tiles groups={plan.parts} current={current} suggested={suggested} onOpen={(part) => openPlace({ kind: 'part', part })} onSuggest={() => openPlace({ kind: 'suggested' })} />
        </div>
        {isHome ? (
          <div className="ws-under">
            <div className="ws-under-left">
              {plan.left.length ? <a className="ws-fold-link" href="#left" onClick={link({ kind: 'left' })}>Left out · {plan.left.length}</a> : null}
              {plan.product.length ? <a className="ws-fold-link" href="#product" onClick={link({ kind: 'product' })}>The whole product</a> : null}
              <a className="ws-fold-link" href="#latest" onClick={link({ kind: 'latest' })}>Latest</a>
              {suggested ? <a className="ws-fold-link ws-late-s" href="#suggested" onClick={link({ kind: 'suggested' })}>{suggested} suggested</a> : null}
              {notes}
            </div>
            <a className="ws-btn ws-suggest" href="#suggested" onClick={link({ kind: 'suggested' })}>Suggest a feature</a>
          </div>
        ) : (
          <section className={formatHash(shown) === firstPlace.current ? 'ws-detail ws-still' : 'ws-detail'} key={formatHash(shown)} aria-labelledby="ws-part-title">{detail}</section>
        )}
        <p className="sr-only" aria-live="polite">{w.announce}</p>
        {w.notice ? <p className="ws-notice" role="alert">{w.notice}</p> : null}
      </div>
    </>
  );
}
