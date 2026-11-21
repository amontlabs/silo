#!/usr/bin/env node
// Loads the Workshop in a real browser at several window sizes and places, and checks what a first visit shows.
//   node scripts/load-check.mjs [url] [--slow-api <ms>] [--slow-script <ms>] [--signed-in]
// The url defaults to the `workshop:dev --fake` server. --slow-api and --slow-script hold back the API's answers
// and the page's script; --signed-in fakes the `me` answer as a signed-in person.
// Each run prints when the page first painted, when each late item appeared, what the page's first frame held
// and every layout shift, and checks the footer rule before the script has run and once everything has loaded.
// Then come the visits that need a browser profile: a second visit, which draws what the first one showed in
// its first frame, and a person signed in through the fake GitHub's page (the real session marker), whose
// second visit has the login in its first frame.
// Last, with the page's script held, what the build drew into the page: the plain view in the first painted
// frame and the script taking over without a move; at a part's address, the plain view never painted; on a
// second visit, the remembered state drawn at the takeover without moving anything under the title row.
// What the page must offer is read from its data (window.__workshopData, set by src/workshop/main.tsx): its parts,
// the whole product's rows; whether it draws a map is read from the page.
// The places with rows are chosen from the real counts of rows (placesOf): the first part that has rows and the whole
// product when it has rows. A
// plan with no part is checked at the places it has: the plain view, the whole product when it has rows, and the
// suggestions.
// It exits non-zero when the footer rule fails, a first frame lacks what it must hold, or a run's layout shifts
// add up to more than SHIFT_BUDGET.
// It drives Playwright's own browser: `npx playwright-core install chromium-headless-shell` once.
import { chromium } from 'playwright-core';

const SIZES = [[1440, 900], [1440, 1100], [2560, 1440], [1024, 1366], [390, 844]];
const LATE = {
  account: '#workshop-account > :not(.ws-ph)',
  votes: '.ws-vote',
  'Suggest a feature': '.ws-suggest',
};
// Room for rounding between boxes, in CSS pixels.
const SLACK = 1;
/** A run fails when its layout shifts add up to more than this (the key gains an item when the state arrives). */
const SHIFT_BUDGET = 0.01;

function parseArgs(argv) {
  const options = { url: 'http://127.0.0.1:4174/workshop', slowApi: 0, slowScript: 0, signedIn: false };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--slow-api') options.slowApi = Number(argv[(i += 1)]);
    else if (arg === '--slow-script') options.slowScript = Number(argv[(i += 1)]);
    else if (arg === '--signed-in') options.signedIn = true;
    else if (!arg.startsWith('--')) options.url = arg;
    else throw new Error(`Unknown option ${arg}`);
  }
  if (!Number.isFinite(options.slowApi) || !Number.isFinite(options.slowScript)) throw new Error('--slow-api and --slow-script take milliseconds');
  return options;
}

const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

/** Records, in the page, when each late item first appears and every layout shift. */
function recordTimeline(late) {
  const seen = {};
  const shifts = [];
  // What the app's first frame holds: read when its first render is in the document, before it is painted. The
  // markup the build drew (marked data-prerendered until the script takes over) is not the app's render.
  let first = null;
  const frame = () => {
    const rows = [...document.querySelectorAll('.ws-detail .ws-item')].filter((row) => row.querySelector('.ws-votes'));
    return {
      at: Math.round(performance.now()),
      signIn: Boolean(document.querySelector('#workshop-account a.button')),
      suggest: Boolean(document.querySelector('.ws-suggest')),
      rows: rows.length,
      rowsWithPlaceholder: rows.filter((row) => row.querySelector('.ws-votes .ws-ph-vote')).length,
      votes: document.querySelectorAll('.ws-vote').length,
      login: document.querySelector('#workshop-account .ws-login')?.textContent ?? null,
    };
  };
  const check = () => {
    if (!first && document.querySelector('.ws') && !document.querySelector('[data-prerendered]')) first = frame();
    window.__loadCheck.first = first;
    for (const [name, selector] of Object.entries(late)) {
      if (seen[name] === undefined && document.querySelector(selector)) seen[name] = Math.round(performance.now());
    }
  };
  new MutationObserver(check).observe(document, { childList: true, subtree: true, attributes: true });
  new PerformanceObserver((list) => {
    for (const entry of list.getEntries()) {
      if (entry.hadRecentInput) continue;
      const nodes = entry.sources.map((source) => {
        const node = source.node;
        if (!node) return '?';
        return node.id ? `#${node.id}` : typeof node.className === 'string' && node.className ? `.${node.className.split(' ')[0]}` : node.nodeName.toLowerCase();
      });
      shifts.push({ at: Math.round(entry.startTime), value: Number(entry.value.toFixed(4)), nodes });
    }
  }).observe({ type: 'layout-shift', buffered: true });
  window.__loadCheck = { seen, shifts, first };
}

