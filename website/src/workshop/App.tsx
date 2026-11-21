// The Workshop: the drawing of Silo (tiles on narrow screens) with the films' line above it, an opened part or film
// under it, and All, every feature and decision in the order they are built. Entries open in place. Product and the
// suggestions are places too, linked from the title row, which also holds the search and the switch between the map
// and All. Addresses follow routes.ts and all.ts; the live state and every write live in useWorkshop.ts.
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import type { ReactNode } from 'react';
import { createPortal, flushSync } from 'react-dom';
import { allSections, filmSections, formatQuery, graphOf, locate, NO_QUERY, parseQuery, partSections, productSections, rowKeyOf } from './all';
import type { Query, Section, View } from './all';
import { AllView } from './AllView';
import { loginUrl } from './api';
import type { Me } from './api';
import { FilmPlace, PartPlace, ProductPlace } from './PartView';
import type { PlaceList } from './PartView';
import type { EntryContext } from './Entry';
import { FilmsLine, Icon } from './Films';
import { MapView } from './MapView';
import { FOLD_HEIGHT, hasMap, MAP_WIDTH } from './map';
import { recallHabit, rememberHabit } from './memory';
import { statusLine } from './plan';
import type { BuildData, Facts, Histories, Notes, Row } from './plan';
import { relationStore } from './relation';
import { formatHash, HOME, placeFrom, resolveHash, rowHash, rowsOf, samePlace } from './routes';
import type { Place } from './routes';
import { SuggestPlace } from './Suggest';
import { Tiles } from './Tiles';
import { Tips } from './Tips';
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

/** What the page uses of the browser's `ViewTransition`. */
type ViewTransitionLike = { finished: Promise<void>; ready?: Promise<void>; skipTransition?: () => void };
/** `bring`: what takes the focus is brought into view at once (going back off the map). */
type Focus = { kind: 'title' } | { kind: 'part'; part: string; bring?: boolean } | { kind: 'selector'; selector: string; bring?: boolean } | { kind: 'row'; key: string; reveal?: boolean; again?: boolean } | { kind: 'reveal'; key: string; again?: boolean };

const LEAVE_MS = 140;
/** How long what just arrived is marked, for its fade in (about 150 ms). */
const ARRIVE_MS = 400;
const ACCOUNT_WIDTH = 'silo-workshop-account-width';
/** At most how long the map stays pinned under the header after going back from a scrolled part. */
const PIN_MS = 1500;
/** How long the map takes to fold, and an entry to open. */
const FOLD_MS = 440;

function media(query: string, fallback: boolean): boolean {
  return typeof matchMedia === 'function' ? matchMedia(query).matches : fallback;
}
const still = () => media('(prefers-reduced-motion: reduce)', true);
/** The width from which the map shows; narrower, the tiles. */
const wide = () => media('(min-width: 1200px)', true);


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


/**
 * The line under the title; a line that appears or changes after the first frames fades in. On the plain view it also
 * holds the notes (offline, sign-in failed), after the text.
 */
