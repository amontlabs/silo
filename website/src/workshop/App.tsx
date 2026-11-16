// The Workshop: the drawing of Silo (tiles on narrow screens), an opened area under it, and its entries
// opening in place. Addresses follow routes.ts; the live state and every write live in useWorkshop.ts.
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { createPortal, flushSync } from 'react-dom';
import { loginUrl } from './api';
import { AreaPlace, LeftPlace } from './AreaView';
import { DecisionsPlace } from './Decisions';
import type { EntryContext } from './Entry';
import { MapView } from './MapView';
import { FOLD_HEIGHT, MAP_WIDTH } from './map';
import { awaitsVerdict, needsOwner, statusLine } from './plan';
import type { AreaState, BuildData, Row } from './plan';
import { formatHash, HOME, placeFrom, placeOf, resolveHash, rowsOf, samePlace } from './routes';
import type { Place } from './routes';
import { SuggestPlace } from './Suggest';
import { Tiles } from './Tiles';
import { OPEN_GUARD_MS, useWorkshop } from './useWorkshop';
import type { Nav } from './useWorkshop';

export { OPEN_GUARD_MS } from './useWorkshop';

export interface AppProps {
  data: BuildData;
  /** The element in the site header that holds sign-in and the signed-in login. */
  accountSlot?: Element | null;
}

type View = 'map' | 'order';
type Focus = { kind: 'title' } | { kind: 'area'; area: string } | { kind: 'selector'; selector: string } | { kind: 'row'; key: string } | { kind: 'reveal'; key: string };

const LEAVE_MS = 140;
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

function isField(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName);
}

const initialView = (): View => (new URLSearchParams(location.search).get('view') === 'order' ? 'order' : 'map');
const address = (hash = '') => `${location.pathname}${location.search}${hash}`;

