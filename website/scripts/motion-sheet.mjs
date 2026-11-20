#!/usr/bin/env node
// Captures the Workshop's moves frame by frame on the built page, and checks them.
//   node scripts/motion-sheet.mjs [url] [--out <dir>] [flow...]
// The url defaults to the `workshop:dev --fake` server; the output folder to website/.motion/; no flow named runs
// every flow (the names are the keys of FLOWS below).
// For each move it writes <name>.jpg, a contact sheet of the frames on screen at fixed times after the action, and
// <name>.txt: per element, each change of its box and opacity at each animation frame, the page's scroll, the
// animations that ran and the layout shifts.
// It exits non-zero when
// - in a move between the plain view and a place, the scroll position takes more than one new value;
// - `.ws-detail` runs `ws-rise` while a `::view-transition-*(detail)` animation runs, or a group named `hero` animates;
// - a move between two places runs any `::view-transition-*` animation;
// - Notes opening changes the panel's height in fewer than four steps;
// - a comment's height or the comment box's top changes when the comment is confirmed.
// On a page with a map, the moves run in Order, where the tiles are.
// It drives Playwright's own browser: `npx playwright-core install chromium-headless-shell` once.
import { mkdirSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright-core';

const wait = (ms) => new Promise((done) => setTimeout(done, ms));
/** The times after the action that the contact sheet shows, in ms. */
const TIMES = [0, 33, 66, 100, 130, 166, 200, 250, 300, 350, 400, 500, 650, 900, 1400];
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

/** Starts, in the page, a sampler that reads at every animation frame the chosen elements and the animations running. */
async function sample(page, selectors, flags) {
  await page.evaluate(({ selectors, flags }) => {
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
      for (const [name, selector] of Object.entries(selectors)) {
        const node = document.querySelector(selector);
        if (!node) { row[name] = null; continue; }
        const box = node.getBoundingClientRect();
        row[name] = [Math.round(box.left), Math.round(box.top * 10) / 10, Math.round(box.width), Math.round(box.height * 10) / 10, +(+getComputedStyle(node).opacity).toFixed(2)];
      }
      for (const [name, selector] of Object.entries(flags)) row[name] = document.querySelector(selector) !== null;
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
  }, { selectors, flags });
}

/** The problems of a move, from what was sampled after the action. */
function problemsOf(kind, rows, t0) {
  const after = rows.filter((row) => row.t >= t0);
  const problems = [];
  const any = (test) => after.some((row) => row.running.some(test));
  if (any((id) => id.includes('(hero)'))) problems.push('a group named hero animates');
  const overlap = after.find((row) => row.running.some((id) => id.includes('|ws-detail|ws-rise')) && row.running.some((id) => /^::view-transition-[a-z-]+\(detail\)/.test(id)));
  if (overlap) problems.push(`.ws-detail runs ws-rise during a view transition of detail (+${overlap.t - t0} ms)`);
  if (kind === 'blind') {
    const values = [];
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
  if (kind === 'comment') {
    const from = after.findIndex((row) => row.pending);
    if (from < 0) problems.push('the comment was never seen being sent');
    else {
      const rest = after.slice(from).filter((row) => row.comment && row.box);
      const heights = new Set(rest.map((row) => row.comment[3]));
      const tops = new Set(rest.map((row) => row.box[1] + row.y));
      if (!rest.some((row) => !row.pending)) problems.push('the comment was never seen confirmed');
      if (heights.size > 1) problems.push(`the comment's height changes: ${[...heights].join(', ')}`);
      if (tops.size > 1) problems.push(`the comment box's top changes: ${[...tops].join(', ')}`);
    }
  }
  return problems;
}

/** Runs one action, writes its sheet and its text, and returns its problems. */
async function capture(out, page, name, action, { selectors = ROWS, flags = {}, kind = null, ms = 1500 } = {}) {
  const client = await page.context().newCDPSession(page);
  const frames = [];
  client.on('Page.screencastFrame', (frame) => {
    frames.push({ t: frame.metadata.timestamp * 1000, data: frame.data });
    client.send('Page.screencastFrameAck', { sessionId: frame.sessionId }).catch(() => {});
  });
  await sample(page, selectors, flags);
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

  const problems = problemsOf(kind, data.rows, t0);
  const lines = [`## ${name}  ${frames.length} frames${kind ? `  checked as: ${kind}` : ''}`];
  for (const key of ['y', 'sx', ...Object.keys(selectors), ...Object.keys(flags)]) {
    let last;
    const changes = [];
    for (const row of data.rows) {
      const value = JSON.stringify(row[key]);
      if (value !== last) { changes.push(`${row.t - t0}:${value}`); last = value; }
    }
    lines.push(`${key}: ${changes.length > 28 ? [...changes.slice(0, 14), `…(${changes.length - 24})…`, ...changes.slice(-10)].join(' ') : changes.join(' ')}`);
  }
  const animations = data.seen.filter(([, a]) => a.first >= t0 - 5).map(([id, a]) => `  +${a.first - t0} ms ${id} duration=${a.duration} delay=${a.delay}`);
  lines.push(`animations after the action (${animations.length}):`, ...animations.slice(0, 40));
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

const ROWS = { title: '#ws-part-title', back: '.ws-back', board: '.ws-board', detail: '.ws-detail', under: '.ws-under', row1: '.ws-item:nth-of-type(1)', row2: '.ws-item:nth-of-type(2)' };
const click = (selector) => async (page) => { await page.locator(selector).first().click({ noWaitAfter: true }); };
const tileOf = (part) => `.ws-tile[data-part="${part}"] .ws-tile-hit`;

/** What the moves need from the page's data: its parts in order, those with rows, an entry with notes. */
async function siteOf(browser, url) {
  const context = await browser.newContext();
  const page = await context.newPage();
  await page.goto(url);
  await page.waitForFunction(() => window.__workshopData, null, { timeout: 15000 });
  const data = await page.evaluate(() => {
    const { parts, entries } = window.__workshopData;
    return { parts: parts.map((part) => part.id), entries: entries.map((entry) => ({ id: entry.id, part: entry.part, notes: entry.notes })) };
  });
  await page.waitForSelector('#workshop-root:not([data-prerendered]) .ws', { timeout: 15000 });
  const mapped = (await page.locator('.ws-stage').count()) > 0;
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

/** The flows; each opens its pages through `s.open` and returns its problems. */
const FLOWS = {
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
    const problems = await capture(s.out, page, 'row-open', click('.ws-item:nth-of-type(1) .ws-row'), { ms: 900 });
    return [...problems, ...await capture(s.out, page, 'row-close', click('.ws-item:nth-of-type(1) .ws-row'), { ms: 900 })];
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
    await box.fill(`A comment, to see it land (${new Date().toISOString()}).`);
    await page.route('**/comment', async (route) => {
      if (route.request().method() === 'POST') await wait(400);
      await route.continue();
    });
    return capture(s.out, page, 'comment-sent', click('.ws-item.open form button[type="submit"]'), {
      kind: 'comment', ms: 1400,
      selectors: { ...ROWS, comment: '.ws-item.open .ws-talk > div.ws-c:last-of-type', box: '.ws-item.open .ws-talk > .ws-box' },
      flags: { pending: '.ws-item.open .ws-c.ws-pending' },
    });
  },
  // Latest from the plain view, then a line of Latest that opens its entry where it lives.
  latest: async (s) => {
    const page = await s.open({});
    // In view before the capture, so that the only scroll in it is the page's.
    const link = 'a.ws-fold-link[href="#latest"]';
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
    // A page at an address with its state loaded; in Order on a page with a map.
    const open = async ({ size = DESK, hash = '', signIn = false }) => {
      const context = await browser.newContext({ viewport: { width: size[0], height: size[1] }, deviceScaleFactor: 1 });
      contexts.push(context);
      const page = await context.newPage();
      const url = new URL(options.url);
      if (site.mapped) url.searchParams.set('view', 'order');
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
