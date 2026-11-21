#!/usr/bin/env node
// Captures the Workshop's moves frame by frame on the built page, and checks them.
//   node scripts/motion-sheet.mjs [url] [--out <dir>] [flow...]
// The url defaults to the `workshop:dev --fake` server; the output folder to website/.motion/; no flow named runs
// every flow (the names are the keys of FLOWS below).
// For each move it writes <name>.jpg, a contact sheet of the frames on screen at fixed times after the action, and
// <name>.txt: every change of the page's scroll and of each element's box and opacity, read at each animation
// frame and never cut, the animations that ran and the layout shifts.
// It exits non-zero when, in any move,
// - a box it follows turns back: its top or its height goes more than 2 px one way, then more than 2 px the other;
// - a box that stays in the document goes from an opacity above 0.9 to one below 0.1 between two frames;
// a flow where one of these is meant names it in MEANT, with its reason. And it exits non-zero when
// - in a move between the plain view and a place, the scroll position takes more than one new value;
// - `.ws-detail` runs `ws-rise` while a `::view-transition-*(detail)` animation runs, or a group named `hero` animates;
// - a move between two places runs any `::view-transition-*` animation;
// - Notes opening changes the panel's height in fewer than four steps;
// - a comment's height or the comment box's top changes when the comment is confirmed;
// - a row's opening or closing covers more than 16% of its way in its first frame;
// - Map to Order or back, with a place open: the place or the footer goes under an opacity of 0.95, or the place's top
//   takes one step of more than 30 px.
// On a page with a map, the moves on the tiles run in Order, where the tiles are; the flows named map-* run on the map, at
// 1440 x 900: the map to a part and back, a scrolled map to a part and back from a scrolled part, Map to Order and back
// on the plain view and with a part and a row open.
// Each flow sets the view it needs, whatever view the url names.
// It drives Playwright's own browser: `npx playwright-core install chromium-headless-shell` once.
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright-core';

const wait = (ms) => new Promise((done) => setTimeout(done, ms));
/** The times after the action that the contact sheet shows, in ms. */
const TIMES = [0, 33, 66, 100, 130, 166, 200, 250, 300, 350, 400, 450, 500, 650, 900, 1400];
const PHONE = [390, 844];
const DESK = [1440, 900];

function parseArgs(argv) {
  const options = { url: 'http://127.0.0.1:4174/workshop', out: join(dirname(fileURLToPath(import.meta.url)), '..', '.motion'), flows: [] };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--out') options.out = resolve(argv[(i += 1)]);
    else if (arg.startsWith('--')) throw new Error(`Unknown option ${arg}`);
    else if (/^https?:/.test(arg)) options.url = arg;
    else options.flows.push(arg);
  }
  return options;
}

/**
 * Starts, in the page, a sampler that reads at every animation frame the chosen elements and the animations running.
 * An element is named by a selector, or by `{ selector, text }`: the first match whose text holds `text`, whose
 * classes are read too.
 */
async function sample(page, selectors) {
  await page.evaluate((selectors) => {
    const rows = [];
    const shifts = [];
    const seen = new Map();
    window.__motion = { rows, shifts, seen, on: true };
    new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) {
        shifts.push({ t: Date.now(), v: +entry.value.toFixed(4), input: entry.hadRecentInput, from: (entry.sources ?? []).slice(0, 3).map((s) => String(s.node?.className || s.node?.nodeName || '?').slice(0, 40)) });
      }
    }).observe({ type: 'layout-shift', buffered: false });
    const step = () => {
      if (!window.__motion.on) return;
      const board = document.querySelector('.ws-board');
      const row = { t: Date.now(), y: Math.round(scrollY), sx: board ? Math.round(board.scrollLeft) : null, running: [] };
      for (const [name, which] of Object.entries(selectors)) {
        const node = typeof which === 'string' ? document.querySelector(which) : [...document.querySelectorAll(which.selector)].find((n) => n.textContent.includes(which.text));
        if (!node) { row[name] = null; continue; }
        const box = node.getBoundingClientRect();
        row[name] = [Math.round(box.left), Math.round(box.top * 10) / 10, Math.round(box.width), Math.round(box.height * 10) / 10, +(+getComputedStyle(node).opacity).toFixed(2)];
        if (typeof which !== 'string') row[name].push(node.className);
      }
      for (const animation of document.getAnimations()) {
        const effect = animation.effect;
        const target = effect?.target;
        const id = `${effect?.pseudoElement ?? ''}|${String(target?.className ?? target?.nodeName ?? '').split(' ')[0]}|${animation.animationName ?? animation.transitionProperty ?? ''}`;
        if (animation.playState === 'running') row.running.push(id);
        if (!seen.has(id)) {
          const timing = effect?.getTiming?.() ?? {};
          seen.set(id, { first: Date.now(), duration: timing.duration, delay: timing.delay });
        }
      }
      rows.push(row);
      requestAnimationFrame(step);
    };
    requestAnimationFrame(step);
  }, selectors);
}