/**
 * The footer rule: when the content is shorter than the window, the frame ends at the bottom of the window with
 * the footer as its last row; when it is taller, the footer follows the content. The list's left border, where
 * there is one, runs down to the footer. Returns the problems found, none when the rule holds.
 */
function footerRule(slack) {
  const problems = [];
  const shell = document.querySelector('.page-shell');
  const main = document.querySelector('.page-shell > main');
  const foot = document.querySelector('.ws-foot');
  if (!shell || !main || !foot) return ['the page has no frame, main or footer'];
  const box = (node) => {
    const rect = node.getBoundingClientRect();
    return { top: rect.top + scrollY, bottom: rect.bottom + scrollY };
  };
  // The content's own height: the frame's height with nothing stretching it to the window, measured and undone
  // in one task, so no frame is painted in between.
  const probe = document.createElement('style');
  probe.textContent = '.page-shell, .page-shell > main, #workshop-root, .ws, .ws-detail { min-height: 0 !important; }';
  document.head.append(probe);
  const natural = box(shell).bottom;
  probe.remove();
  const view = innerHeight;
  const frame = box(shell);
  const footer = box(foot);
  const page = document.documentElement.scrollHeight;
  const pad = parseFloat(getComputedStyle(shell).paddingBottom);
  if (Math.abs(footer.top - box(main).bottom) > slack) problems.push(`the footer starts at ${Math.round(footer.top)}, not under the content (${Math.round(box(main).bottom)})`);
  if (Math.abs(frame.bottom - pad - footer.bottom) > slack) problems.push(`the frame goes on ${Math.round(frame.bottom - pad - footer.bottom)} px under the footer`);
  if (natural < view) {
    if (Math.abs(frame.bottom - view) > slack) problems.push(`short page: the frame ends at ${Math.round(frame.bottom)}, the window at ${view}`);
    if (page > view + slack) problems.push(`short page: the page is ${page} px tall in a ${view} px window`);
  } else if (Math.abs(frame.bottom - natural) > slack) {
    problems.push(`tall page: the frame ends at ${Math.round(frame.bottom)}, ${Math.round(frame.bottom - natural)} px past its content`);
  }
  const list = document.querySelector('.ws-detail .ws-list');
  if (list && parseFloat(getComputedStyle(list).borderLeftWidth) > 0 && Math.abs(box(list).bottom - footer.top) > slack) {
    problems.push(`the list's border stops ${Math.round(footer.top - box(list).bottom)} px above the footer`);
  }
  return problems;
}

/** Holds every script but the theme's until `release` is called, then for `delay` more. */
async function holdScripts(page, delay) {
  let release;
  const gate = new Promise((resolve) => { release = resolve; });
  await page.route((url) => /\.(m?js|tsx?)(\?|$)/.test(url.pathname) && url.pathname !== '/theme.js', async (route) => {
    await gate;
    if (delay) await wait(delay);
    await route.continue();
  });
  return release;
}

async function slowApi(page, delay, signedIn) {
  await page.route('**/api/workshop/**', async (route) => {
    if (delay) await wait(delay);
    if (signedIn && new URL(route.request().url()).pathname === '/api/workshop/me') {
      await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ login: 'signed-in', avatar: '' }) });
    } else await route.continue();
  });
}

