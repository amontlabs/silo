#!/usr/bin/env node
// The home page's film where it needs layout, in Playwright's Firefox.
//   node scripts/film-check.mjs [url] [--write-still] [--shots <dir>]
// The url defaults to the dev server, http://127.0.0.1:4173/. `npm test` does not run it.
//
// It holds a frame every 100 ms of the loop at each of SCREENS and exits non-zero when
// - two texts overlap, a text is cut by the film's edge or runs out of the box it stands in;
// - a word shows under 11 px tall, or on a phone a word of the film is never whole in view long enough to read;
// - anything around the film changes place, or the film has another size without its script than with it;
// - the page scrolls sideways at a width from 320 to 1440;
// - the window and its controls are not above the fold of a laptop's screen;
// - src/film/still.html, what shows without script, is not the frame the film draws at the end of Network.
// --write-still writes that file. --shots saves the end of each shot, at each screen in both themes.

import { mkdir, readFile, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';
import { firefox } from 'playwright-core';

const { values, positionals } = parseArgs({
  allowPositionals: true,
  options: { 'write-still': { type: 'boolean' }, shots: { type: 'string' } },
});
const URL_ = positionals[0] ?? 'http://127.0.0.1:4173/';
const STILL_FILE = fileURLToPath(new URL('../src/film/still.html', import.meta.url));
const SCREENS = [[1440, 900], [1280, 720], [1024, 768], [390, 844]];
/** The screens where the whole film has to show without scrolling. */
const ABOVE_FOLD = [[1440, 900], [1280, 720], [1024, 768]];
const STEP = 100;
/** Overlaps that are meant: the fork's name lifts off as an opaque tag and passes over the prompt on its way to the Bin. */
const MEANT = [['$', 'fork']];
/** How long a word has to be whole in a phone's view, at least once. */
const READ = 500;

const { TIMED, LENGTH, STILL, endOf } = await (async () => {
  const { createServer } = await import('vite');
  const root = fileURLToPath(new URL('..', import.meta.url));
  const vite = await createServer({ root, configFile: false, logLevel: 'error', appType: 'custom', server: { middlewareMode: true, hmr: false, ws: false, watch: null } });
  try {
    return await vite.ssrLoadModule('/src/film/story.ts');
  } finally {
    await vite.close();
  }
})();

const browser = await firefox.launch();
const problems = new Set();

async function open([width, height], { theme = 'light', script = true, query = '?hold=0' } = {}) {
  const context = await browser.newContext({ viewport: { width, height }, colorScheme: theme, javaScriptEnabled: script });
  const page = await context.newPage();
  page.on('pageerror', (error) => problems.add(`${width} x ${height}: ${error.message}`));
  await page.goto(URL_ + query);
  if (script) {
    await page.waitForSelector('.film.live');
    await page.evaluate(() => document.fonts.ready.then(() => true));
  } else {
    // Without script a page settles no promise, so its fonts cannot be waited for.
    await page.waitForTimeout(1000);
  }
  return { page, close: () => context.close() };
}

/** Where the film and what follows it stand on the page. */
const placesOf = (page) =>
  page.evaluate(() => {
    const boxes = ['.film', '.film figure', '.film-under', '.for', '.site-footer'].map((selector) => {
      const { x, y, width, height } = document.querySelector(selector).getBoundingClientRect();
      return `${selector} ${[x, y + scrollY, width, height].map((n) => Math.round(n)).join(' ')}`;
    });
    return boxes.join('\n');
  });

/** The markup of the frame on screen without what cannot be seen and without what only works with a script. */
const stillMarkup = (page, selector) =>
  page.evaluate((selector) => {
    const round = (value) => value.replace(/-?\d+\.\d+/g, (n) => String(Math.round(Number(n) * 10) / 10));
    const copyOf = (el) => {
      if (el.nodeType === Node.TEXT_NODE) return el.cloneNode();
      const style = getComputedStyle(el);
      const svgText = el.tagName === 'text';
      if (Number(style.opacity) < 0.004 || (style.visibility === 'hidden' && !el.children.length)) return null;
      if (style.visibility === 'hidden' && el.tagName !== 'svg') return null;
      if (svgText && (Number(style.fillOpacity) < 0.5 || !el.textContent)) return null;
      const copy = el.cloneNode(false);
      for (const name of ['data-shot', 'data-go', 'data-line', 'role', 'tabindex', 'aria-label', 'aria-hidden']) copy.removeAttribute(name);
      for (const name of ['visibility', 'fill-opacity', 'stroke-dashoffset']) copy.style?.removeProperty(name);
      if (Number(copy.style?.opacity) === 1) copy.style.removeProperty('opacity');
      if (!copy.getAttribute('style')) copy.removeAttribute('style');
      if (copy.getAttribute('transform') === 'translate(0.00 0.00)') copy.removeAttribute('transform');
      for (const attribute of [...copy.attributes]) if (attribute.name !== 'class' && attribute.name !== 'href') copy.setAttribute(attribute.name, round(attribute.value));
      for (const child of el.childNodes) {
        const inner = copyOf(child);
        if (inner) copy.append(inner);
      }
      if (el.tagName === 'g' && !copy.childNodes.length) return null;
      return copy;
    };
    const copy = copyOf(document.querySelector(selector));
    copy.setAttribute('aria-hidden', 'true');
    return copy.outerHTML;
  }, selector);

async function still() {
  const parts = [];
  for (const [screen, selector] of [[[1440, 900], '.film-window'], [[390, 844], '.film-phone']]) {
    const { page, close } = await open(screen, { query: `?hold=${STILL}` });
    parts.push(await stillMarkup(page, selector));
    await close();
  }
  return `${parts.join('\n')}\n`;
}

/** Holds each moment of `times` and reports what is wrong with its words, and which words are whole in view. */
const readFrames = (page, times) =>
  page.evaluate(([times, meant]) => {
    const root = document.querySelector('.film');
    const svgs = [...root.querySelectorAll('figure svg')].filter((svg) => svg.getClientRects().length);
    const found = new Set();
    const seen = [];
    for (const t of times) {
      root.film.hold(t);
      const words = [];
      const boxes = [];
      const walk = (el, opacity, svg, edge) => {
        const style = getComputedStyle(el);
        const now = opacity * Number(style.opacity);
        // The film only ever hides a part, so nothing in a hidden part shows.
        if (now < 0.004 || style.visibility === 'hidden') return;
        if (el.tagName === 'text') {
          const alpha = now * Number(style.fillOpacity);
          if (alpha < 0.1 || !el.textContent) return;
          const scale = el.getScreenCTM().a;
          const { left, right, top, bottom, height } = el.getBoundingClientRect();
          // The box has room beside the letters, so across it is the advance of the characters.
          const width = el.children.length ? right - left : el.getComputedTextLength() * scale;
          const middle = (left + right) / 2;
          const box = { left: middle - width / 2, right: middle + width / 2, top, bottom, height };
          words.push({ el, text: el.textContent, alpha, box, edge, px: parseFloat(style.fontSize) * scale });
          return;
        }
        if (el.matches('rect:is(.f-box, .f-card, .f-btn, .f-gate)')) boxes.push({ el, box: el.getBoundingClientRect() });
        for (const child of el.children) walk(child, now, svg, edge);
      };
      for (const svg of svgs) walk(svg, 1, svg, svg.getBoundingClientRect());

      for (const { text, box, edge, px, el } of words) {
        if (px < 10.95) found.add(`"${text}" is ${px.toFixed(2)} px tall`);
        if (box.left < edge.left - 0.5 || box.right > edge.right + 0.5 || box.top < edge.top - 1 || box.bottom > edge.bottom + 1) found.add(`at ${t}: "${text}" is cut`);
        for (const other of boxes) {
          if (other.el.parentNode !== el.parentNode) continue;
          const middle = (box.top + box.bottom) / 2;
          const inside = middle > other.box.top && middle < other.box.bottom && box.left >= other.box.left && box.left < other.box.right;
          if (inside && box.right > other.box.right + 0.5) found.add(`at ${t}: "${text}" runs out of its box`);
        }
      }
      for (let i = 0; i < words.length; i++) {
        for (let j = i + 1; j < words.length; j++) {
          const [a, b] = [words[i], words[j]];
          // One word giving way to itself in another ink stands exactly where it was.
          if (a.text === b.text && Math.abs(a.box.left - b.box.left) < 1 && Math.abs(a.box.top - b.box.top) < 1) continue;
          if (meant.some((pair) => pair.includes(a.text) && pair.includes(b.text) && a.text !== b.text)) continue;
          // A text's box holds room over and under its letters.
          const room = (word) => word.box.height * 0.2;
          const across = Math.min(a.box.right, b.box.right) - Math.max(a.box.left, b.box.left);
          const down = Math.min(a.box.bottom - room(a), b.box.bottom - room(b)) - Math.max(a.box.top + room(a), b.box.top + room(b));
          if (across > 0.5 && down > 0.5) found.add(`at ${t}: "${a.text}" is over "${b.text}"`);
        }
      }
      for (const place of root.querySelectorAll('.film-place span')) {
        if (!place.getClientRects().length || getComputedStyle(place).visibility === 'hidden') continue;
        if (parseFloat(getComputedStyle(place).fontSize) < 10.95) found.add(`the place "${place.textContent}" is under 11 px tall`);
      }
      seen.push({ t, words: words.filter((word) => word.alpha > 0.8).map((word) => word.text), page: document.documentElement.scrollWidth > innerWidth });
    }
    return { found: [...found], seen };
  }, [times, MEANT]);

/** Every word the film can show, at its fullest, whatever the camera frames. */
const allWords = (page, times) =>
  page.evaluate((times) => {
    const root = document.querySelector('.film');
    const all = new Set();
    for (const t of times) {
      root.film.hold(t);
      const walk = (el, opacity) => {
        const style = getComputedStyle(el);
        const now = opacity * Number(style.opacity);
        if (now < 0.9 || style.visibility === 'hidden') return;
        if (el.tagName === 'text') {
          if (el.textContent) all.add(el.textContent);
        } else for (const child of el.children) walk(child, now);
      };
      walk(root.querySelector('.film-view'), 1);
    }
    return [...all];
  }, times);

try {
  if (values['write-still']) {
    await writeFile(STILL_FILE, await still());
    console.log(`wrote ${STILL_FILE}`);
  } else {
    if ((await readFile(STILL_FILE, 'utf8')) !== (await still())) problems.add('src/film/still.html is not the film at the end of Network: write it again with --write-still');

    const times = [];
    for (let t = 0; t < LENGTH; t += STEP) times.push(t);
    for (const screen of SCREENS) {
      const name = screen.join(' x ');
      const { page, close } = await open(screen);
      const before = await placesOf(page);
      const { found, seen } = await readFrames(page, times);
      for (const problem of found) problems.add(`${name}: ${problem}`);
      if ((await placesOf(page)) !== before) problems.add(`${name}: something changed place between two frames`);
      if (screen[0] < 1000) {
        const runs = new Map();
        const best = new Map();
        for (const { words } of seen) {
          for (const word of runs.keys()) if (!words.includes(word)) runs.delete(word);
          for (const word of new Set(words)) {
            const run = (runs.get(word) ?? 0) + STEP;
            runs.set(word, run);
            best.set(word, Math.max(best.get(word) ?? 0, run));
          }
        }
        // A text that types itself or counts down is read as its whole: its beginnings and its steps are not words of their own.
        const all = await allWords(page, times);
        for (const word of all) {
          if (/ GB$/.test(word) && word !== '41.2 GB' && word !== '9.8 GB') continue;
          if (all.some((other) => other !== word && other.startsWith(word))) continue;
          if ((best.get(word) ?? 0) < READ) problems.add(`${name}: "${word}" is whole in view for ${best.get(word) ?? 0} ms`);
        }
      }
      await close();

      const playing = await open(screen, { query: '' });
      const start = await placesOf(playing.page);
      for (let i = 0; i < 8; i++) {
        await playing.page.waitForTimeout(400);
        if ((await placesOf(playing.page)) !== start) problems.add(`${name}: something changed place while it played`);
      }
      await playing.close();
      const plain = await open(screen, { script: false, query: '' });
      const without = await placesOf(plain.page);
      // With its script the film's controls show in the room they kept.
      if (without !== start) problems.add(`${name}: the page stands differently without script:\n${without}\n${start}`);
      await plain.close();
      console.log(`read ${times.length} frames at ${name}`);
    }

    for (const screen of ABOVE_FOLD) {
      const { page, close } = await open(screen, { query: '' });
      const bottom = await page.evaluate(() => document.querySelector('.film-under').getBoundingClientRect().bottom);
      if (bottom > screen[1]) problems.add(`${screen.join(' x ')}: the film's controls end at ${Math.round(bottom)}, below the fold`);
      await close();
    }

    const { page, close } = await open([1440, 900]);
    const widths = new Set([999, 1000, 1064, 1065, 640, 641, 900, 901, 1440]);
    for (let width = 320; width <= 1440; width += 10) widths.add(width);
    for (const width of [...widths].sort((a, b) => a - b)) {
      await page.setViewportSize({ width, height: 844 });
      await page.evaluate(() => new Promise((done) => requestAnimationFrame(() => requestAnimationFrame(() => done(true)))));
      if (await page.evaluate(() => document.documentElement.scrollWidth > innerWidth)) problems.add(`${width} wide: the page scrolls sideways`);
      if (width < 390) continue;
      for (const problem of (await readFrames(page, TIMED.map(endOf))).found) problems.add(`${width} wide: ${problem}`);
    }
    await close();
  }

  if (values.shots) {
    await mkdir(values.shots, { recursive: true });
    for (const screen of SCREENS) {
      for (const theme of ['light', 'dark']) {
        const { page, close } = await open(screen, { theme });
        for (const [i, shot] of TIMED.entries()) {
          await page.evaluate((t) => document.querySelector('.film').film.hold(t), endOf(shot));
          await page.screenshot({ path: join(values.shots, `${screen.join('x')}-${theme}-${String(i + 1).padStart(2, '0')}-${shot.key}.png`) });
        }
        await close();
      }
    }
  }
} finally {
  await browser.close();
}

for (const problem of problems) console.log(`FAIL ${problem}`);
if (problems.size) process.exitCode = 1;
else if (!values['write-still']) console.log('the film passes');