/** A box's top and height, read from a sample: `[left, top, width, height, opacity]`. */
const MEASURES = { top: 1, height: 3 };
/** How far a box may go one way, then the other, before it counts as turning back, in px. */
const TURN = 2;

/**
 * The boxes in `keys` that turn back after the action: for each, its top or its height as a list of runs, a run being
 * the sum of the changes of one sign in a row; two runs in a row of more than TURN px each are a turn. A box is read
 * only while it is drawn: one that is not (`display: none`, a box of no size) appears and leaves without moving.
 */
function turnsOf(rows, t0, keys) {
  const found = [];
  for (const key of keys) {
    for (const [measure, index] of Object.entries(MEASURES)) {
      const drawn = (box) => box && (box[2] > 0 || box[3] > 0);
      const series = rows.filter((row) => row.t >= t0 && drawn(row[key])).map((row) => ({ t: row.t - t0, v: row[key][index] }));
      const runs = [];
      for (let i = 1; i < series.length; i += 1) {
        const d = series[i].v - series[i - 1].v;
        if (!d) continue;
        const last = runs[runs.length - 1];
        if (last && Math.sign(last.sum) === Math.sign(d)) last.sum += d;
        else runs.push({ t: series[i].t, sum: d });
      }
      for (let i = 1; i < runs.length; i += 1) {
        if (Math.abs(runs[i - 1].sum) > TURN && Math.abs(runs[i].sum) > TURN) {
          found.push(`${key}'s ${measure} turns back: ${runs[i - 1].sum.toFixed(1)} px from +${runs[i - 1].t} ms, then ${runs[i].sum.toFixed(1)} px from +${runs[i].t} ms`);
          break;
        }
      }
    }
  }
  return found;
}

/** The boxes in `keys` that go from an opacity above 0.9 to one below 0.1 between two frames, staying in the document. */
function dropsOf(rows, t0, keys) {
  const found = [];
  for (const key of keys) {
    for (let i = 1; i < rows.length; i += 1) {
      const [a, b] = [rows[i - 1][key], rows[i][key]];
      if (rows[i].t < t0 || !a || !b || !(a[4] > 0.9 && b[4] < 0.1)) continue;
      found.push(`${key} goes from opacity ${a[4]} to ${b[4]} in one frame (+${rows[i].t - t0} ms)`);
      break;
    }
  }
  return found;
}

