// The Workshop: the drawing of Silo (tiles on narrow screens), an opened area under it, and one entry open
// in place. Addresses follow routes.ts; the live state and every write live in useWorkshop.ts.
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import { createPortal } from 'react-dom';
import { loginUrl } from './api';
import { AreaPlace, LeftPlace } from './AreaView';
import { DecisionsPlace } from './Decisions';
import type { EntryContext } from './Entry';
import { MapView } from './MapView';
import { FOLD_HEIGHT, MAP_WIDTH } from './map';
import { awaitsVerdict, statusLine } from './plan';
import type { AreaState, BuildData, Row } from './plan';
import { formatHash, HOME, placeOf, resolveHash, rowsOf, samePlace } from './routes';
import type { Place } from './routes';
import { SuggestPlace } from './Suggest';
import { Tiles } from './Tiles';
import { useWorkshop } from './useWorkshop';
import type { Nav } from './useWorkshop';

export { OPEN_GUARD_MS } from './useWorkshop';

export interface AppProps {
  data: BuildData;
  /** The element in the site header that holds sign-in and the signed-in login. */
  accountSlot?: Element | null;
}

type View = 'map' | 'order';
type Focus = { kind: 'title' } | { kind: 'area'; area: string } | { kind: 'row' | 'reveal'; key: string };

const LEAVE_MS = 140;
const SWAP_MS = 130;
const KEY: [AreaState, string, string][] = [
  ['done', 'ws-pip built', 'done'], ['now', 'ws-pip building', 'building now'], ['next', 'ws-pip next', 'next'],
  ['planned', 'ws-pip planned', 'planned'], ['open', 'ws-dash', 'not decided yet'],
];

function media(query: string, fallback: boolean): boolean {
  return typeof matchMedia === 'function' ? matchMedia(query).matches : fallback;
}
const still = () => media('(prefers-reduced-motion: reduce)', true);
const wide = () => media('(min-width: 1040px)', true);

function isField(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return target.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(target.tagName);
}

const initialView = (): View => (new URLSearchParams(location.search).get('view') === 'order' ? 'order' : 'map');
const address = (hash = '') => `${location.pathname}${location.search}${hash}`;