/** One load of one place at one size: the rule before the script runs, then the timeline and the rule after. */
async function run(browser, options, [width, height], place, shared = null) {
  const context = shared ?? await browser.newContext({ viewport: { width, height }, deviceScaleFactor: 1, reducedMotion: 'no-preference' });
  if (options.signedIn && !shared) await context.addCookies([{ name: 'workshop-hint', value: '1', url: options.url }]);
  const page = await context.newPage();
  await page.addInitScript(recordTimeline, LATE);
  await slowApi(page, options.slowApi, options.signedIn);
  const release = await holdScripts(page, options.slowScript);
  await page.goto(options.url + place.hash, { waitUntil: 'commit' });
  // The page is parsed and styled; the held script has not run.
  await page.waitForFunction(() => document.readyState !== 'loading' && [...document.querySelectorAll('link[rel=stylesheet]')].every((link) => link.sheet));
  const before = await page.evaluate(footerRule, SLACK);
  release();
  await page.waitForLoadState('load');
  await page.waitForSelector('.ws');
  // A visit can act before the answers (open an entry), then watch a box that must not move when they arrive.
  if (place.act) await place.act(page);
  const top = (selector) => page.evaluate((sel) => {
    const nodes = document.querySelectorAll(sel);
    const node = nodes[nodes.length - 1];
    return node ? node.getBoundingClientRect().top + scrollY : null;
  }, selector);
  const watchedBefore = place.watch ? await top(place.watch) : null;
  await page.waitForLoadState('networkidle');
  // Late answers and the transitions they start.
  await wait(1200);
  const watchedAfter = place.watch ? await top(place.watch) : null;
  const after = await page.evaluate(footerRule, SLACK);
  const timeline = await page.evaluate(() => ({
    ...window.__loadCheck,
    paint: Math.round(performance.getEntriesByName('first-paint')[0]?.startTime ?? NaN),
  }));
  if (shared) await page.close();
  else await context.close();
  const moved = place.watch ? (watchedBefore === null || watchedAfter === null ? NaN : Math.round(watchedAfter - watchedBefore)) : null;
  return { size: `${width}x${height}`, place: place.name, before, after, moved, watch: place.watch, ...timeline };
}

/** What a first frame must hold, by the kind of visit; returns the problems. */
function firstFrame(first, wants) {
  if (!first) return ['the page never rendered'];
  const problems = [];
  if (wants.signIn && !first.signIn) problems.push('no sign-in button');
  if (wants.suggest && !first.suggest) problems.push('no "Suggest a feature"');
  if (wants.placeholders && (first.rows === 0 || first.rowsWithPlaceholder !== first.rows)) problems.push(`${first.rowsWithPlaceholder} of ${first.rows} rows have a vote placeholder`);
  if (wants.votes && first.votes === 0) problems.push('no votes');
  if (wants.login && first.login !== wants.login) problems.push(`login ${JSON.stringify(first.login)}, not ${wants.login}`);
  return problems;
}

