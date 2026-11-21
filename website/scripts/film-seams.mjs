#!/usr/bin/env node
// Looks for seams in the films' soft shapes (shadows and glows) on the built site, in the browsers' own engines.
//   node scripts/film-seams.mjs [url] [--browsers webkit,chromium,firefox] [--films id,id] [--quick]
// The url defaults to the `workshop:dev --fake` server (`npm run build` first, then `npm run workshop:dev -- --fake`).
// A soft shape is nine gradient tiles (films/player/kit.js, `soft`). Where two tiles meet, an engine can draw a faint or
// a dark hairline. For each film, at its first picture and three others, in both themes, at 1440 and 390 px wide and at
// the device scales 1, 2 and 3 (--quick: 1440 px light at 2 and 390 px dark at 3), each soft shape is shown alone on the
// page's colour, cut out, and read on the four lines its tiles meet along: by how many levels (of 255) the line stands
// out from the pixels 3 device px to either side, along its whole length (the median, so a crossing shape is not
// counted). It exits non-zero when a line stands out by more than LIMIT levels.
// It drives Playwright's own browsers: `npx playwright-core install webkit firefox chromium-headless-shell` once,
// and `npx playwright-core uninstall --all` removes them again.
import { fileURLToPath } from 'node:url';
import { chromium, firefox, webkit } from 'playwright-core';

/** How far a seam may stand out, in levels of 255. The edge of a smooth shape reads up to 2; an overlap or a gap, 5 and more. */
export const LIMIT = 3;
const ENGINES = { webkit, chromium, firefox };
const FILMS = ['engine-one-call', 'what-silo-knows', 'actions-and-kinds', 'rules-and-precautions', 'extensions'];
const ALL = [1440, 390].flatMap((width) => ['light', 'dark'].flatMap((scheme) => [1, 2, 3].map((scale) => ({ width, scheme, scale }))));
const QUICK = [{ width: 1440, scheme: 'light', scale: 2 }, { width: 390, scheme: 'dark', scale: 3 }];
/** Shapes read per picture, spread over those on stage. */
const SHAPES = 10;
/** A shape cut out smaller than this (device px) is too small to read a line on. */
const SMALLEST = 80;

/**
 * How far each line stands out in a picture of one soft shape alone. `gray` is the picture's levels, row after row,
 * `w` wide and `h` high; `xs` and `ys` are the columns and rows its tiles meet along (device px). Gives one signed
 * number per line: positive for a lighter line, negative for a darker one.
 */
export function seamStrengths(gray, w, h, { xs, ys }) {
  const median = (values) => {
    const sorted = [...values].sort((a, b) => a - b);
    return sorted[sorted.length >> 1];
  };
  const at = (x, y) => gray[y * w + x];
  const out = [];
  const read = (length, line, across) => {
    let best = 0;
    for (let o = -1; o <= 1; o += 1) {
      const d = median(Array.from({ length }, (_, k) => across(k, line + o) - (across(k, line - 3) + across(k, line + 3)) / 2));
      if (Math.abs(d) > Math.abs(best)) best = d;
    }
    out.push(best);
  };
  for (const x of xs) {
    const xi = Math.round(x);
    if (xi >= 4 && xi <= w - 5 && h > 8) read(h, xi, (y, col) => at(col, y));
  }
  for (const y of ys) {
    const yi = Math.round(y);
    if (yi >= 4 && yi <= h - 5 && w > 8) read(w, yi, (x, row) => at(x, row));
  }
  return out;
}

/**
 * Runs in the page of a film. Hides everything but the soft shapes, and remembers which of them are whole on stage (clear of the frame's border) and
 * opaque; gives each one's bounds and the lines its tiles meet along, in CSS px of the page.
 */