/** The problems of a move, from what was sampled after the action; `meant` names the checks the move skips. */
function problemsOf(kind, rows, t0, keys, meant = {}) {
  const after = rows.filter((row) => row.t >= t0);
  const problems = [];
  if (!meant.turns) problems.push(...turnsOf(rows, t0, keys));
  if (!meant.drops) problems.push(...dropsOf(rows, t0, keys));
  const any = (test) => after.some((row) => row.running.some(test));
  if (any((id) => id.includes('(hero)'))) problems.push('a group named hero animates');
  const overlap = after.find((row) => row.running.some((id) => id.includes('|ws-detail|ws-rise')) && row.running.some((id) => /^::view-transition-[a-z-]+\(detail\)/.test(id)));
  if (overlap) problems.push(`.ws-detail runs ws-rise during a view transition of detail (+${overlap.t - t0} ms)`);
  if (kind === 'blind') {
    // From the position before the action: each value after it that differs from the one before is a new one.
    const before = rows.filter((row) => row.t < t0).at(-1);
    const values = before ? [before.y] : [];
    for (const row of after) if (values[values.length - 1] !== row.y) values.push(row.y);
    if (values.length > 2) problems.push(`the scroll takes ${values.length - 1} new values: ${values.join(' → ')}`);
    if (!any((id) => id.startsWith('::view-transition'))) problems.push('no view transition ran');
  }
  if (kind === 'between') {
    const vt = after.find((row) => row.running.some((id) => id.startsWith('::view-transition')));
    if (vt) problems.push(`a view transition runs between two places (+${vt.t - t0} ms)`);
  }
  if (kind === 'notes') {
    const heights = [];
    for (const row of after) if (row.panel && heights[heights.length - 1] !== row.panel[3]) heights.push(row.panel[3]);
    if (heights.length - 1 < 4) problems.push(`the panel's height changes in ${heights.length - 1} steps: ${heights.join(' → ')}`);
  }
  if (kind === 'row') {
    // From the height before the action to the last one: the share of that way covered at the first frame that moves.
    const before = rows.filter((row) => row.t < t0 && row.more).at(-1)?.more[3];
    const heights = after.filter((row) => row.more).map((row) => row.more[3]);
    const end = heights.at(-1);
    const first = heights.find((height) => height !== before);
    if (before === undefined || end === undefined || first === undefined || end === before) problems.push('the row never moved');
    else {
      const share = Math.abs(first - before) / Math.abs(end - before);
      if (share > 0.16) problems.push(`the row covers ${Math.round(share * 100)}% of its way in its first frame (${before} → ${first} of ${end})`);
    }
  }
  if (kind === 'swap') {
    // With a place open, Map and Order swap the strip only: the place and the footer stay, the place slides.
    for (const key of ['detail', 'foot']) {
      const low = after.find((row) => row[key] && row[key][4] < 0.95);
      if (low) problems.push(`${key}'s opacity goes to ${low[key][4]} (+${low.t - t0} ms)`);
    }
    const tops = after.filter((row) => row.detail).map((row) => ({ t: row.t - t0, v: row.detail[1] }));
    const step = tops.slice(1).find((top, i) => Math.abs(top.v - tops[i].v) > 30);
    if (step) problems.push(`the place's top takes a step of ${Math.round(step.v - tops[tops.indexOf(step) - 1].v)} px (+${step.t} ms)`);
  }
  if (kind === 'comment') {
    // The comment is the one with the text sent; its classes say whether it is being sent.
    const pending = (row) => row.comment[5].split(' ').includes('ws-pending');
    const from = after.findIndex((row) => row.comment && pending(row));
    if (from < 0) problems.push('the comment was never seen being sent');
    else {
      const rest = after.slice(from);
      const lost = rest.find((row) => !row.comment || !row.box);
      if (lost) problems.push(`the comment or the box is not found at +${lost.t - t0} ms`);
      const found = rest.filter((row) => row.comment && row.box);
      if (!found.some((row) => !pending(row))) problems.push('the comment was never seen confirmed');
      const heights = new Set(found.map((row) => row.comment[3]));
      const tops = new Set(found.map((row) => row.box[1] + row.y));
      if (heights.size > 1) problems.push(`the comment's height changes: ${[...heights].join(', ')}`);
      if (tops.size > 1) problems.push(`the comment box's top changes: ${[...tops].join(', ')}`);
    }
  }
  return problems;
}