function report(result, wants = {}) {
  const late = Object.keys(LATE).map((name) => `${name} ${result.seen[name] === undefined ? '—' : `${result.seen[name]} ms`}`).join(', ');
  const rule = (problems) => (problems.length ? `FAIL: ${problems.join('; ')}` : 'ok');
  const first = result.first;
  const held = first ? [
    first.signIn ? 'sign-in' : null, first.suggest ? 'Suggest a feature' : null,
    first.rows ? `${first.rowsWithPlaceholder}/${first.rows} rows with a vote placeholder` : null,
    first.votes ? `${first.votes} votes` : null, first.login ? `login ${first.login}` : null,
  ].filter(Boolean).join(', ') || 'nothing live' : '—';
  const frame = firstFrame(first, wants);
  const total = result.shifts.reduce((sum, shift) => sum + shift.value, 0);
  const shifted = total > SHIFT_BUDGET;
  const moved = result.moved !== null && !(Math.abs(result.moved) <= SLACK);
  const lines = [
    `${result.size.padEnd(10)} ${result.place}`,
    `  first paint ${result.paint} ms; ${late}`,
    `  first frame (${first?.at ?? '—'} ms): ${held}${frame.length ? ` — FAIL: ${frame.join('; ')}` : ''}`,
    ...(result.moved !== null ? [`  ${result.watch} moved ${result.moved} px when the answers came${moved ? ' — FAIL' : ''}`] : []),
    `  footer before the script: ${rule(result.before)}`,
    `  footer after loading: ${rule(result.after)}`,
    `  layout shifts: ${result.shifts.length ? `${result.shifts.map((shift) => `${shift.at} ms ${shift.value} (${shift.nodes.join(' ')})`).join(', ')}; total ${Number(total.toFixed(4))}${shifted ? ` — FAIL: over ${SHIFT_BUDGET}` : ''}` : 'none'}`,
  ];
  console.log(lines.join('\n'));
  return !result.before.length && !result.after.length && !frame.length && !shifted && !moved;
}

/** Signs in through the fake GitHub's page as `login`, in this context; the page sets the real session cookies. */
async function signIn(context, url, login) {
  const page = await context.newPage();
  const origin = new URL(url).origin;
  await page.goto(`${origin}/api/workshop/login?return=${encodeURIComponent('/workshop')}`);
  await page.getByText(`Sign in as ${login}`).click();
  await page.waitForURL((next) => next.origin === origin && next.pathname === '/workshop', { timeout: 30000 });
  await page.waitForSelector('.ws-login');
  // The visit ends once the state is shown, so the next one remembers it.
  await page.waitForSelector('.ws[data-live="answered"]', { timeout: 30000 });
  await page.waitForLoadState('networkidle');
  await page.close();
}

/**
 * The visits that need a browser profile: a second visit, and the second visit of a person signed in, at `rows`
 * (the first place with entries' rows), or at the plain view when the plan has none.
 */
async function visits(browser, options, rows, page$) {
  const place = rows ?? { name: 'plain view', hash: '' };
  const size = [1440, 900];
  const context = () => browser.newContext({ viewport: { width: size[0], height: size[1] }, deviceScaleFactor: 1, reducedMotion: 'no-preference' });
  const plain = { ...options, signedIn: false };
  let passed = true;
  console.log('\nvisits in one browser profile');
  const visitor = await context();
  passed = report({ ...(await run(browser, plain, size, { ...place, name: `${place.name}, first visit` }, visitor)) }, { signIn: true, placeholders: Boolean(rows) }) && passed;
  passed = report({ ...(await run(browser, plain, size, { ...place, name: `${place.name}, second visit` }, visitor)) }, { signIn: true, votes: Boolean(rows) }) && passed;
  await visitor.close();
  const person = await context();
  await signIn(person, options.url, 'owner-fake');
  passed = report({ ...(await run(browser, plain, size, { ...place, name: `${place.name}, owner-fake signed in, second visit` }, person)) }, { votes: Boolean(rows), login: 'owner-fake' }) && passed;
  await person.close();

  // An entry of the first place with rows opened before the answers: the rows below it do not move when they arrive. The
  // entry has no comments: their number comes with the state, and their lines are not held before it.
  const live = await (await fetch(`${new URL(options.url).origin}/api/workshop/state`)).json();
  // An entry with no thread yet has none either.
  // The thread of an entry is what `state.threads` names: a suggestion's issue too, for a feature written from it.
  const talked = Object.entries(live.threads ?? {}).filter(([, n]) => live.issues?.[n]?.comments > 0).map(([entry]) => `entry-${entry}`);
  for (const narrow of rows ? [false, true] : []) {
    const view = narrow ? [390, 844] : size;
    const fresh = await browser.newContext({ viewport: { width: view[0], height: view[1] }, deviceScaleFactor: 1, reducedMotion: 'no-preference' });
    let opened = null;
    const open = async (page) => {
      opened = await page.evaluate((ids) => [...document.querySelectorAll('.ws-detail .ws-item')].map((item) => item.id).find((id) => !ids.includes(id)) ?? null, talked);
      if (!opened) throw new Error(`${place.name} has no entry without comments`);
      await page.click(`#${opened} .ws-row`);
      await wait(500);
    };
    const result = await run(browser, plain, view, { ...place, name: `${place.name}, an entry opened before the answers`, act: open, watch: '.ws-detail .ws-item' }, fresh);
    passed = report({ ...result, place: `${result.place} (${opened})` }) && passed;
    await fresh.close();
  }

  // #suggested with the session marker and nobody remembered: the list does not move when `me` answers.
  const marked = await context();
  await signIn(marked, options.url, 'owner-fake');
  const forget = await marked.newPage();
  // A page of the same origin that is not the Workshop, so nothing remembers the person again.
  await forget.goto(`${new URL(options.url).origin}/favicon.svg`);
  await forget.evaluate(() => localStorage.removeItem('silo-workshop-me'));
  await forget.close();
  passed = report({ ...(await run(browser, plain, size, { name: '#suggested, signed in, nobody remembered', hash: '#suggested', watch: 'section[aria-label="Suggested by people"]' }, marked)) }) && passed;
  await marked.close();
  return passed;
}

