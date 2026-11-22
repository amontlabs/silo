// The owner's Take and Drop in a real browser, on the page Vite serves from the real plan. The buttons come after `me`
// answers. With the owner remembered, their room is held from the first frame, and their arrival moves nothing at any
// width. On a first visit (nothing remembered of the person) they move nothing wherever they fit beside the state's
// words; where they do not, they go under them. Either way the state's words show whole, never cut. The API is answered
// here; `me` is held until the page has drawn everything else. Skipped when Playwright's Chromium is not installed
// (`npx playwright-core install chromium-headless-shell`).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { chromium } from 'playwright-core';
import { createServer } from 'vite';

const website = fileURLToPath(new URL('..', import.meta.url));
const WIDTHS = [320, 360, 390, 600, 768, 1024, 1440];
const SLACK = 0.5;

test('Take and Drop arriving after me move nothing for the owner remembered, nor on a first visit where they fit, and the state\'s words show whole, at every width', async (t) => {
  let browser;
  try {
    browser = await chromium.launch();
  } catch {
    t.skip('Playwright\'s Chromium is not installed');
    return;
  }
  const cacheDir = await mkdtemp(join(tmpdir(), 'verdict-layout-vite-'));
  const vite = await createServer({ root: website, cacheDir, logLevel: 'error', server: { host: '127.0.0.1', port: 0, strictPort: false, hmr: false, watch: null, fs: { strict: false } } });
  try {
    await vite.listen();
    const base = vite.resolvedUrls.local[0];
    const { default: data } = await vite.ssrLoadModule('virtual:workshop-data');
    if (!data.builtFrom) {
      t.skip('plan/ has changes git sees, so the page names no build and offers no verdict');
      return;
    }
    // Entries the owner may decide, in a part: with and without details to load, the longest id (the widest line), and
    // one the agents decided (the longest state's words, "Decided, by agents.").
    const open = data.entries.filter((entry) => entry.part && (entry.verdict === null || entry.by === 'agents') && !entry.replacedBy && !entry.built);
    assert.ok(open.length > 0, 'the plan has an entry the owner may decide');
    const longest = open.reduce((a, b) => (b.id.length > a.id.length ? b : a));
    const agents = open.find((entry) => entry.by === 'agents' && entry.verdict === 'take');
    const picks = [...new Map([open.find((entry) => entry.more), open.find((entry) => !entry.more), longest, agents].filter(Boolean).map((entry) => [entry.id, entry])).values()];

    for (const entry of picks) {
      for (const [width, visit] of WIDTHS.flatMap((one) => [[one, 'first visit'], [one, 'owner remembered']])) {
        const label = `${entry.id} at ${width} px, ${visit}`;
        const context = await browser.newContext({ viewport: { width, height: 900 } });
        try {
          // Signed in (the session marker); on a first visit nothing is remembered of the person.
          await context.addCookies([{ name: 'workshop-hint', value: '1', url: base }]);
          if (visit === 'owner remembered') await context.addInitScript(() => localStorage.setItem('silo-workshop-me', JSON.stringify({ login: 'the-owner', avatar: '', owner: true })));
          let answerMe;
          const me = new Promise((resolve) => { answerMe = resolve; });
          await context.route('**/api/workshop/**', async (route) => {
            const path = new URL(route.request().url()).pathname.replace('/api/workshop/', '');
            const json = (body) => route.fulfill({ status: 200, contentType: 'application/json', body: JSON.stringify(body) });
            if (path === 'state') return json({ repo: 'amontlabs/silo', fetchedAt: new Date().toISOString(), issues: {}, threads: {} });
            if (path === 'me') {
              await me;
              return json({ login: 'the-owner', avatar: '', owner: true });
            }
            return route.fulfill({ status: 404, contentType: 'application/json', body: '{"error":"not_found","message":"Not found."}' });
          });
          const page = await context.newPage();
          await page.goto(`${base}workshop/#${entry.part}/${entry.id}`);
          // The open entry, as a row of the part (`entry-<id>`) or under the row that holds it (`entry-<id>@<row>`).
          const item = page.locator(`[id="entry-${entry.id}"], [id^="entry-${entry.id}@"]`).filter({ has: page.locator('.ws-state') }).first();
          const state = item.locator('.ws-state').first();
          await state.waitFor();
          // Everything but `me` has drawn: the live state, the entry's details, the fonts.
          await page.waitForFunction(() => document.querySelector('.ws[data-live="answered"]'));
          await state.evaluate((line) => new Promise((resolve) => {
            const panel = line.parentElement;
            const done = () => !panel.querySelector('.ws-ph-notes');
            if (done()) return resolve();
            const watch = new MutationObserver(() => { if (done()) { watch.disconnect(); resolve(); } });
            watch.observe(panel, { childList: true, subtree: true });
          }));
          await page.evaluate(() => document.fonts.ready);
          const measure = () => state.evaluate((line) => {
            // In the page's coordinates, so a scroll is not taken for a move.
            const box = line.getBoundingClientRect();
            const next = line.nextElementSibling?.getBoundingClientRect();
            const words = line.querySelector('.ws-state-text');
            const text = words.getBoundingClientRect();
            const buttons = line.querySelector('.ws-verdict')?.getBoundingClientRect() ?? null;
            return {
              height: box.height,
              below: (next ? next.top : box.bottom) + window.scrollY,
              words: words.textContent,
              // Whole: nothing of the words overflows their box (cut, with an ellipsis or not) or the line, and the buttons
              // do not cover them.
              whole: words.scrollWidth <= words.clientWidth && text.right <= box.right + 0.5
                && (!buttons || buttons.left >= text.right - 0.5 || buttons.top >= text.bottom - 0.5),
              besideWords: buttons ? buttons.top < text.bottom - 0.5 : null,
            };
          });
          const before = await measure();
          assert.ok(before.whole, `${label}: before me, the state's words "${before.words}" are cut`);
          answerMe();
          await item.getByRole('button', { name: /^Take$/ }).waitFor();
          const after = await measure();
          assert.ok(after.whole, `${label}: with the buttons, the state's words "${after.words}" are cut`);
          const still = Math.abs(after.height - before.height) <= SLACK && Math.abs(after.below - before.below) <= SLACK;
          const moves = `${label}: the state line was ${before.height} px high, then ${after.height} px; what is below was at ${before.below}, then ${after.below}`;
          if (visit === 'owner remembered' || after.besideWords) assert.ok(still, moves);
          else t.diagnostic(`${label}: the buttons went under the words "${after.words}"`);
        } finally {
          await context.close();
        }
      }
    }
  } finally {
    await vite.close();
    await browser.close();
    await rm(cacheDir, { recursive: true, force: true });
  }
});