function Status({ text, boot, children }: { text: string; boot: boolean; children?: ReactNode }) {
  const [fade] = useState(!boot);
  return <p className={fade ? 'ws-status ws-in' : 'ws-status'}>{text}{children}</p>;
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


/**
 * The view and All's query the address asks for. A plain address shows the view and the order the person used last
 * (memory.ts); at build time there is no address and nothing remembered: the map.
 */
function fromAddress(): { view: View; query: Query; recalled: boolean } {
  if (typeof location === 'undefined') return { view: 'map', query: NO_QUERY, recalled: false };
  const asked = parseQuery(location.search);
  const plain = !location.search && location.hash.length <= 1;
  const habit = plain ? recallHabit() : null;
  const view = asked.view ?? habit?.view ?? 'map';
  const order = asked.order ?? (habit && view === 'all' ? habit.order : 'build');
  return { view, query: { ...asked.query, order }, recalled: Boolean(habit && view === 'all') };
}
const address = (search: string, hash = '') => `${location.pathname}${search}${hash}`;

/** The search of the title row: an icon until pressed, then a field. Typing shows All with the matches. */
function Search({ value, open, onOpen, onChange, onClose }: { value: string; open: boolean; onOpen: () => void; onChange: (text: string) => void; onClose: () => void }) {
  const field = useRef<HTMLInputElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  // Escape in the field gives the focus back to the search button once it is drawn again.
  const back = useRef(false);
  useEffect(() => {
    if (open) field.current?.focus();
  }, [open]);
  const closed = !open && !value;
  useEffect(() => {
    if (!closed || !back.current) return;
    back.current = false;
    button.current?.focus();
  }, [closed]);
  if (closed) {
    return <button type="button" ref={button} className="wa-ctl icon wa-searchctl" aria-label="Search" data-tip="Search" data-kbd="/" onClick={onOpen}><Icon name="search" /></button>;
  }
  return (
    <label className="wa-search">
      <Icon name="search" />
      <input
        ref={field} type="search" value={value} aria-label="Search" placeholder="Search"
        onChange={(event) => onChange(event.target.value)}
        onBlur={() => { if (!value) onClose(); }}
        onKeyDown={(event) => {
          if (event.key !== 'Escape') return;
          event.stopPropagation();
          back.current = true;
          onChange('');
          onClose();
        }}
      />
      {value ? <button type="button" className="wa-x" aria-label="Clear the search" onClick={() => { onChange(''); field.current?.focus(); }}><Icon name="x" /></button> : null}
    </label>
  );
}

export function App({ data, loadNotes = noNotes, loadFacts = noFacts, loadHistory = noHistory, accountSlot }: AppProps) {
  const mapped = hasMap(data.map);
  const [first] = useState(fromAddress);
  const [place, setPlace] = useState<Place>(HOME);
  // Rows open independently. Being open belongs to the entry, by its row's key, not to one drawing of it: a decision
  // opened under its feature is open wherever the list draws it. `openKey` is the one opened last: the address follows
  // it. `openAt` is the drawing of it last opened or found (all.ts `underKey`), whose relations are drawn.
  const [openKey, setOpenKey] = useState<string | null>(null);
  const [openKeys, setOpenKeys] = useState<ReadonlySet<string>>(new Set());
  const [openAt, setOpenAt] = useState<string | null>(null);
  const [openings, setOpenings] = useState(0);
  const [view, setViewState] = useState<View>(first.view);
  const [query, setQueryState] = useState<Query>(first.query);
  const [searching, setSearching] = useState(false);
  // Features and bundles folded or unfolded by hand, and those whose "more" was pressed.
  const [folds, setFolds] = useState<ReadonlyMap<string, boolean>>(new Map());
  const [more, setMore] = useState<ReadonlySet<string>>(new Set());
  // The film pointed at on the films' line: the map marks its parts.
  const [filmPoint, setFilmPoint] = useState<string | null>(null);
  const [store] = useState(relationStore);
  const handlers = useRef<{ toggle: EntryContext['onToggle']; filmEntries: (film: string) => void }>({ toggle: () => {}, filmEntries: () => {} });
  const stableToggle = useCallback<EntryContext['onToggle']>((row, instance) => handlers.current.toggle(row, instance), []);
  const stableFilmEntries = useCallback((film: string) => handlers.current.filmEntries(film), []);
  const [leaving, setLeaving] = useState(false);
  // After going back from a scrolled part on the map, the map stays pinned under the header until the scroll to the top
  // has brought its own place below that line: unpinned sooner, it would jump up, then come down with the scroll.
  const [pinned, setPinned] = useState(false);
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
  const graph = useMemo(() => graphOf(plan), [plan]);
  const sections = useMemo(() => allSections(plan, graph, data.stages, query), [plan, graph, data.stages, query]);
  const focusNext = useRef<Focus | null>(null);
  /** An address naming what has not arrived yet (a suggestion comes with the live state), and whether the browser's history gave it. */
  const wantedHash = useRef<{ hash: string; fromHistory: boolean } | null>(null);
  const foldedAt = useRef(0);
  /** The scroll driven by the fold, from a scrolled full map. */
  const lift = useRef<{ frame: number; from: number; full: number } | null>(null);
  /** A count of navigations, so a view transition's update that has been overtaken never applies. */
  const navigations = useRef(0);
  /** Clears the page's held height after the map unfolds; the next move or unmounting releases it sooner. */
  const heldHeight = useRef<ReturnType<typeof setTimeout>>(undefined);
  /** The count of the move that is a going-back; while no move came after it, that going-back is under way. */
  const goingBack = useRef(-1);
  /** The move whose view transition set the mark on `<html>` (`vt-open`, `vt-back`); -1 when none is set. */
  const markOwner = useRef(-1);
  /** True while a view transition's update runs: what it scrolls is set at once, before the new view is captured. */
  const inTransition = useRef(false);
  /** The browser's view transition of the last move that started one, until it ends or another move starts. */
  const transition = useRef<ViewTransitionLike | null>(null);
  const queryRef = useRef(query);
  queryRef.current = query;
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
  const mainRef = useRef<HTMLDivElement>(null);
  useEffect(() => () => {
    timers.current.forEach(clearTimeout);
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
  // The row opened last draws its relations while the pointer and the focus are elsewhere.
  useEffect(() => store.open(openAt), [store, openAt]);

  const shown: Place = place;
  const isAll = view === 'all';
  const isHome = shown.kind === 'home';
  const onMap = () => !isAll && wide() && mapped;

  /** The rows of a place as its list draws them, to find where an entry stands. */
  const sectionsOf = (where: Place, inAll: boolean): Section[] => {
    if (inAll) return sections;
    if (where.kind === 'part') return partSections(graph, rowsOf(plan, where));
    if (where.kind === 'product') return productSections(plan.product);
    if (where.kind === 'film') return filmSections(graph, rowsOf(plan, where));
    return [];
  };
  /** Where an entry stands in a list: the instance to open, its holder unfolded (and its "more" pressed) when it is under one. */
  const standing = (key: string, where: Place, inAll: boolean): string => {
    if (where.kind === 'suggested') return key;
    const found = locate(sectionsOf(where, inAll), key, more);
    if (!found) return key;
    if (found.holder) {
      const holder = found.holder;
      setFolds((now) => (now.get(holder) === true ? now : new Map(now).set(holder, true)));
      if (found.more) setMore((now) => new Set(now).add(holder));
    }
    return found.instance;
  };

  /** Moves to a place, with at most the one row open; `instance` is the drawing of it to show open (its own key by default). */
  function settle(next: Place, key: string | null, instance: string | null = key) {
    timers.current.forEach(clearTimeout);
    timers.current = [];
    setLeaving(false);
    setPlace(next);
    setOpenKey(key);
    setOpenKeys(key ? new Set([key]) : new Set());
    setOpenAt(key ? instance : null);
    if (key) setOpenings((n) => n + 1);
    openKeyRef.current = key;
    navigations.current += 1;
  }

  /** The address of what is shown: the view and All's query, then the place or, in All, the open row. */
  function hashOf(next: Place, instance: string | null, inAll = isAll): string {
    const row = instance ? rowByKey.get(rowKeyOf(instance)) : undefined;
    if (inAll) return row ? rowHash(row) : '';
    return formatHash(next, row ? row.key : null);
  }
  function address$(next: Place, instance: string | null, push = false, inAll = isAll, q = query) {
    const target = address(formatQuery(location.search, inAll ? 'all' : 'map', q), hashOf(next, instance, inAll));
    if (target === `${location.pathname}${location.search}${location.hash}`) return;
    if (push) history.pushState(null, '', target);
    else history.replaceState(null, '', target);
  }

  function go(next: Place, instance: string | null, options: { push?: boolean; focus?: Focus | null; announce?: string } = {}) {
    address$(next, instance, options.push);
    if ((next.kind === 'part' || next.kind === 'film') && shown.kind === 'home') foldedAt.current = Date.now();
    settle(next, instance);
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
    setPinned(false);
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

  /**
   * Opens an entry: next to the open ones where it stands now, or alone in the place where it lives. A link to an entry
   * always opens it: in All when the list holds it, else in its own place (its part, Product, the suggestions).
   * `instance` names the drawing pressed, when one was.
   */
  function openRow(key: string | null, options: { focus?: boolean; reveal?: boolean; push?: boolean; closing?: string; instance?: string } = {}) {
    const row = key ? rowByKey.get(key) : undefined;
    if (!row) {
      if (options.closing) closeRow(options.closing);
      return;
    }
    const inList = isAll && (options.instance !== undefined || locate(sections, row.key, more) !== null);
    const target = isAll ? (inList ? HOME : placeOf$(row)) : placeFrom(row, shown, plan);
    if (!inList && (isAll || !samePlace(target, shown))) {
      w.fetchNow(row);
      if (isAll) setViewState('map');
      const instance = standing(row.key, target, false);
      address$(target, row.key, options.push !== false, false);
      if ((target.kind === 'part' || target.kind === 'film') && shown.kind === 'home') foldedAt.current = Date.now();
      settle(target, row.key, instance);
      w.setAnnounce(`${row.name}, open`);
      focusNext.current = options.focus ? { kind: 'row', key: instance, reveal: options.reveal !== false } : null;
      return;
    }
    const instance = options.instance ?? standing(row.key, shown, isAll);
    // Asked for in the same event as the opening, so a failed early fetch shows as loading, not as an error.
    w.fetchNow(row);
    address$(shown, row.key);
    setOpenKeys((current) => {
      const next = new Set(current);
      if (options.closing) next.delete(options.closing);
      next.add(row.key);
      return next;
    });
    setOpenKey(row.key);
    setOpenAt(instance);
    setOpenings((n) => n + 1);
    openKeyRef.current = row.key;
    w.setAnnounce(`${row.name}, open`);
    focusNext.current = options.focus ? { kind: 'row', key: instance, reveal: options.reveal !== false } : null;
  }

  /** Closes one entry, wherever it is drawn, and leaves the others as they are. */
  function closeRow(key: string) {
    setOpenKeys((current) => {
      const next = new Set(current);
      next.delete(key);
      return next;
    });
    if (openKey === key || openKeyRef.current === key) {
      address$(shown, null);
      setOpenKey(null);
      setOpenAt(null);
      openKeyRef.current = null;
    }
  }

  function back() {
    // A going-back under way lands at its time: a second one (the button pressed twice, Escape again) changes nothing.
    if (isHome || backUnderWay()) return;
    // What leads to the place being closed takes the focus, however the place was reached, or the page's title.
    // Off the map, with a view transition or without one, it is brought into view; the map has its own going-back.
    const bring = !onMap();
    const focus: Focus = shown.kind === 'part' ? { kind: 'part', part: shown.part, bring }
      : shown.kind === 'film' ? { kind: 'selector', selector: `.wa-films [data-film="${shown.film}"]`, bring }
        : { kind: 'selector', selector: `.ws-places a[href="${formatHash(shown)}"]`, bring };
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
    if (scrollY > 0) {
      setPinned(true);
      scrollTo({ top: 0, behavior: 'smooth' });
    }
    setLeaving(true);
    later(finish, LEAVE_MS);
  }

  function openPlace(next: Place) {
    if (samePlace(next, shown) && !isAll) {
      // While going back from this place, its part or tile is a new move that keeps it open: the going-back is dropped.
      if (backUnderWay()) moveStarts();
      else back();
      return;
    }
    if (isAll) {
      // From All, a place opens on the map's side, at once.
      moveStarts();
      setViewState('map');
      rememberHabit({ view: 'map', order: recallHabit()?.order ?? 'build' });
      const title = next.kind === 'part' ? plan.parts.find((group) => group.part.id === next.part)?.part.title : null;
      address$(next, null, true, false);
      if (next.kind === 'part' || next.kind === 'film') foldedAt.current = Date.now();
      settle(next, null);
      w.setAnnounce(title ? `${title}, open` : '');
      focusNext.current = { kind: 'title' };
      return;
    }
    // From a scrolled full map, the fold drives the scroll back to the top: each frame the page is scrolled in
    // proportion to how far the map has folded, so every box travels one way during the fold.
    if (isHome && onMap() && scrollY > 0 && !still()) startLift();
    const title = next.kind === 'part' ? plan.parts.find((group) => group.part.id === next.part)?.part.title : next.kind === 'film' ? plan.films.find((film) => film.id === next.film)?.title : null;
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

  /** The map or All, from a press of the switch: a move of its own, which Back undoes. */
  function setView(next: View) {
    if (next === view && isHome) return;
    moveStarts();
    setViewState(next);
    // Pressing All shows the order used last, and writes it into the address. The order is remembered only from its own
    // control: an address that shows another one changes nothing remembered.
    const order = recallHabit()?.order ?? 'build';
    const q = next === 'all' ? { ...query, order } : query;
    if (next === 'all') setQueryState(q);
    rememberHabit({ view: next, order });
    address$(HOME, null, true, next === 'all', q);
    settle(HOME, null);
    focusNext.current = null;
  }

  /** All's query changes in place: the address follows, and the folds made by hand start over. */
  function setQuery(next: Query) {
    setQueryState(next);
    if (next.order !== query.order) rememberHabit({ view: 'all', order: next.order });
    const filtersChanged = next.states.join() !== query.states.join() || next.parts.join() !== query.parts.join() || next.films.join() !== query.films.join() || next.q !== query.q || next.order !== query.order;
    const nextSections = allSections(plan, graph, data.stages, next);
    if (filtersChanged) {
      // The folds made by hand start over, but an open entry is always drawn while the list holds it: its feature stays
      // unfolded, and its "more" pressed when it lies past the first six.
      const folds = new Map<string, boolean>();
      const shownMore = new Set<string>();
      for (const key of openKeys) {
        const found = locate(nextSections, key, new Set());
        if (!found?.holder) continue;
        folds.set(found.holder, true);
        if (found.more) shownMore.add(found.holder);
      }
      setFolds(folds);
      setMore(shownMore);
    }
    if (!isAll) {
      // A search typed elsewhere shows All with its matches.
      moveStarts();
      setViewState('all');
      address$(HOME, null, true, true, next);
      settle(HOME, null);
      return;
    }
    // The entry open last stays named only while the list still draws it; open, it is drawn open wherever it now stands.
    const open = openKeyRef.current;
    const found = open ? locate(nextSections, open, new Set()) : null;
    if (open && !found) {
      setOpenKey(null);
      setOpenAt(null);
      openKeyRef.current = null;
    } else if (found) setOpenAt(found.instance);
    address$(HOME, found ? open : null, false, true, next);
  }

  /** The entries a film shows, in All. */
  function filmEntries(film: string) {
    setQuery({ ...NO_QUERY, films: [film], order: query.order });
    if (isAll) scrollTo({ top: 0, behavior: 'instant' });
  }

  // Addresses: on load, on Back and Forward, and once rows that did not exist yet arrive. The address is the truth: an
  // address with no `order` is Build order. An entry it names opens at once, from the build's data: its comments fill
  // their placeholders when the live state comes.
  const follow = useCallback((fromHistory: boolean) => {
    const resolved = resolveHash(location.hash, plan, rowByKey);
    if (!resolved) {
      wantedHash.current = { hash: location.hash, fromHistory };
      return;
    }
    wantedHash.current = null;
    const asked = parseQuery(location.search);
    let inAll = asked.view === 'all';
    let q = asked.query;
    // An old address that was a place (Latest, Left out) opens All as it asks.
    if (resolved.all) {
      inAll = true;
      q = { ...NO_QUERY, ...resolved.all };
    }
    // In All, a place's address (Product, a film, a part with no row) opens that place on the map's side.
    if (inAll && resolved.key === null && resolved.place.kind !== 'home') inAll = false;
    const row = resolved.key === null ? undefined : rowByKey.get(resolved.key);
    const lists = inAll ? allSections(plan, graphOf(plan), data.stages, q) : null;
    let found = lists && resolved.key !== null ? locate(lists, resolved.key, new Set()) : null;
    // A link to an entry always opens it: when the list does not hold it, in its own place.
    if (inAll && row && !found) inAll = false;
    // In All, the list; asked in All but not held there, the entry's own place; else the place the address names.
    const where: Place = inAll ? HOME : row && lists ? placeOf$(row) : resolved.place;
    if (!inAll && row) found = locate(sectionsOf(where, false), row.key, new Set());
    if (found?.holder) {
      const holder = found.holder;
      setFolds((now) => new Map(now).set(holder, true));
      if (found.more) setMore((now) => new Set(now).add(holder));
    }
    setViewState(inAll ? 'all' : 'map');
    setQueryState(q);
    const canonical = address(formatQuery(location.search, inAll ? 'all' : 'map', q), inAll ? (row ? rowHash(row) : '') : formatHash(where, row ? row.key : null));
    // An address read from the history is shown as it is; one followed on load or by hand is written in its own form.
    if (!fromHistory && `${location.pathname}${location.search}${location.hash}` !== canonical && (location.hash || resolved.all || location.search)) history.replaceState(history.state, '', canonical);
    const instance = row ? found?.instance ?? row.key : null;
    settle(where, row ? row.key : null, instance);
    focusNext.current = instance ? { kind: 'reveal', key: instance } : null;
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [plan, rowByKey]);
  const followRef = useRef(follow);
  followRef.current = follow;
  const moveStartsRef = useRef(moveStarts);
  moveStartsRef.current = moveStarts;
  // The address is read before the first paint, so the first frame is already the place it names. On Back, Forward
  // and an address followed it is read again, as on a fresh load. It is a new move, even when the place it names is
  // not known yet. A view recalled for a plain address is written into it.
  useLayoutEffect(() => {
    if (first.recalled) history.replaceState(history.state, '', address(formatQuery(location.search, 'all', first.query), location.hash));
    followRef.current(false);
    // Back and Forward across fragments fire popstate, then hashchange for the same address: the second is the same
    // move and is not followed again. Where the browser says how it navigates (the Navigation API), a popstate that is
    // not a traverse (a fragment followed or typed) is a person's address; elsewhere every popstate is the history's.
    let kind: string | null = null;
    let popped: string | null = null;
    const nav = (window as Window & { navigation?: EventTarget }).navigation;
    const onNavigate = (event: Event) => { kind = (event as Event & { navigationType?: string }).navigationType ?? null; };
    const onPop = () => {
      const fromHistory = kind === null || kind === 'traverse';
      kind = null;
      popped = location.href;
      moveStartsRef.current();
      followRef.current(fromHistory);
    };
    const onHash = () => {
      if (popped === location.href) {
        popped = null;
        return;
      }
      popped = null;
      moveStartsRef.current();
      followRef.current(false);
    };
    nav?.addEventListener('navigate', onNavigate);
    addEventListener('popstate', onPop);
    addEventListener('hashchange', onHash);
    return () => {
      nav?.removeEventListener('navigate', onNavigate);
      removeEventListener('popstate', onPop);
      removeEventListener('hashchange', onHash);
    };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);
  useEffect(() => {
    const wanted = wantedHash.current;
    if (wanted === null) return;
    // However late it loads, an address the history gave is shown as it is.
    if (resolveHash(wanted.hash, plan, rowByKey)) follow(wanted.fromHistory);
    else if (w.settled) {
      // A link to something that the state shown does not have falls back to the plan, once the load has ended
      // (the `fresh=1` ask included, when one was made: what was just suggested may be only in that answer).
      wantedHash.current = null;
      if (!wanted.fromHistory) history.replaceState(history.state, '', address(location.search));
      settle(HOME, null);
    }
  }, [plan, rowByKey, w.settled, follow]);

  // Focus follows what the person asked for; a new part starts at its top, under the folded map.
  useLayoutEffect(() => {
    const target = focusNext.current;
    if (!target) return;
    focusNext.current = null;
    if (target.kind === 'row' || target.kind === 'reveal') {
      const item = document.getElementById(target.key);
      // A row under a feature that unfolds in this same move is drawn on the next render: wait for it, once.
      if (!item && !target.again) {
        focusNext.current = { ...target, again: true };
        return;
      }
      if (target.kind === 'row') item?.querySelector<HTMLElement>(':scope > .wa-row > .wa-hit, :scope > .ws-line > .ws-row')?.focus({ preventScroll: true });
      // While the map folds, the page under it moves: the row is brought into view once the fold is done.
      const wait = Math.max(0, foldedAt.current + FOLD_MS - Date.now());
      const show = () => document.getElementById(target.key)?.scrollIntoView?.({ block: 'nearest', behavior: still() ? 'instant' : 'smooth' });
      if (target.kind === 'row' && target.reveal === false) return;
      if (wait) later(show, wait);
      else show();
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
      else selector = onMap() ? `.ws-part[data-part="${target.part}"]` : `.ws-tile[data-part="${target.part}"] .ws-tile-hit`;
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

  // The map pinned after going back stays so until its own place is below the header, at most PIN_MS.
  useEffect(() => {
    if (!pinned) return;
    const check = () => {
      const top = document.querySelector<HTMLElement>('.ws-top');
      const header = document.querySelector<HTMLElement>('.site-header')?.offsetHeight ?? 0;
      if (!top || scrollY <= top.offsetTop + top.offsetHeight - header) setPinned(false);
    };
    addEventListener('scroll', check, { passive: true });
    const timer = setTimeout(() => setPinned(false), PIN_MS);
    return () => {
      removeEventListener('scroll', check);
      clearTimeout(timer);
    };
  }, [pinned]);

  const keyHandler = useRef<(event: KeyboardEvent) => void>(() => {});
  keyHandler.current = (event: KeyboardEvent) => {
    if (isField(event.target) || event.metaKey || event.ctrlKey || event.altKey) return;
    if (event.key === '/') {
      event.preventDefault();
      setSearching(true);
    } else if (event.key === 'Escape' && !isHome && !isAll) {
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
  // The rows call these through stable functions, so a row that does not render again still calls the current ones.
  handlers.current = {
    toggle: (row, instance) => {
      if (openKeys.has(row.key)) closeRow(row.key);
      else openRow(row.key, instance === undefined ? {} : { instance });
    },
    filmEntries,
  };
  const ctx: EntryContext = { w, signIn, open: openKeys, onToggle: stableToggle, onFilmEntries: stableFilmEntries };
  const list: PlaceList = {
    plan, graph, ctx, store, folds, more, waiting: w.waiting,
    onFold: (key, open) => setFolds((now) => new Map(now).set(key, open)),
    onMore: (key) => setMore((now) => new Set(now).add(key)),
  };
  const account = <Account me={w.me} signIn={signIn} onSignOut={() => void w.signOut()} />;

  // Offline, with a way to try again, and sign-in failed: in a place, under its counts; on the plain view, on the line
  // under the title (`inline`), where they are spans of the paragraph.
  const notesOf = (inline: boolean) => {
    const Note = inline ? 'span' : 'p';
    return (
      <>
        {w.offline ? (
          <Note className="ws-quiet ws-note ws-offline">
            <span>Votes and comments are offline.</span>
            <button type="button" className="ws-link ws-retry" disabled={live.load === 'pending'} onClick={w.retry}>Try again</button>
          </Note>
        ) : null}
        {w.signinFailed ? <Note className="ws-quiet ws-note">Sign-in did not complete.</Note> : null}
      </>
    );
  };
  const notes = notesOf(false);
  const current = shown.kind === 'part' ? shown.part : null;
  const film = shown.kind === 'film' ? plan.films.find((one) => one.id === shown.film) ?? null : null;
  // The map marks the parts of the film pointed at, or of the film whose place is open.
  const pointed = filmPoint ? plan.films.find((one) => one.id === filmPoint) : null;
  const marked = pointed ? new Set(pointed.parts) : film ? new Set(film.parts) : null;
  const link = (target: Place) => (event: { preventDefault: () => void }) => {
    event.preventDefault();
    openPlace(target);
  };
  /** A link of the title row: marked while its place is open, where pressing it again goes back. */
  const placeLink = (target: Place, label: string, className = 'ws-place') => {
    const on = !isAll && samePlace(target, shown);
    return <a className={on ? `${className} on` : className} href={formatHash(target)} aria-current={on ? 'true' : undefined} onClick={link(target)}>{label}</a>;
  };

  let detail = null;
  if (shown.kind === 'part') {
    const group = plan.parts.find((item) => item.part.id === shown.part);
    if (group) detail = <PartPlace group={group} list={list} onBack={back} notes={notes} onFilm={(id) => openPlace({ kind: 'film', film: id })} />;
  } else if (shown.kind === 'suggested') detail = <SuggestPlace ctx={ctx} onBack={back} notes={notes} />;
  else if (shown.kind === 'product') detail = <ProductPlace rows={plan.product} list={list} onBack={back} notes={notes} />;
  else if (film) detail = <FilmPlace film={film} rows={rowsOf(plan, shown)} list={list} onBack={back} notes={notes} />;

  // The place in the first frame is drawn where it stands: it never runs its entrance afterwards. A place opened
  // later rises in as before; one that opens with a row open fades in where it stands, so that the rows under that row
  // only move down as its panel grows. Its entrance is chosen once, when it opens.
  const firstPlace = useRef<string | null>(null);
  if (boot && !isHome) firstPlace.current = formatHash(shown);
  else if (!boot && firstPlace.current !== null && firstPlace.current !== formatHash(shown)) firstPlace.current = null;
  const entrance = useRef<{ place: string; fade: boolean } | null>(null);
  if (isHome) entrance.current = null;
  else if (entrance.current?.place !== formatHash(shown)) entrance.current = { place: formatHash(shown), fade: openKey !== null };
  const detailClass = formatHash(shown) === firstPlace.current ? 'ws-detail ws-still' : entrance.current?.fade ? 'ws-detail ws-fade' : 'ws-detail';
  const classes = ['ws', isAll ? 'is-all' : isHome ? 'is-sheet' : 'is-open', isAll ? 'v-all' : 'v-map', mapped ? '' : 'no-map', leaving ? 'ws-leaving' : '', boot ? 'ws-boot' : '', lifting ? 'ws-lifting' : '', held ? 'ws-keeping' : '', pinned ? 'ws-pinned' : '',
    arrivedState ? 'ws-arr-s' : '', arrivedPerson ? 'ws-arr-p' : ''].filter(Boolean).join(' ');
  const status = statusLine(plan.parts);
  return (
    <>
      {accountSlot ? createPortal(account, accountSlot) : accountSlot === false ? null : <div className="ws-account-inline">{account}</div>}
      <div className={classes} ref={mainRef} data-live={live.load}>
        <section className="ws-top joint">
          <div className="ws-head">
            <h1>Workshop</h1>
            <Status key={status} text={status} boot={boot}>{isHome ? notesOf(true) : null}</Status>
          </div>
          <div className="ws-top-side">
            <div className="ws-tools">
              <nav className="ws-places" aria-label="Places">
                {plan.product.length ? placeLink({ kind: 'product' }, 'Product') : null}
                <Search value={query.q} open={searching} onOpen={() => setSearching(true)} onClose={() => setSearching(false)} onChange={(text) => setQuery({ ...query, q: text })} />
                {placeLink({ kind: 'suggested' }, 'Suggest a feature', 'ws-btn ws-suggest')}
              </nav>
              <div className="ws-seg wa-view" role="group" aria-label="View">
                <button type="button" aria-pressed={!isAll} onClick={() => setView('map')}>{mapped ? <><span className="wa-wide">Map</span><span className="wa-narrow">Parts</span></> : 'Parts'}</button>
                <button type="button" aria-pressed={isAll} onClick={() => setView('all')}>All</button>
              </div>
            </div>
          </div>
        </section>
        {isAll ? (
          <AllView
            plan={plan} graph={graph} sections={sections} query={query} onQuery={setQuery} ctx={ctx} store={store}
            folds={folds} onFold={list.onFold} more={more} onMore={list.onMore} waiting={w.waiting}
            onOpen={(key) => openRow(key, { focus: true })}
          />
        ) : (
          <>
            <FilmsLine films={plan.films} current={film?.id ?? null} onOpen={(id) => openPlace({ kind: 'film', film: id })} onPoint={setFilmPoint} />
            {mapped ? (
              <div className="ws-swap ws-swap-map">
                <MapView layout={data.map} groups={plan.parts} folded={!isHome && !leaving} current={current} marked={marked} onOpen={(part) => openPlace({ kind: 'part', part })} />
              </div>
            ) : null}
            <div className="ws-swap ws-swap-order">
              <Tiles groups={plan.parts} current={current} onOpen={(part) => openPlace({ kind: 'part', part })} />
            </div>
            {isHome ? null : (
              <section className={detailClass} key={formatHash(shown)} aria-labelledby="ws-part-title">{detail}</section>
            )}
          </>
        )}
        <p className="sr-only" aria-live="polite">{w.announce}</p>
        {w.notice ? <p className="ws-notice" role="alert">{w.notice}</p> : null}
      </div>
      <Tips />
    </>
  );
}

/** Where a row lives, as a place on the map's side. */
function placeOf$(row: Row): Place {
  if (row.kind === 'suggestion') return { kind: 'suggested' };
  return row.part ? { kind: 'part', part: row.part } : { kind: 'product' };
}