/**
 * The places to visit, chosen from the page's data (window.__workshopData): `parts` are the parts in order, `entries`
 * the rows, each with its part's id or null for the whole product. Returns `tile`, the first part's id (its tile is drawn whether or not it has rows), or null
 * without parts; `part`, the first part that has rows; `product`, the whole product when it has rows; `rows`, the
 * first of these two (the place that shows entries' rows), or null when the plan has none; and `places`: the plain
 * view, those two, Latest and the suggestions. A part without rows is not visited as a place with rows, so its
 * correct render of zero rows is not refused.
 */
export function placesOf({ parts, entries }) {
  const count = (id) => entries.filter((entry) => entry.part === id).length;
  const withRows = parts.find((part) => count(part.id) > 0);
  const part = withRows ? { name: `first part with rows (#${withRows.id})`, hash: `#${withRows.id}`, rows: true } : null;
  const product = count(null) ? { name: '#product', hash: '#product', rows: true } : null;
  return {
    tile: parts.length ? parts[0].id : null,
    part,
    product,
    rows: part ?? product,
    places: [
      { name: 'plain view', hash: '' },
      ...(part ? [part] : []),
      ...(product ? [product] : []),
      { name: '#latest', hash: '#latest' },
      { name: '#suggested', hash: '#suggested' },
    ],
  };
}

/** What the page offers: the places `placesOf` chooses from the page's data, and whether its map shows at 1440 x 900, where the checks that use it run. */
async function pageOf(browser, url) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const page = await context.newPage();
  await page.goto(url);
  await page.waitForFunction(() => window.__workshopData, null, { timeout: 15000 });
  const data = await page.evaluate(() => {
    const { parts, entries } = window.__workshopData;
    return { parts: parts.map((part) => ({ id: part.id })), entries: entries.map((entry) => ({ part: entry.part })) };
  });
  await page.waitForSelector('#workshop-root:not([data-prerendered]) .ws', { timeout: 15000 });
  await page.waitForLoadState('networkidle');
  // Whether the map shows at this url, not whether it is in the document: in Order, CSS hides it.
  const mapped = await page.locator('.ws-stage').first().isVisible();
  await context.close();
  return { mapped, ...placesOf(data) };
}

/**
 * Opening the first part from the map (or its tile, without a map) and going back never moves the page sideways, as
 * a scroll bar coming or going would: the frame's left edge is sampled on every frame, at 1440x900.
 */