export function App({ data, accountSlot }: AppProps) {
  const [place, setPlace] = useState<Place>(HOME);
  const [openKey, setOpenKey] = useState<string | null>(null);
  const [view, setViewState] = useState<View>(initialView);
  const [leaving, setLeaving] = useState(false);
  const [swap, setSwap] = useState<'out' | 'in' | null>(null);
  // The first frames take the state as it is, without animating into it.
  const [boot, setBoot] = useState(true);
  const nav = useRef<Nav>({ openKey: null, openedAt: 0, advanceFrom: () => {}, reopen: () => {} });
  const w = useWorkshop(data, openKey, nav);
  const { plan, rowByKey, live, canDecide } = w;
  const focusNext = useRef<Focus | null>(null);
  const wantedHash = useRef<string | null>(null);
  const timers = useRef<ReturnType<typeof setTimeout>[]>([]);
  const mainRef = useRef<HTMLDivElement>(null);
  // A row that opens below the open one keeps its place on screen: the one above closes at once.
  const anchor = useRef<{ key: string; top: number } | null>(null);
  const [snap, setSnap] = useState<string | null>(null);
  useEffect(() => () => timers.current.forEach(clearTimeout), []);
  useEffect(() => {
    let frame = requestAnimationFrame(() => { frame = requestAnimationFrame(() => setBoot(false)); });
    return () => cancelAnimationFrame(frame);
  }, []);
  const later = (fn: () => void, ms: number) => { timers.current.push(setTimeout(fn, ms)); };

  // Visitors never see the decisions: the address shows them the plan.
  const shown: Place = place.kind === 'decisions' && !canDecide ? HOME : place;
  const isHome = shown.kind === 'home';

  const waiting = useCallback((row: Row) => awaitsVerdict(row) && row.issue !== null && Boolean(live.state?.issues[row.issue]), [live.state]);
  const toDecide = useMemo(() => new Map(canDecide ? plan.areas.map((group) => [group.area.id, group.rows.filter(waiting).length]) : []), [canDecide, plan, waiting]);
  const totalToDecide = [...toDecide.values()].reduce((sum, n) => sum + n, 0);
  const byBuildOrder = useMemo(() => plan.areas.flatMap((group) => group.rows).sort((a, b) => (a.n ?? 0) - (b.n ?? 0)), [plan]);
  const free = (row: Row) => waiting(row) && !w.pendingRef.current.has(row.issue as number);

  function settle(next: Place, key: string | null) {
    timers.current.forEach(clearTimeout);
    timers.current = [];
    setLeaving(false);
    setPlace(next);
    setOpenKey(key);
    nav.current.openKey = key;
    nav.current.openedAt = Date.now();
  }

  function go(next: Place, key: string | null, options: { push?: boolean; focus?: Focus | null; announce?: string } = {}) {
    const hash = formatHash(next, key);
    if (hash !== location.hash) {
      if (options.push) history.pushState(null, '', address(hash));
      else history.replaceState(null, '', address(hash));
    }
    settle(next, key);
    w.setRefining(null);
    w.setRowNotice((current) => (current?.key === key ? current : null));
    if (options.announce !== undefined) w.setAnnounce(options.announce);
    focusNext.current = options.focus ?? null;
  }

  /** Opens a row where it lives, unfolding its area when it is elsewhere. */
  function openRow(key: string | null, options: { focus?: boolean; push?: boolean } = {}) {
    const row = key ? rowByKey.get(key) : undefined;
    if (!row) {
      go(shown, null);
      return;
    }
    const target = placeOf(row);
    const moving = !samePlace(target, shown);
    const before = openKey ? document.getElementById(openKey) : null;
    const after = document.getElementById(row.key);
    const collapse = before?.querySelector<HTMLElement>('.ws-more')?.offsetHeight ?? 0;
    if (!moving && before && after && before !== after && before.compareDocumentPosition(after) & Node.DOCUMENT_POSITION_FOLLOWING && collapse <= scrollY) {
      anchor.current = { key: row.key, top: after.getBoundingClientRect().top };
      setSnap(openKey);
    }
    go(target, row.key, { push: moving && options.push !== false, announce: `${row.name}, open`, focus: options.focus ? { kind: 'row', key: row.key } : null });
  }

  nav.current.advanceFrom = (row: Row) => {
    const group = rowsOf(plan, placeOf(row));
    const at = group.findIndex((item) => item.key === row.key);
    const ordered = at === -1 ? group : [...group.slice(at + 1), ...group.slice(0, at)];
    const fresh = (item: Row) => item.key !== row.key && free(item);
    const next = ordered.find(fresh) ?? byBuildOrder.find(fresh);
    if (next) openRow(next.key, { focus: true, push: false });
    else openRow(null);
  };
  nav.current.reopen = (row: Row) => openRow(row.key, { focus: true, push: false });

  function back() {
    if (isHome) return;
    const from = shown.kind === 'area' ? shown.area : shown.kind === 'suggested' ? 'suggested' : null;
    const finish = () => go(HOME, null, { push: true, focus: from ? { kind: 'area', area: from } : null, announce: '' });
    if (still()) {
      finish();
      return;
    }
    setLeaving(true);
    later(finish, LEAVE_MS);
  }

  function openPlace(next: Place) {
    if (samePlace(next, shown)) {
      back();
      return;
    }
    const title = next.kind === 'area' ? plan.areas.find((group) => group.area.id === next.area)?.area.title : null;
    go(next, null, { push: true, focus: { kind: 'title' }, announce: title ? `${title}, open` : '' });
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
    settle(resolved.place, resolved.key);
    focusNext.current = resolved.key ? { kind: 'reveal', key: resolved.key } : null;
  }, [plan, rowByKey]);
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
  // Once it is known that this person cannot decide, the decisions address is dropped.
  useEffect(() => {
    if (place.kind === 'decisions' && live.status !== 'loading' && w.me !== undefined && !canDecide) {
      history.replaceState(history.state, '', address());
      settle(HOME, null);
    }
  }, [place, live.status, w.me, canDecide]);

  useLayoutEffect(() => {
    const held = anchor.current;
    if (!held) return;
    anchor.current = null;
    const node = document.getElementById(held.key);
    const shift = node ? node.getBoundingClientRect().top - held.top : 0;
    if (shift) scrollTo({ top: scrollY + shift, behavior: 'instant' });
    setSnap(null);
  });

  // Focus follows what the person asked for; a new area starts at its top, under the folded map.
  useLayoutEffect(() => {
    const target = focusNext.current;
    if (!target) return;
    focusNext.current = null;
    const onMap = view === 'map' && wide();
    if (target.kind === 'row' || target.kind === 'reveal') {
      const item = document.getElementById(target.key);
      if (target.kind === 'row') item?.querySelector<HTMLElement>('.ws-row')?.focus({ preventScroll: true });
      item?.scrollIntoView?.({ block: 'nearest' });
    } else if (target.kind === 'title') {
      const top = document.querySelector<HTMLElement>('.ws-top');
      const header = document.querySelector<HTMLElement>('.site-header');
      if (top && mainRef.current) {
        const stop = (onMap ? top.offsetTop + top.offsetHeight : mainRef.current.offsetTop + top.offsetHeight) - (header?.offsetHeight ?? 0);
        if (scrollY > stop) scrollTo({ top: stop, behavior: 'instant' });
      }
      document.getElementById('ws-area-title')?.focus({ preventScroll: true });
    } else if (target.kind === 'area') {
      const selector = onMap ? `.ws-part[data-part="${target.area}"]` : `.ws-tile[data-area="${target.area}"] .ws-tile-hit`;
      document.querySelector<HTMLElement>(selector)?.focus({ preventScroll: true });
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
    if (still() || !media('(min-width: 1040px)', false)) {
      apply();
      return;
    }
    setSwap('out');
    later(() => {
      apply();
      setSwap('in');
      later(() => setSwap(null), 300);
    }, SWAP_MS);
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
    if (!row || !(awaitsVerdict(row) || row.carried) || !shownRows.includes(row)) return;
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
  const ctx: EntryContext = { w, signIn, mainAction, snap, onToggle: (row) => openRow(openKey === row.key ? null : row.key) };
  const account = !w.liveOk || w.me === undefined ? null : w.me ? (
    <span className="ws-account">
      <span className="ws-login">{w.me.login}</span>
      <button type="button" className="ws-link" onClick={() => void w.signOut()}>Sign out</button>
    </span>
  ) : (
    <a className="button small" href={signIn}><span>Sign in{' '}<span className="ws-wide">with GitHub</span></span></a>
  );

  const notes = (
    <>
      {live.status === 'unavailable' ? <p className="ws-quiet ws-note">Live comments are unavailable.</p> : null}
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
    if (group) detail = <AreaPlace group={group} toDecide={toDecide.get(group.area.id) ?? 0} openKey={openKey} ctx={ctx} onBack={back} notes={notes} />;
  } else if (shown.kind === 'suggested') detail = <SuggestPlace openKey={openKey} ctx={ctx} onBack={back} notes={notes} />;
  else if (shown.kind === 'left') detail = <LeftPlace rows={plan.left} openKey={openKey} ctx={ctx} onBack={back} notes={notes} />;
  else if (shown.kind === 'decisions') detail = <DecisionsPlace openKey={openKey} ctx={ctx} onBack={back} notes={notes} />;

  const classes = ['ws', isHome ? 'is-sheet' : 'is-open', `v-${view}`, leaving ? 'leaving' : '', swap ? `swap-${swap}` : '', boot ? 'ws-boot' : ''].filter(Boolean).join(' ');
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
                  Decisions{plan.openDecisions.length ? <>{' '}<b>{plan.openDecisions.length} to decide</b></> : null}
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
              {suggested ? <a className="ws-fold-link" href="#suggested" onClick={link({ kind: 'suggested' })}>{suggested} suggested</a> : null}
              {plan.left.length ? <a className="ws-fold-link" href="#left" onClick={link({ kind: 'left' })}>Not carried over · {plan.left.length}</a> : null}
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
