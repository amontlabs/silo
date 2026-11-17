// The build writes the Workshop's plain view into its page (scripts/prerender.mjs): built here into a directory of
// its own, then read as a browser without JavaScript would, and handed to prepare-vercel.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFile } from 'node:child_process';
import { cp, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { promisify } from 'node:util';
import { fileURLToPath } from 'node:url';
import { prepareVercel } from './prepare-vercel.mjs';

const run = promisify(execFile);
const website = fileURLToPath(new URL('..', import.meta.url));
const areas = JSON.parse(await readFile(join(website, '..', 'plan', 'areas.json'), 'utf8'));
const escape = (text) => text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

const dir = await mkdtemp(join(tmpdir(), 'silo-prerender-'));
const dist = join(dir, 'dist');
test.after(() => rm(dir, { recursive: true, force: true }));

const vite = join(website, 'node_modules', 'vite', 'bin', 'vite.js');
await run(process.execPath, [vite, 'build', '--logLevel', 'error', '--outDir', dist], { cwd: website });
await run(process.execPath, [join(website, 'scripts', 'prerender.mjs'), dist], { cwd: website });
const html = await readFile(join(dist, 'workshop', 'index.html'), 'utf8');
const between = (open, close) => html.slice(html.indexOf(open) + open.length, html.indexOf(close, html.indexOf(open)));

test('the built page holds the plain view: the title row, the map with every area, the tiles and the row under them', () => {
  const root = between('<div id="workshop-root" data-prerendered>', '<noscript>');
  assert.match(root, /^<div class="ws is-sheet v-map ws-boot" data-live="pending">/);
  assert.match(root, /<h1>Workshop<\/h1>/);
  assert.match(root, /class="ws-status"/);
  for (const area of areas) {
    assert.match(root, new RegExp(`data-part="${area.id}"`), `the map's part for ${area.id}`);
    assert.match(root, new RegExp(`data-area="${area.id}"`), `the tile for ${area.id}`);
    assert.ok(root.includes(escape(area.title)), `the title of ${area.id}`);
  }
  assert.match(root, /class="ws-under"/);
  assert.match(root, /href="#latest"/);
  assert.match(root, /class="ws-btn ws-suggest" href="#suggested">Suggest a feature</);
  // Nothing of the account is drawn inside the root: it has its place in the header.
  assert.doesNotMatch(root, /ws-account-inline|Sign in/);
});

test('the header holds sign-in with its tooltip, returning to the plain address', () => {
  const slot = between('<div class="ws-account-slot" id="workshop-account" data-prerendered>', '</div>');
  assert.match(slot, /^<span class="ws-signin-wrap"><a class="button small" href="\/api\/workshop\/login\?return=%2Fworkshop" aria-describedby="ws-signin-tip">/);
  assert.match(slot, /<span class="ws-signin-tip" id="ws-signin-tip" role="tooltip">or comment directly on GitHub<\/span><\/span>$/);
  assert.equal(html.match(/Sign in/g).length, 1, 'one sign-in button');
});

test('without JavaScript a line says what needs it, and every face the page waits for is preloaded', () => {
  assert.match(html, /<noscript><p class="ws-quiet">Voting, commenting and opening an area need JavaScript.<\/p><\/noscript>/);
  for (const face of ['serif-400-italic', 'serif-400-normal', 'sans-400-normal', 'mono-400-normal']) {
    assert.match(html, new RegExp(`<link rel="preload" href="/fonts/ibm-plex-${face}.woff2" as="font"`), face);
  }
  // The fonts gate runs after the fonts' stylesheet and before the page's module script, not at DOMContentLoaded.
  const gate = html.indexOf('document.fonts.load');
  assert.ok(gate > html.indexOf('/fonts/fonts.css') && gate < html.indexOf('type="module"'));
  assert.doesNotMatch(html, /DOMContentLoaded/);
});

test('prepare-vercel takes the new build output', async () => {
  const outputDir = join(dir, '.vercel', 'output');
  await prepareVercel({ siteDir: website, distDir: dist, outputDir });
  const served = await readFile(join(outputDir, 'static', 'workshop', 'index.html'), 'utf8');
  assert.equal(served, html);
});

test('a prerender that cannot place the markup fails, and leaves the page as it was', async () => {
  const broken = join(dir, 'broken');
  await cp(dist, broken, { recursive: true });
  const page = join(broken, 'workshop', 'index.html');
  await writeFile(page, '<!doctype html><title>Workshop</title><main></main>');
  await assert.rejects(run(process.execPath, [join(website, 'scripts', 'prerender.mjs'), broken], { cwd: website }), /must hold/);
  assert.equal(await readFile(page, 'utf8'), '<!doctype html><title>Workshop</title><main></main>');
});