/** Runs one action, writes its sheet and its text, and returns its problems. */
async function capture(out, page, name, action, { selectors = ROWS, kind = null, ms = 1500 } = {}) {
  const meant = MEANT[name] ?? {};
  const client = await page.context().newCDPSession(page);
  const frames = [];
  client.on('Page.screencastFrame', (frame) => {
    frames.push({ t: frame.metadata.timestamp * 1000, data: frame.data });
    client.send('Page.screencastFrameAck', { sessionId: frame.sessionId }).catch(() => {});
  });
  await sample(page, selectors);
  await client.send('Page.startScreencast', { format: 'jpeg', quality: 70, maxWidth: 720, maxHeight: 720, everyNthFrame: 1 });
  await wait(250);
  const t0 = Date.now();
  await action(page);
  await wait(ms);
  await client.send('Page.stopScreencast');
  const data = await page.evaluate(() => {
    window.__motion.on = false;
    return { rows: window.__motion.rows, shifts: window.__motion.shifts, seen: [...window.__motion.seen.entries()] };
  });
  await client.detach().catch(() => {});

  const problems = problemsOf(kind, data.rows, t0, Object.keys(selectors), meant);
  const skipped = Object.entries(meant).map(([check, why]) => `${check} not checked: ${why}`);
  const lines = [`## ${name}  ${frames.length} frames${kind ? `  checked as: ${kind}` : ''}`, ...skipped];
  for (const key of ['y', 'sx', ...Object.keys(selectors)]) {
    let last;
    const changes = [];
    for (const row of data.rows) {
      const value = JSON.stringify(row[key]);
      if (value !== last) { changes.push(`${row.t - t0}:${value}`); last = value; }
    }
    lines.push(`${key}: ${changes.join(' ')}`);
  }
  const animations = data.seen.filter(([, a]) => a.first >= t0 - 5).map(([id, a]) => `  +${a.first - t0} ms ${id} duration=${a.duration} delay=${a.delay}`);
  lines.push(`animations after the action (${animations.length}):`, ...animations);
  const shifts = data.shifts.filter((s) => s.t >= t0);
  lines.push(`layout shifts after the action: ${shifts.length ? shifts.map((s) => `+${s.t - t0} ms ${s.v}${s.input ? ' (input)' : ''} [${s.from.join(', ')}]`).join('; ') : 'none'}`);
  lines.push(problems.length ? `FAIL: ${problems.join('; ')}` : 'ok');
  writeFileSync(join(out, `${name}.txt`), `${lines.join('\n')}\n`);

  // The contact sheet: the frame on screen at each time.
  const picks = TIMES.filter((t) => t <= ms).map((t) => {
    let pick = null;
    for (const frame of frames) if (frame.t - t0 <= t) pick = frame;
    return { t, frame: pick ?? frames[0], at: pick ? Math.round(pick.t - t0) : null };
  });
  const sheet = await page.context().newPage();
  await sheet.setViewportSize({ width: 2190, height: 100 });
  const view = page.viewportSize();
  const width = view.width >= view.height ? 720 : Math.round((720 * view.width) / view.height);
  const cols = view.width >= view.height ? 3 : 5;
  await sheet.setContent(`<body style="margin:0;background:#222;font:14px/1.2 monospace;color:#fff"><div style="display:grid;grid-template-columns:repeat(${cols},${width}px);gap:6px;padding:6px;width:max-content">${picks.map((p) => `<figure style="margin:0"><figcaption style="padding:2px 4px;background:#000">${name} +${p.t}ms (frame at ${p.at})</figcaption>${p.frame ? `<img style="display:block;width:${width}px" src="data:image/jpeg;base64,${p.frame.data}">` : ''}</figure>`).join('')}</div></body>`);
  await sheet.waitForLoadState('load');
  await sheet.locator('body > div').screenshot({ path: join(out, `${name}.jpg`), type: 'jpeg', quality: 72 });
  await sheet.close();
  console.log(`${name}: ${problems.length ? `FAIL: ${problems.join('; ')}` : 'ok'}`);
  return problems;
}

const ROWS = { title: '#ws-part-title', back: '.ws-back', board: '.ws-board', detail: '.ws-detail', places: '.ws-places', row1: '.ws-item:nth-of-type(1)', row2: '.ws-item:nth-of-type(2)' };
/** What the map's moves read: the stage, the part opened, a part inside another, a wire's word, the back control and the detail. */
const MAP_ROWS = { stage: '.ws-stage', opened: '.ws-part.on', inner: '.ws-part.inner', views: '.ws-part[data-part="views"]', word: '.ws-wl', back: '.ws-back', detail: '.ws-detail', places: '.ws-places' };
/**
 * The moves where a check is meant to fail, by the sheet's name: the check (`turns` or `drops`) and why. A move between
 * the plain view and a place replaces what the selectors find, so a box named the same can be another one.
 */