async function sideways(browser, url, part, mapped) {
  const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
  const page = await context.newPage();
  await page.goto(url);
  await page.waitForLoadState('networkidle');
  await wait(800);
  await page.evaluate(() => {
    const lefts = [];
    const sample = () => {
      lefts.push(Math.round(document.querySelector('.page-shell').getBoundingClientRect().left));
      requestAnimationFrame(sample);
    };
    sample();
    window.__lefts = lefts;
  });
  const left = () => page.evaluate(() => Math.round(document.querySelector('.page-shell').getBoundingClientRect().left));
  const plain = await left();
  // A map's part is pressed on its title, near its top left corner: a part drawn inside another covers the middle of that one.
  if (mapped) await page.locator(`.ws-part[data-part="${part}"]`).click({ position: { x: 12, y: 12 } });
  else await page.locator(`.ws-tile[data-part="${part}"] .ws-tile-hit`).click();
  await wait(1500);
  const opened = await left();
  await page.locator('.ws-back').click();
  await wait(1500);
  const back = await left();
  const seen = [...new Set(await page.evaluate(() => window.__lefts))];
  await context.close();
  const passed = plain === opened && opened === back && seen.length === 1;
  console.log(`1440x900   open #${part} and go back: frame's left edge ${plain} px, opened ${opened} px, back ${back} px, on every frame ${seen.join(' / ')} px: ${passed ? 'ok' : 'FAIL: the page moved sideways'}`);
  return passed;
}

/**
 * Records, on every frame from the first one shown (fonts in) until the script has taken over: whether the
 * markup the build drew is still there, whether the plain map, sign-in and an opened part are visible, and the
 * boxes of the title row, its places, the map or the tiles, the footer and the account slot.
 */
function recordFrames() {
  const state = { firstShown: null, lastDrawn: null, firstMounted: null, plainView: [] };
  const visible = (node) => {
    if (!node) return false;
    const box = node.getBoundingClientRect();
    return box.width > 0 && box.height > 0 && getComputedStyle(node).visibility === 'visible';
  };
  // To the hundredth of a pixel, so a move of half a pixel shows.
  const boxes = () => Object.fromEntries(['.ws-top', '.ws-places', '.ws-swap-map', '.ws-swap-order', '.ws-foot', '#workshop-account'].map((selector) => {
    const box = document.querySelector(selector)?.getBoundingClientRect();
    return [selector, box ? [box.top + scrollY, box.left, box.width, box.height].map((value) => Math.round(value * 100) / 100) : null];
  }));
  const step = () => {
    if (document.body && !document.documentElement.classList.contains('fonts-pending')) {
      const frame = {
        at: Math.round(performance.now()),
        drawn: Boolean(document.querySelector('#workshop-root[data-prerendered]')),
        map: visible(document.querySelector('.ws.is-sheet .ws-stage')),
        tiles: visible(document.querySelector('.ws.is-sheet .ws-board')),
        signIn: visible(document.querySelector('#workshop-account a.button')),
        part: visible(document.querySelector('.ws-detail')),
        boxes: boxes(),
      };
      state.firstShown ??= frame;
      if (frame.map || frame.tiles) state.plainView.push(frame.at);
      if (frame.drawn) state.lastDrawn = frame;
      else state.firstMounted ??= frame;
    }
    if (!state.firstMounted) requestAnimationFrame(step);
  };
  requestAnimationFrame(step);
  window.__frames = state;
}

/**
 * One load with the script held (1440x900 unless a context is given): the frames until it has taken over, the
 * footer rule meanwhile.
 */
async function drawn(browser, options, place, shared = null) {
  const context = shared ?? await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1, reducedMotion: 'no-preference' });
  const page = await context.newPage();
  await page.addInitScript(recordTimeline, LATE);
  await page.addInitScript(recordFrames);
  // The state answers well after the takeover, so a shift then is the takeover's (the key gains an item with it).
  await slowApi(page, Math.max(options.slowApi, options.slowScript + 2000), false);
  const release = await holdScripts(page, options.slowScript);
  await page.goto(options.url + place.hash, { waitUntil: 'commit' });
  await page.waitForFunction(() => window.__frames?.firstShown);
  const prerendered = await page.evaluate(() => Boolean(document.querySelector('#workshop-root[data-prerendered]')));
  const before = await page.evaluate(footerRule, SLACK);
  release();
  await page.waitForFunction(() => window.__frames.firstMounted, null, { timeout: 30000 });
  await wait(300);
  const result = await page.evaluate(() => ({ ...window.__frames, shifts: window.__loadCheck.shifts }));
  if (shared) await page.close();
  else await context.close();
  return { prerendered, before, ...result };
}