const ISOLATE = `({ shapes }) => {
  const stage = document.querySelector('#stage');
  const tiled = (g) => g.children.length === 9 && /^url\\(#fj-s/.test(g.children[0].getAttribute('fill') || '');
  const all = [...stage.querySelectorAll('g')].filter(tiled);
  const bounds = stage.getBoundingClientRect();
  const found = all.filter((g) => {
    let opacity = 1;
    for (let e = g; e && e !== stage.parentNode; e = e.parentNode) opacity *= +getComputedStyle(e).opacity;
    const u = [...g.children].map((r) => r.getBoundingClientRect()).reduce((a, r) => ({ left: Math.min(a.left, r.left), top: Math.min(a.top, r.top), right: Math.max(a.right, r.right), bottom: Math.max(a.bottom, r.bottom) }), { left: 1e9, top: 1e9, right: -1e9, bottom: -1e9 });
    return opacity >= 0.95 && u.right - u.left > 6 && u.left > bounds.left + 8 && u.top > bounds.top + 8 && u.right < bounds.right - 8 && u.bottom < bounds.bottom - 8;
  });
  window.__shapes = found.length > shapes ? found.filter((_, i) => i % Math.ceil(found.length / shapes) === 0) : found;
  const keep = new Set(all.flatMap((g) => [...g.children]));
  all.forEach((g) => { for (let e = g; e && e !== stage.parentNode; e = e.parentNode) keep.add(e); });
  stage.querySelectorAll('*').forEach((e) => { if (!keep.has(e)) e.style.visibility = 'hidden'; });
  const play = document.querySelector('.bigplay');
  if (play) play.style.visibility = 'hidden';
  all.forEach((g) => [...g.children].forEach((t) => { t.style.visibility = 'hidden'; }));
  return window.__shapes.map((g) => {
    const rects = [...g.children].map((r) => r.getBoundingClientRect());
    const u = rects.reduce((a, r) => ({ l: Math.min(a.l, r.left), t: Math.min(a.t, r.top), r: Math.max(a.r, r.right), b: Math.max(a.b, r.bottom) }), { l: 1e9, t: 1e9, r: -1e9, b: -1e9 });
    // The lines come from the corner tiles, which always have a size; a shape whose middle tile is empty has none to give.
    return { u: [u.l, u.t, u.r, u.b], m: [rects[0].right, rects[0].bottom, rects[3].left, rects[3].top] };
  });
}`;

/** Runs in the page of a film: shows only the `j`th shape of those ISOLATE kept. */
const ONLY = `(j) => {
  document.querySelectorAll('#stage [data-on]').forEach((t) => { t.style.visibility = 'hidden'; delete t.dataset.on; });
  [...window.__shapes[j].children].forEach((t) => { t.style.visibility = 'visible'; t.dataset.on = '1'; });
}`;

/** Runs in a blank page, given `seamStrengths`: reads each picture (PNG, base64) as levels of grey and gives the strengths of its lines. */
const DECODE = `async ({ shots, cuts }, strengths) => {
  const out = [];
  for (const [k, b64] of shots.entries()) {
    const img = new Image();
    img.src = 'data:image/png;base64,' + b64;
    await img.decode();
    const canvas = document.createElement('canvas');
    canvas.width = img.width; canvas.height = img.height;
    const ctx = canvas.getContext('2d');
    ctx.drawImage(img, 0, 0);
    const px = ctx.getImageData(0, 0, img.width, img.height).data;
    const gray = new Array(img.width * img.height);
    for (let i = 0; i < gray.length; i += 1) gray[i] = (px[i * 4] + px[i * 4 + 1] + px[i * 4 + 2]) / 3;
    out.push(strengths(gray, img.width, img.height, cuts[k]));
  }
  return out;
}`;