const MEANT = {
  'part-to-part': { turns: 'the new place replaces the old one at once and rises 8 px into its place (ws-rise), as a place opened after the first frames does; the boxes followed are the new place\'s, which start 8 px lower' },
};
const click = (selector) => async (page) => { await page.locator(selector).first().click({ noWaitAfter: true }); };
const tileOf = (part) => `.ws-tile[data-part="${part}"] .ws-tile-hit`;

/** What the moves need from the page's data: its parts in order, those with rows, an entry with notes. */
async function siteOf(browser, url) {
  const context = await browser.newContext({ viewport: { width: DESK[0], height: DESK[1] } });
  const page = await context.newPage();
  // Whatever view the url names: a page has a map when the map shows in its own default view, on a desk.
  const plain = new URL(url);
  plain.searchParams.delete('view');
  await page.goto(plain.href);
  await page.waitForFunction(() => window.__workshopData, null, { timeout: 15000 });
  const data = await page.evaluate(() => {
    const { parts, entries } = window.__workshopData;
    return { parts: parts.map((part) => part.id), entries: entries.map((entry) => ({ id: entry.id, part: entry.part, notes: entry.notes })) };
  });
  await page.waitForSelector('#workshop-root:not([data-prerendered]) .ws', { timeout: 15000 });
  const mapped = await page.locator('.ws-stage').first().isVisible();
  await context.close();
  const withRows = data.parts.filter((part) => data.entries.some((entry) => entry.part === part));
  const noted = data.entries.find((entry) => entry.notes && entry.part);
  return { mapped, parts: data.parts, withRows, noted };
}

async function openAndBack(s, size, prefix) {
  const page = await s.open({ size });
  const part = s.site.withRows[0] ?? s.site.parts[0];
  await page.locator(tileOf(part)).hover();
  await wait(300);
  const problems = await capture(s.out, page, `${prefix}open-part`, click(tileOf(part)), { kind: 'blind' });
  return [...problems, ...await capture(s.out, page, `${prefix}back-part`, click('.ws-back'), { kind: 'blind' })];
}

/** The part a map flow opens: the first with rows, by its box on the map. */
const mapPartOf = (s) => `.ws-part[data-part="${s.site.withRows[0] ?? s.site.parts[0]}"]`;
// Near its top left corner, on its title: a part drawn inside another covers the middle of that one.
const TITLE = { position: { x: 12, y: 12 } };
const press = (selector) => async (page) => { await page.locator(selector).first().click({ ...TITLE, noWaitAfter: true }); };