export function App({ data, accountSlot }: AppProps) {
  const [place, setPlace] = useState<Place>(HOME);
  // Rows open independently; `openKey` is the one opened last: the address, Enter and the guard follow it.
  const [openKey, setOpenKey] = useState<string | null>(null);
  const [openKeys, setOpenKeys] = useState<ReadonlySet<string>>(new Set());
  const [openings, setOpenings] = useState(0);
  const [view, setViewState] = useState<View>(initialView);
  const [leaving, setLeaving] = useState(false);
  const [swap, setSwap] = useState<'out' | 'in' | null>(null);
  const [, setArmed] = useState(0);
  // The first frames take the state as it is, without animating into it.
  const [boot, setBoot] = useState(true);
  const nav = useRef<Nav>({ openKey: null, openedAt: 0, advanceFrom: () => {}, reopen: () => {} });
  const w = useWorkshop(data, openKey, openKeys, openings, nav);
  const { plan, rowByKey, live, canDecide } = w;
  const focusNext = useRef<Focus | null>(null);
  const wantedHash = useRef<string | null>(null);
  /** A row named by the address while the live state loads; it opens once the state is there. */
  const heldOpen = useRef<string | null>(null);
  /** The control that opened the suggestions, the left-out list or the decisions, to return focus to. */
  const opener = useRef<string | null>(null);
  const foldedAt = useRef(0);
  const lifting = useRef(false);
  // Once `me` has answered, the account slot takes the width of what it holds (see workshop.css).
  useEffect(() => {
    if (!accountSlot) return;
    if (w.me === undefined) accountSlot.removeAttribute('data-account');
    else accountSlot.setAttribute('data-account', w.me ? 'in' : 'out');
  }, [accountSlot, w.me]);
  const timers = useRef<ReturnType<typeof setTimeout>[]>([]);
  const swapTimers = useRef<ReturnType<typeof setTimeout>[]>([]);
  const mainRef = useRef<HTMLDivElement>(null);
  useEffect(() => () => [...timers.current, ...swapTimers.current].forEach(clearTimeout), []);
  useEffect(() => {
    let frame = requestAnimationFrame(() => { frame = requestAnimationFrame(() => setBoot(false)); });
    return () => cancelAnimationFrame(frame);
  }, []);
  const later = (fn: () => void, ms: number) => { timers.current.push(setTimeout(fn, ms)); };

  // Visitors never see the decisions: the address shows them the plan.
  const shown: Place = place.kind === 'decisions' && !canDecide ? HOME : place;
  const isHome = shown.kind === 'home';
  const onMap = () => view === 'map' && wide();

  const waiting = useCallback((row: Row) => needsOwner(row) && row.issue !== null && Boolean(live.state?.issues[row.issue]), [live.state]);
  const toDecide = useMemo(() => new Map(canDecide ? plan.areas.map((group) => [group.area.id, group.rows.filter(waiting).length]) : []), [canDecide, plan, waiting]);
  const totalToDecide = [...toDecide.values()].reduce((sum, n) => sum + n, 0);
  const byBuildOrder = useMemo(() => plan.areas.flatMap((group) => group.rows).sort((a, b) => (a.n ?? 0) - (b.n ?? 0)), [plan]);
  const free = (row: Row) => waiting(row) && !w.pendingRef.current.has(row.issue as number);

  // The guard's buttons look armed once it has passed.
  useEffect(() => {
    if (!openKey) return;
    const timer = setTimeout(() => setArmed((n) => n + 1), Math.max(0, nav.current.openedAt + OPEN_GUARD_MS - Date.now()) + 10);
    return () => clearTimeout(timer);
  }, [openKey, openings]);

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
    nav.current.openedAt = Date.now();
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
    w.setRefining(null);
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
    start(() => flushSync(update)).finished.finally(() => { if (tile) tile.style.viewTransitionName = ''; });
  }

  /** Opens a row: next to the open ones in this place, or alone where it lives. */
  function openRow(key: string | null, options: { focus?: boolean; push?: boolean; closing?: string } = {}) {
    const row = key ? rowByKey.get(key) : undefined;
    if (!row) {
      if (options.closing) closeRow(options.closing);
      return;
    }
    const target = placeFrom(row, shown, plan);
    if (!samePlace(target, shown)) {
      go(target, row.key, { push: options.push !== false, announce: `${row.name}, open`, focus: options.focus ? { kind: 'row', key: row.key } : null });
      return;
    }
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
    nav.current.openedAt = Date.now();
    w.setRefining(null);
    w.setRowNotice((current) => (current?.key === row.key ? current : null));
    w.setAnnounce(`${row.name}, open`);
    focusNext.current = options.focus ? { kind: 'row', key: row.key } : null;
  }

  /** Closes one row and leaves the others as they are. */
  function closeRow(key: string) {
    setOpenKeys((current) => {
      const next = new Set(current);
      next.delete(key);
      return next;
    });
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
    const fresh = (item: Row) => item.key !== row.key && free(item);
    const next = ordered.find(fresh) ?? byBuildOrder.find(fresh);
    const take = document.getElementById(row.key)?.querySelector('.ws-btn.go')?.getBoundingClientRect().top;
    openRow(next?.key ?? null, { focus: true, push: false, closing: row.key });
    // The next Take comes under the pointer when the page can scroll that far.
    if (next && take !== undefined && samePlace(placeFrom(next, shown, plan), shown)) {
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
      main.style.minHeight = `${main.offsetHeight}px`;
      later(() => { main.style.minHeight = ''; }, LEAVE_MS + 420);
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
    // From a scrolled full map, the page first goes back to the top, then the map folds: the two at once
    // would move every part up and then down.
    if (isHome && onMap() && scrollY > 0 && !still()) {
      if (lifting.current) return;
      lifting.current = true;
      const from = scrollY;
      const length = Math.min(260, 120 + from / 4);
      const start = performance.now();
      const step = (now: number) => {
        const t = Math.min(1, (now - start) / length);
        scrollTo({ top: from * (1 - (1 - (1 - t) ** 3)), behavior: 'instant' });
        if (t < 1) requestAnimationFrame(step);
        else {
          lifting.current = false;
          openPlace(next);
        }
      };
      requestAnimationFrame(step);
      return;
    }
    if (next.kind !== 'area') opener.current = selectorOf(document.activeElement);
    const title = next.kind === 'area' ? plan.areas.find((group) => group.area.id === next.area)?.area.title : null;
    const hero = next.kind === 'area' ? next.area : next.kind === 'suggested' ? 'suggested' : null;
    travel(() => go(next, null, { push: true, focus: { kind: 'title' }, announce: title ? `${title}, open` : '' }), hero);
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
    const hold = resolved.key !== null && liveStatus.current === 'loading';
    settle(resolved.place, hold ? null : resolved.key);
    if (hold) heldOpen.current = resolved.key;
    focusNext.current = resolved.key && !hold ? { kind: 'reveal', key: resolved.key } : null;
  }, [plan, rowByKey]);
  const liveStatus = useRef(live.status);
  liveStatus.current = live.status;
  const followRef = useRef(follow);
  followRef.current = follow;
  useEffect(() => {
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
    else if (live.status !== 'loading') {
      // A link to something that does not exist falls back to the plan.
      wantedHash.current = null;
      history.replaceState(history.state, '', address());
      settle(HOME, null);
    }
  }, [plan, rowByKey, live.status, follow]);
  useEffect(() => {
    const key = heldOpen.current;
    if (live.status === 'loading' || !key) return;
    heldOpen.current = null;
    if (!rowByKey.has(key)) return;
    setOpenKey(key);
    setOpenKeys((current) => new Set([...current, key]));
    setOpenings((n) => n + 1);
    nav.current.openKey = key;
    nav.current.openedAt = Date.now();
    focusNext.current = { kind: 'reveal', key };
  }, [live.status, rowByKey]);
  // Once it is known that this person cannot decide, the decisions address is dropped.
  useEffect(() => {
    if (place.kind === 'decisions' && live.status !== 'loading' && w.me !== undefined && !canDecide) {
      history.replaceState(history.state, '', address());
      settle(HOME, null);
    }
  }, [place, live.status, w.me, canDecide]);

  // Focus follows what the person asked for; a new area starts at its top, under the folded map.
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
      if (wait) later(reveal, wait);
      else reveal();
    } else if (target.kind === 'title') {
      const top = document.querySelector<HTMLElement>('.ws-top');
      const header = document.querySelector<HTMLElement>('.site-header');
      if (top && mainRef.current) {
        const stop = (onMap() ? top.offsetTop + top.offsetHeight : mainRef.current.offsetTop + top.offsetHeight) - (header?.offsetHeight ?? 0);
        if (scrollY > stop) scrollTo({ top: stop, behavior: still() ? 'instant' : 'smooth' });
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
    else if (row.carried && row.fileVerdict === 'refine') w.setRefining(row.key);
    else if (row.carried && row.fileVerdict) void w.sendVerdict(row, row.fileVerdict);
  };

  const shownRows = rowsOf(plan, shown);
  const keyHandler = useRef<(event: KeyboardEvent) => void>(() => {});
  keyHandler.current = (event: KeyboardEvent) => {
    if (event.key === 'Escape' && !isHome && !isField(event.target)) {
      back();
      return;
    }
    if (event.key !== 'Enter' || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
    if (!canDecide || !openKey || isField(event.target)) return;
    const row = rowByKey.get(openKey);
    if (!row || !needsOwner(row) || !openKeys.has(row.key) || !shownRows.includes(row)) return;
    const target = event.target as HTMLElement | null;
    const item = document.getElementById(openKey);
    // Other buttons and links in the row keep their own Enter.
    const control = target?.closest('button, a, summary');
    const onRow = target === document.body || target === document.documentElement
      || (item !== null && target !== null && item.contains(target) && (!control || control.classList.contains('ws-row')));
    if (!onRow) return;
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
    armed: (row) => row.key === openKey && Date.now() - nav.current.openedAt >= OPEN_GUARD_MS,
    // A verdict button of an open row that is not the last opened makes it the last opened, and arms after the guard.
    makeCurrent: (row) => { if (row.key !== openKey) openRow(row.key, { push: false }); },
  };
  const account = !w.liveOk || w.me === undefined ? null : w.me ? (
    <span className="ws-account">
      <span className="ws-login" title={w.me.login}>{w.me.login}</span>
      <button type="button" className="ws-link" onClick={() => void w.signOut()}>Sign out</button>
    </span>
  ) : (
    <a className="button small" href={signIn}><span>Sign in{' '}<span className="ws-wide">with GitHub</span></span></a>
  );

  const notes = (
    <>
      {live.status === 'unavailable' ? <p className="ws-quiet ws-note">Votes and comments are offline.</p> : null}
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

  const classes = ['ws', isHome ? 'is-sheet' : 'is-open', `v-${view}`, leaving ? 'leaving' : '', swap ? `swap-${swap}` : '', boot ? 'ws-boot' : '', canDecide ? 'ws-owner' : ''].filter(Boolean).join(' ');
  return (
    <>
      {accountSlot ? createPortal(account, accountSlot) : <div className="ws-account-inline">{account}</div>}
      <div className={classes} ref={mainRef}>
        <section className="ws-top joint">
          <div className="ws-head">
            <h1>Workshop</h1>
            <p className="ws-status">{statusLine(plan.areas)}</p>
          </div>
          <div className="ws-top-side">
            <div className="ws-tools">
              {canDecide && firstToDecide ? (
                <a className="ws-loglink" href={formatHash(placeOf(firstToDecide), firstToDecide.key)} onClick={(event) => { event.preventDefault(); openRow(firstToDecide.key, { focus: true }); }}>
                  <b>{totalToDecide} to decide</b>
                </a>
              ) : null}
              {canDecide ? (
                <a className="ws-loglink" href="#decisions" aria-current={shown.kind === 'decisions' ? 'page' : undefined} onClick={link({ kind: 'decisions' })}>
                  Decisions{plan.openDecisions.length ? <>{' · '}<b>{plan.openDecisions.length} open</b></> : null}
                </a>
              ) : null}
              <div className="ws-seg" role="group" aria-label="Arrangement">
                <button type="button" aria-pressed={view === 'map'} onClick={() => setView('map')}>Map</button>
                <button type="button" aria-pressed={view === 'order'} onClick={() => setView('order')}>Order</button>
              </div>
            </div>
            <ul className="ws-key" aria-label="Key">
              {KEY.filter(([state]) => present.has(state)).map(([state, mark, label]) => <li key={state}><i className={mark} />{label}</li>)}
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
              {suggested ? <a className="ws-fold-link" href="#suggested" onClick={link({ kind: 'suggested' })}>{suggested} suggested</a> : null}
              {notes}
            </div>
            {w.liveOk && w.me !== undefined ? <a className="ws-btn ws-suggest" href="#suggested" onClick={link({ kind: 'suggested' })}>Suggest a feature</a> : null}
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
