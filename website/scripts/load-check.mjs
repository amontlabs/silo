#!/usr/bin/env node
// Loads the Workshop in a real browser at several window sizes and places, and checks what a first visit shows.
//   node scripts/load-check.mjs [url] [--slow-api <ms>] [--slow-script <ms>] [--owner]
// The url defaults to the `workshop:dev --fake` server. --slow-api and --slow-script hold back the API's answers
// and the page's script; --owner loads the page as the signed-in owner.
// Each run prints when the page first painted, when each late item appeared and every layout shift, and checks
// the footer rule before the script has run and once everything has loaded. It exits non-zero when the rule
// fails or a layout shift is recorded.
// It drives Playwright's own browser: `npx playwright-core install chromium-headless-shell` once.
import { chromium } from 'playwright-core';

const SIZES = [[1440, 900], [1440, 1100], [2560, 1440], [1024, 1366], [390, 844]];
const LATE = {
  account: '#workshop-account > *',
  votes: '.ws-vote',
  'owner marks': '.ws.ws-owner',
  'Suggest a feature': '.ws-suggest',
};
// Room for rounding between boxes, in CSS pixels.
const SLACK = 1;

function parseArgs(argv) {
  const options = { url: 'http://127.0.0.1:4174/workshop', slowApi: 0, slowScript: 0, owner: false };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--slow-api') options.slowApi = Number(argv[(i += 1)]);
    else if (arg === '--slow-script') options.slowScript = Number(argv[(i += 1)]);
    else if (arg === '--owner') options.owner = true;
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
  const check = () => {
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
  window.__loadCheck = { seen, shifts };
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

async function slowApi(page, delay, owner) {
  await page.route('**/api/workshop/**', async (route) => {
    if (delay) await wait(delay);
    if (owner && new URL(route.request().url()).pathname === '/api/workshop/me') {
      await route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify({ login: 'owner', avatar: '', canDecide: true }) });
    } else await route.continue();
  });
}

/** One load of one place at one size: the rule before the script runs, then the timeline and the rule after. */
async function run(browser, options, [width, height], place) {
  const context = await browser.newContext({ viewport: { width, height }, deviceScaleFactor: 1, reducedMotion: 'no-preference' });
  if (options.owner) await context.addCookies([{ name: 'workshop-hint', value: '1', url: options.url }]);
  const page = await context.newPage();
  await page.addInitScript(recordTimeline, LATE);
  await slowApi(page, options.slowApi, options.owner);
  const release = await holdScripts(page, options.slowScript);
  await page.goto(options.url + place.hash, { waitUntil: 'commit' });
  // The page is parsed and styled; the held script has not run.
  await page.waitForFunction(() => document.readyState !== 'loading' && [...document.querySelectorAll('link[rel=stylesheet]')].every((link) => link.sheet));
  const before = await page.evaluate(footerRule, SLACK);
  release();
  await page.waitForLoadState('load');
  await page.waitForSelector('.ws');
  await page.waitForLoadState('networkidle');
  // Late answers and the transitions they start.
  await wait(1200);
  const after = await page.evaluate(footerRule, SLACK);
  const timeline = await page.evaluate(() => ({
    ...window.__loadCheck,
    paint: Math.round(performance.getEntriesByName('first-paint')[0]?.startTime ?? NaN),
  }));
  await context.close();
  return { size: `${width}x${height}`, place: place.name, before, after, ...timeline };
}

function report(result) {
  const late = Object.keys(LATE).map((name) => `${name} ${result.seen[name] === undefined ? '—' : `${result.seen[name]} ms`}`).join(', ');
  const rule = (problems) => (problems.length ? `FAIL: ${problems.join('; ')}` : 'ok');
  const lines = [
    `${result.size.padEnd(10)} ${result.place}`,
    `  first paint ${result.paint} ms; ${late}`,
    `  footer before the script: ${rule(result.before)}`,
    `  footer after loading: ${rule(result.after)}`,
    `  layout shifts: ${result.shifts.length ? result.shifts.map((shift) => `${shift.at} ms ${shift.value} (${shift.nodes.join(' ')})`).join(', ') : 'none'}`,
  ];
  console.log(lines.join('\n'));
  return !result.before.length && !result.after.length && !result.shifts.length;
}

/** The plain view, the first area, Latest and the suggestions. */
async function places(browser, url) {
  const context = await browser.newContext();
  const page = await context.newPage();
  await page.goto(url);
  const first = await page.locator('[data-area]').first().getAttribute('data-area', { timeout: 15000 });
  await context.close();
  return [
    { name: 'plain view', hash: '' },
    { name: `first area (#${first})`, hash: `#${first}` },
    { name: '#latest', hash: '#latest' },
    { name: '#suggested', hash: '#suggested' },
  ];
}

/**
 * Opening the first area from the map and going back never moves the page sideways, as a scroll bar coming or
 * going would: the frame's left edge is sampled on every frame, at 1440x900.
 */
async function sideways(browser, url, area) {
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
  await page.locator(`.ws-part[data-part="${area}"]`).click();
  await wait(1500);
  const opened = await left();
  await page.locator('.ws-back').click();
  await wait(1500);
  const back = await left();
  const seen = [...new Set(await page.evaluate(() => window.__lefts))];
  await context.close();
  const passed = plain === opened && opened === back && seen.length === 1;
  console.log(`1440x900   open #${area} and go back: frame's left edge ${plain} px, opened ${opened} px, back ${back} px, on every frame ${seen.join(' / ')} px: ${passed ? 'ok' : 'FAIL: the page moved sideways'}`);
  return passed;
}

async function main() {
  const options = parseArgs(process.argv.slice(2));
  // Real scroll bars, as on Windows, Linux and a Mac with a mouse: headless Chromium hides them by default.
  const browser = await chromium.launch({ ignoreDefaultArgs: ['--hide-scrollbars'] });
  let passed = true;
  try {
    const list = await places(browser, options.url);
    console.log(`${options.url}, API held ${options.slowApi} ms, script held ${options.slowScript} ms${options.owner ? ', as the owner' : ''}`);
    for (const size of SIZES) {
      for (const place of list) passed = report(await run(browser, options, size, place)) && passed;
    }
    passed = (await sideways(browser, options.url, list[1].hash.slice(1))) && passed;
  } finally {
    await browser.close();
  }
  console.log(passed ? 'load check passed' : 'load check FAILED');
  process.exitCode = passed ? 0 : 1;
}

await main();