/** A box that moves or resizes by half a pixel or more counts. */
const HALF = 0.5;
const boxMoves = (from, to, only = Object.keys(from)) => only.filter((selector) => {
  const [a, b] = [from[selector], to[selector]];
  if (!a && !b) return false;
  return !a || !b || a.some((value, i) => Math.abs(value - b[i]) >= HALF);
}).map((selector) => `${selector} ${JSON.stringify(from[selector])} → ${JSON.stringify(to[selector])}`);

/**
 * A second visit, the state remembered, with the script held: the first render draws the remembered state over
 * the plain view the build drew, and nothing under the title row moves, to the half pixel, at 1440x900 and 390x844.
 */
async function drawnSecondVisit(browser, options) {
  let passed = true;
  for (const [width, height] of [[1440, 900], [390, 844]]) {
    const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: 1, reducedMotion: 'no-preference' });
    const first = await context.newPage();
    await first.goto(options.url);
    await first.waitForSelector('.ws[data-live="answered"]', { timeout: 30000 });
    await first.waitForLoadState('networkidle');
    await first.close();
    const r = await drawn(browser, options, { hash: '' }, context);
    await context.close();
    const problems = [];
    if (!r.prerendered) problems.push('the page holds no prerendered markup');
    const moves = r.lastDrawn && r.firstMounted ? boxMoves(r.lastDrawn.boxes, r.firstMounted.boxes, ['.ws-places', '.ws-swap-map', '.ws-swap-order', '.ws-foot']) : ['no frame before the takeover'];
    const top = r.lastDrawn && r.firstMounted ? [r.lastDrawn.boxes['.ws-top'], r.firstMounted.boxes['.ws-top']] : null;
    if (top && Math.abs(top[0][0] + top[0][3] - (top[1][0] + top[1][3])) >= HALF) moves.unshift(`the title row's bottom ${top[0][0] + top[0][3]} → ${top[1][0] + top[1][3]}`);
    if (moves.length) problems.push(`moved at the takeover: ${moves.join(', ')}`);
    console.log(`${`${width}x${height}`.padEnd(10)} plain view, second visit (state remembered)
  script took over at ${r.firstMounted?.at} ms: ${moves.length ? 'moved' : 'nothing under the title row moved'}
  ${problems.length ? `FAIL: ${problems.join('; ')}` : 'ok'}`);
    passed = !problems.length && passed;
  }
  return passed;
}

/** The plain address: the map (or the tiles, when the plan has parts) and sign-in in the first painted frame, the footer rule, no move at the takeover. */
async function drawnPlain(browser, options, mapped, parts) {
  const r = await drawn(browser, options, { hash: '' });
  const problems = [];
  if (!r.prerendered) problems.push('the page holds no prerendered markup');
  if (parts && (mapped ? !r.firstShown?.map : !r.firstShown?.tiles)) problems.push(`no ${mapped ? 'map' : 'tiles'} in the first painted frame`);
  if (!r.firstShown?.signIn) problems.push('no sign-in in the first painted frame');
  if (r.before.length) problems.push(`footer: ${r.before.join('; ')}`);
  const moves = r.lastDrawn && r.firstMounted ? boxMoves(r.lastDrawn.boxes, r.firstMounted.boxes) : ['no frame before the takeover'];
  if (moves.length) problems.push(`moved at the takeover: ${moves.join(', ')}`);
  const atTakeover = r.shifts.filter((shift) => r.lastDrawn && shift.at >= r.lastDrawn.at && shift.at <= r.firstMounted.at + 100);
  if (atTakeover.length) problems.push(`layout shifts at the takeover: ${atTakeover.map((shift) => `${shift.at} ms ${shift.value}`).join(', ')}`);
  console.log(`\nthe built page, script held ${options.slowScript} ms\n1440x900   plain view
  first painted frame ${r.firstShown?.at} ms: ${[r.firstShown?.map ? 'map' : null, r.firstShown?.tiles ? 'tiles' : null, r.firstShown?.signIn ? 'sign-in' : null].filter(Boolean).join(', ') || 'nothing'}
  footer before the script: ${r.before.length ? 'FAIL' : 'ok'}
  script took over at ${r.firstMounted?.at} ms: ${moves.length ? 'boxes moved' : 'title row, map, footer and account in place'}; shifts at the takeover ${atTakeover.length}
  ${problems.length ? `FAIL: ${problems.join('; ')}` : 'ok'}`);
  return !problems.length;
}