/** The flows; each opens its pages through `s.open` and returns its problems. */
const FLOWS = {
  // The map to a part, which folds it, and back.
  'map-open-and-back': async (s) => {
    if (!s.site.mapped) return [];
    const page = await s.open({ map: true });
    await page.locator(mapPartOf(s)).hover(TITLE);
    await wait(300);
    const problems = await capture(s.out, page, 'map-open-part', press(mapPartOf(s)), { selectors: MAP_ROWS });
    return [...problems, ...await capture(s.out, page, 'map-back-part', click('.ws-back'), { selectors: MAP_ROWS })];
  },
  // The map scrolled down, then a part: the map lifts as it folds; that part scrolled down, then Escape.
  'map-scrolled': async (s) => {
    if (!s.site.mapped) return [];
    const page = await s.open({ map: true });
    await page.evaluate(() => scrollTo({ top: 300, behavior: 'instant' }));
    await wait(200);
    const problems = await capture(s.out, page, 'map-scrolled-open', press(mapPartOf(s)), { selectors: MAP_ROWS });
    await page.evaluate(() => scrollTo({ top: 500, behavior: 'instant' }));
    await wait(200);
    return [...problems, ...await capture(s.out, page, 'map-scrolled-back', async (p) => { await p.keyboard.press('Escape'); }, { selectors: MAP_ROWS })];
  },
  // Map to Order and back, on the plain view.
  'map-order': async (s) => {
    if (!s.site.mapped) return [];
    const page = await s.open({ map: true });
    const order = '.ws-seg button:has-text("Order")';
    const problems = await capture(s.out, page, 'map-to-order', click(order), { selectors: { ...MAP_ROWS, tile: '.ws-tile' } });
    await wait(300);
    return [...problems, ...await capture(s.out, page, 'order-to-map', click('.ws-seg button:has-text("Map")'), { selectors: { ...MAP_ROWS, tile: '.ws-tile' } })];
  },
  // A part and a row open on the map, then Order, then the map again: the strip swaps, the place slides.
  'map-order-open': async (s) => {
    if (!s.site.mapped) return [];
    const page = await s.open({ map: true, hash: `#${s.site.withRows[0]}` });
    await click('.ws-item:nth-of-type(1) .ws-row')(page);
    await wait(800);
    const selectors = { ...MAP_ROWS, board: '.ws-board', row1: '.ws-item:nth-of-type(1)', row2: '.ws-item:nth-of-type(2)', more: '.ws-item.open .ws-more', foot: '.ws-foot' };
    const problems = await capture(s.out, page, 'map-to-order-open', click('.ws-seg button:has-text("Order")'), { kind: 'swap', selectors });
    await wait(300);
    return [...problems, ...await capture(s.out, page, 'order-to-map-open', click('.ws-seg button:has-text("Map")'), { kind: 'swap', selectors })];
  },
  // The plain view to a part and back, on a desk and on a phone.
  'open-and-back': (s) => openAndBack(s, DESK, ''),
  'phone-open-and-back': (s) => openAndBack(s, PHONE, 'phone-'),
  // From one part to another through the strip.
  'part-to-part': async (s) => {
    const page = await s.open({ hash: `#${s.site.withRows[0]}` });
    return capture(s.out, page, 'part-to-part', click(tileOf(s.site.withRows[1] ?? s.site.parts[1])), { kind: 'between' });
  },
  // A row opens and closes.
  rows: async (s) => {
    const page = await s.open({ hash: `#${s.site.withRows[0]}` });
    const selectors = { ...ROWS, more: '.ws-item:nth-of-type(1) .ws-more' };
    const problems = await capture(s.out, page, 'row-open', click('.ws-item:nth-of-type(1) .ws-row'), { kind: 'row', ms: 900, selectors });
    return [...problems, ...await capture(s.out, page, 'row-close', click('.ws-item:nth-of-type(1) .ws-row'), { kind: 'row', ms: 900, selectors })];
  },
  // The Notes of an open row, their chunk loaded on the click.
  notes: async (s) => {
    if (!s.site.noted) return ['no entry has notes'];
    const page = await s.open({ hash: `#${s.site.noted.part}/${s.site.noted.id}` });
    const item = `[id="entry-${s.site.noted.id}"]`;
    await page.waitForSelector(`${item}.open .ws-more.open`);
    await wait(500);
    return capture(s.out, page, 'notes-open', click(`${item} .ws-disc:not(.ws-disc-history)`), { kind: 'notes', ms: 900, selectors: { ...ROWS, panel: `${item} .ws-more-in` } });
  },
  // Signed in through the fake GitHub's page, a comment sent; the server's answer is held back so it is seen being sent.
  comment: async (s) => {
    const page = await s.open({ hash: `#${s.site.withRows[0]}`, signIn: true });
    await click('.ws-item:nth-of-type(1) .ws-row')(page);
    const box = page.locator('.ws-item.open textarea');
    await box.waitFor();
    await wait(600);
    // A text of its own each run: the page shows a person's comment once per text, and the server keeps earlier runs'.
    const text = `A comment, to see it land (${new Date().toISOString()}).`;
    await box.fill(text);
    await page.route('**/comment', async (route) => {
      if (route.request().method() === 'POST') await wait(400);
      await route.continue();
    });
    return capture(s.out, page, 'comment-sent', click('.ws-item.open form button[type="submit"]'), {
      kind: 'comment', ms: 1400,
      selectors: { ...ROWS, comment: { selector: '.ws-item.open .ws-c', text }, box: '.ws-item.open .ws-talk > .ws-box' },
    });
  },
  // Latest from the plain view, then a line of Latest that opens its entry where it lives.
  latest: async (s) => {
    const page = await s.open({});
    // In view before the capture, so that the only scroll in it is the page's.
    const link = '.ws-places a[href="#latest"]';
    await page.locator(link).scrollIntoViewIfNeeded();
    await wait(200);
    const problems = await capture(s.out, page, 'open-latest', click(link), { kind: 'blind', selectors: { ...ROWS, latest: '.ws-latest' } });
    const line = '.ws-latest li:not(.ws-latest-ghost) a.ws-row';
    await page.waitForSelector(line);
    await wait(300);
    return [...problems, ...await capture(s.out, page, 'latest-to-entry', click(line), { kind: 'between', ms: 1400 })];
  },
  // The plain view scrolled down, then the last tile; that part scrolled down, then Escape.
  scrolled: async (s) => {
    const page = await s.open({ size: [1440, 700] });
    await page.evaluate(() => scrollTo({ top: 400, behavior: 'instant' }));
    await wait(200);
    const low = s.site.parts[s.site.parts.length - 1];
    const problems = await capture(s.out, page, 'scrolled-open', click(tileOf(low)), { kind: 'blind' });
    await page.evaluate(() => scrollTo({ top: 500, behavior: 'instant' }));
    await wait(200);
    return [...problems, ...await capture(s.out, page, 'scrolled-back', async (p) => { await p.keyboard.press('Escape'); }, { kind: 'blind' })];
  },
};