/** The pictures of one film at its first moment and three others: the strengths of every line read, with where. */
async function readFilm(page, reader, base, film, scale) {
  await page.goto(`${base}/films/${film}/`);
  await page.waitForSelector('body[data-ready="1"]', { timeout: 30000 });
  const starts = await page.evaluate(() => window.__player.starts);
  const total = await page.evaluate(() => window.__timing.total);
  const n = starts.length;
  const end = (k) => (k + 1 < n ? starts[k + 1] : total) - 0.05;
  const found = [];
  for (const [moment, t] of [0.02, end(Math.floor(n / 3)), end(Math.floor((2 * n) / 3)), end(n - 1)].entries()) {
    await page.evaluate((t) => { window.__player.pause(); window.__player.seek(t); }, t);
    await page.waitForTimeout(120);
    const shapes = await page.evaluate(`(${ISOLATE})(${JSON.stringify({ shapes: SHAPES })})`);
    const shots = [];
    const cuts = [];
    for (const [j, { u, m }] of shapes.entries()) {
      const clip = { x: Math.floor(u[0]) - 3, y: Math.floor(u[1]) - 3, width: Math.ceil(u[2]) - Math.floor(u[0]) + 6, height: Math.ceil(u[3]) - Math.floor(u[1]) + 6 };
      if (clip.width * scale < SMALLEST || clip.height * scale < SMALLEST) continue;
      await page.evaluate(`(${ONLY})(${j})`);
      shots.push((await page.screenshot({ clip })).toString('base64'));
      cuts.push({ xs: [(m[0] - clip.x) * scale, (m[2] - clip.x) * scale], ys: [(m[1] - clip.y) * scale, (m[3] - clip.y) * scale] });
    }
    const strengths = shots.length ? await reader.evaluate(`(${DECODE})(${JSON.stringify({ shots, cuts })}, ${seamStrengths})`) : [];
    strengths.forEach((ds, k) => ds.forEach((d) => found.push({ film, moment, shape: k, d })));
  }
  return found;
}

/** Reads every film in every combination in one browser; gives the lines that stand out by more than LIMIT. */
export async function check({ base, browser: name, films = FILMS, combos = ALL, log = () => {} }) {
  const browser = await ENGINES[name].launch();
  const reader = await (await browser.newContext()).newPage();
  const bad = [];
  try {
    for (const combo of combos) {
      const context = await browser.newContext({ viewport: { width: combo.width, height: combo.width > 600 ? 900 : 844 }, deviceScaleFactor: combo.scale, colorScheme: combo.scheme });
      let light = 0;
      let dark = 0;
      let lines = 0;
      for (const film of films) {
        const page = await context.newPage();
        for (const f of await readFilm(page, reader, base, film, combo.scale)) {
          lines += 1;
          light = Math.max(light, f.d);
          dark = Math.min(dark, f.d);
          if (Math.abs(f.d) > LIMIT) bad.push({ ...combo, ...f });
        }
        await page.close();
      }
      await context.close();
      log(`${name} ${combo.width} px ${combo.scheme} x${combo.scale}: ${lines} lines, lightest +${light.toFixed(2)}, darkest ${dark.toFixed(2)}`);
    }
  } finally {
    await browser.close();
  }
  return bad;
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const args = process.argv.slice(2);
  const option = (name) => (args.includes(name) ? args[args.indexOf(name) + 1] : null);
  const base = (args.find((a) => /^https?:/.test(a)) || 'http://127.0.0.1:4174').replace(/\/$/, '');
  const browsers = (option('--browsers') || 'webkit,chromium,firefox').split(',');
  const films = option('--films')?.split(',') || FILMS;
  let failed = false;
  for (const browser of browsers) {
    const bad = await check({ base, browser, films, combos: args.includes('--quick') ? QUICK : ALL, log: console.log });
    for (const b of bad.slice(0, 10)) console.log(`  SEAM ${b.d.toFixed(1)} levels: ${browser} ${b.width} px ${b.scheme} x${b.scale}, ${b.film}, picture ${b.moment}, shape ${b.shape}`);
    if (bad.length) failed = true;
  }
  console.log(failed ? `FAIL: a seam stands out by more than ${LIMIT} levels` : `ok: no seam stands out by more than ${LIMIT} levels`);
  process.exit(failed ? 1 : 0);
}