/** A part's address: the plain view (map or tiles) is never painted, the part appears when the script runs. */
async function drawnPart(browser, options, place) {
  const r = await drawn(browser, options, place);
  const problems = [];
  if (!r.prerendered) problems.push('the page holds no prerendered markup');
  if (r.plainView.length) problems.push(`the plain view painted at ${r.plainView.slice(0, 3).join(', ')} ms`);
  if (!r.firstMounted?.part) problems.push('no part in the first frame after the takeover');
  if (r.before.length) problems.push(`footer: ${r.before.join('; ')}`);
  console.log(`1440x900   ${place.name}
  first painted frame ${r.firstShown?.at} ms: ${r.firstShown?.map || r.firstShown?.tiles ? 'plain view' : 'title row only'}; plain view painted in ${r.plainView.length} frames
  footer before the script: ${r.before.length ? 'FAIL' : 'ok'}
  script took over at ${r.firstMounted?.at} ms, part ${r.firstMounted?.part ? 'shown' : 'missing'} in that frame
  ${problems.length ? `FAIL: ${problems.join('; ')}` : 'ok'}`);
  return !problems.length;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  // Real scroll bars, as on Windows, Linux and a Mac with a mouse: headless Chromium hides them by default.
  const browser = await chromium.launch({ ignoreDefaultArgs: ['--hide-scrollbars'] });
  let passed = true;
  try {
    const page$ = await pageOf(browser, options.url);
    const list = page$.places;
    console.log(`${options.url}, API held ${options.slowApi} ms, script held ${options.slowScript} ms${options.signedIn ? ', signed in' : ''}`);
    for (const size of SIZES) {
      for (const place of list) {
        // Every run here is a first visit: nothing is remembered, so the first frame has the placeholders, "Suggest a
        // feature" in the title row and, signed out, the sign-in button.
        const rows = Boolean(place.rows);
        const wants = { signIn: !options.signedIn, suggest: true, placeholders: rows };
        passed = report(await run(browser, options, size, place), wants) && passed;
      }
    }
    console.log(`the page: ${page$.mapped ? 'a map' : page$.tile ? 'tiles, no map' : 'no part'}`);
    if (page$.tile) passed = (await sideways(browser, options.url, page$.tile, page$.mapped)) && passed;
    passed = (await visits(browser, options, page$.rows, page$)) && passed;
    // The build draws the plain address: what it drew is checked there, whatever view the url names. At any other
    // address the page holds what is under the title row until the script draws it.
    const plain = new URL(options.url);
    plain.search = '';
    const atPlain = { ...options, url: plain.href };
    const plain$ = plain.href === options.url ? page$ : await pageOf(browser, plain.href);
    passed = (await drawnPlain(browser, atPlain, plain$.mapped, Boolean(plain$.tile))) && passed;
    if (plain$.rows) passed = (await drawnPart(browser, atPlain, plain$.rows)) && passed;
    passed = (await drawnSecondVisit(browser, atPlain)) && passed;
  } finally {
    await browser.close();
  }
  console.log(passed ? 'load check passed' : 'load check FAILED');
  process.exitCode = passed ? 0 : 1;
}

// Run as a script, not when a test imports placesOf.
if (import.meta.main) await main();