async function main() {
  const options = parseArgs(process.argv.slice(2));
  const unknown = options.flows.filter((name) => !FLOWS[name]);
  if (unknown.length) throw new Error(`Unknown flow ${unknown.join(', ')}; the flows are ${Object.keys(FLOWS).join(', ')}`);
  mkdirSync(options.out, { recursive: true });
  const browser = await chromium.launch();
  const failed = [];
  try {
    const site = await siteOf(browser, options.url);
    if (!site.parts.length) throw new Error('the page has no part');
    const contexts = [];
    // A page at an address with its state loaded, in the view the flow needs whatever the url names: on the map for the
    // map's flows, in Order for the others on a page with a map.
    const open = async ({ size = DESK, hash = '', signIn = false, map = false }) => {
      const context = await browser.newContext({ viewport: { width: size[0], height: size[1] }, deviceScaleFactor: 1 });
      contexts.push(context);
      const page = await context.newPage();
      const url = new URL(options.url);
      if (site.mapped && !map) url.searchParams.set('view', 'order');
      else url.searchParams.delete('view');
      if (signIn) {
        await page.goto(url.href, { waitUntil: 'networkidle' });
        await page.locator('#workshop-account a.button').click();
        await page.waitForLoadState('networkidle');
        await page.getByText('owner-fake').first().click();
        await page.waitForURL(/\/workshop/);
        await page.waitForLoadState('networkidle');
      }
      url.hash = hash;
      await page.goto(url.href, { waitUntil: 'networkidle' });
      await page.waitForSelector('#workshop-root:not([data-prerendered]) .ws[data-live="answered"]', { timeout: 15000 });
      if (map && !(await page.locator('.ws-stage').first().isVisible())) throw new Error('the map does not show');
      await wait(400);
      return page;
    };
    for (const [name, run] of Object.entries(FLOWS)) {
      if (options.flows.length && !options.flows.includes(name)) continue;
      try {
        const problems = await run({ site, out: options.out, open });
        if (problems.length) failed.push(name);
      } catch (error) {
        console.log(`${name}: FAIL: ${error.message.split('\n')[0]}`);
        failed.push(name);
      }
      for (const context of contexts.splice(0)) await context.close();
    }
  } finally {
    await browser.close();
  }
  console.log(`sheets in ${options.out}`);
  console.log(failed.length ? `motion sheet FAILED: ${failed.join(', ')}` : 'motion sheet passed');
  process.exitCode = failed.length ? 1 : 0;
}

await main();
