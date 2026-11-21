#!/usr/bin/env node
// A Workshop page left open across a deployment, driven in a real browser through the whole case, and the same page
// with the network cut instead. `npm run reload-check`; nothing to build first.
//
// Two builds of the plan in plan/ are made, A and B, into a directory of their own. A names its chunks apart (as a
// deployment whose chunks changed does: the site then serves only B's), and each build has its own id. A is served
// with the API against a fake GitHub (scripts/workshop-dev.mjs) and opened, signed in as owner-fake, at a part that
// has a feature with facts, notes and history (F) and a feature with no facts (X). A comment is typed on X. Then:
// - B replaces A. F's facts, Notes and History each say "The Workshop has changed since this page opened." with
//   Reload. The first press does not reload, since the comment is not sent: it says "Text you typed is not sent yet."
//   with "Reload anyway". The second press reloads: the page is B, F is open, and F's facts (a label only B's facts
//   chunk gives), notes and history load. Nothing offers Reload any more.
// - On B, with the network cut, the same parts say they could not load, with the same Reload; with the network back,
//   the two presses give the same page as above.
// It exits non-zero when a check fails. It drives Playwright's own browser: `npx playwright-core install
// chromium-headless-shell` once.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { cp, mkdtemp, readdir, rm, writeFile } from 'node:fs/promises';
import { createServer } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { chromium } from 'playwright-core';
import { buildSite, workshopOf } from './workshop-builds.mjs';
import { startDev } from './workshop-dev.mjs';

const website = fileURLToPath(new URL('..', import.meta.url));
const plan = join(website, '..', 'plan');
const COMMENT = 'Half a thought, typed before the deployment.';
const UNSENT = 'Text you typed is not sent yet.';
const CHANGED = 'The Workshop has changed since this page opened.';

const dir = await mkdtemp(join(tmpdir(), 'silo-reload-check-'));
const site = join(dir, 'site');
// Build A names its chunks apart: a config of its own over the site's (vite.config.ts), read by `vite build --config`.
const configA = join(dir, 'vite.config.a.mjs');
await writeFile(configA, `import { mergeConfig } from ${JSON.stringify(join(website, 'node_modules', 'vite', 'dist', 'node', 'index.js'))};
import config from ${JSON.stringify(join(website, 'vite.config.ts'))};
export default mergeConfig(config, { build: { rolldownOptions: { output: { chunkFileNames: 'assets/[name]-a-[hash].js' } } } });
`);
await Promise.all([buildSite(join(dir, 'a'), plan, ['--config', configA]), buildSite(join(dir, 'b'), plan)]);
const A = await workshopOf(join(dir, 'a'));
const B = await workshopOf(join(dir, 'b'));
assert.notEqual(A.id, B.id);

/** Serves a build at the site's root, in place of what was there: the files of the build before are gone. */
async function deploy(build) {
  await rm(site, { recursive: true, force: true });
  await cp(join(dir, build), site, { recursive: true });
}

const port = await new Promise((resolve) => {
  const probe = createServer().listen(0, '127.0.0.1', () => {
    const { port: free } = probe.address();
    probe.close(() => resolve(free));
  });
});
await deploy('a');
const dev = await startDev({ fake: true, planDir: plan, port, distDir: site, log: () => {} });
const browser = await chromium.launch();
const context = await browser.newContext({ viewport: { width: 1440, height: 1000 } });

test.after(async () => {
  await context.close();
  await browser.close();
  await dev.close();
  await rm(dir, { recursive: true, force: true });
});

/** The id of the build the page runs, from its page. */
const buildOfPage = (page) => page.evaluate(() => document.querySelector('meta[name="workshop-build"]')?.getAttribute('content') ?? null);
/** What an entry's row shows once open: its panel, under the row's line in a part's list. */
const item = (page, id) => page.locator(`[id="entry-${id}-more"]`);
const row = (page, id) => page.locator(`[data-row="entry-${id}"]`);

/**
 * A part with a feature whose facts, notes and history the page loads later (F), and a feature with no facts (X).
 * F is one whose facts give a label the check can look for after the reload (`labelOf`), so that the choice and
 * what is looked for always agree.
 */
async function chooseEntries(page) {
  const parts = await page.evaluate(() => {
    const data = window.__workshopData;
    return data.parts.map((part) => {
      const features = data.entries.filter((entry) => entry.part === part.id && entry.kind === 'feature');
      return {
        part: part.id,
        later: features.filter((entry) => entry.more && entry.notes && entry.history).map((entry) => entry.id),
        plain: features.filter((entry) => !entry.more).map((entry) => entry.id),
      };
    });
  });
  const facts = await factsOfB();
  for (const { part, later, plain } of parts) {
    const f = later.find((id) => labelOf(facts[id]));
    const x = plain.find((id) => id !== f);
    if (f && x) return { part, f, x };
  }
  assert.fail('the plan has no part with a feature whose facts give a label to look for, with notes and history, and a feature with no facts');
}

async function signIn() {
  const page = await context.newPage();
  await page.goto(`${dev.origin}/api/workshop/login?return=${encodeURIComponent('/workshop')}`);
  await page.getByText('Sign in as owner-fake').click();
  await page.waitForURL((next) => next.pathname === '/workshop', { timeout: 30000 });
  await page.waitForSelector('.ws-login');
  await page.close();
}

/** Opens the page in a new tab at X and types a comment on it, not sent. */
async function draft(entries) {
  const page = await context.newPage();
  await page.goto(`${dev.origin}/workshop#${entries.part}/${entries.x}`);
  await page.waitForSelector('.ws[data-live="answered"]');
  await assertOpen(page, entries.x);
  await item(page, entries.x).getByRole('textbox', { name: 'Comment' }).fill(COMMENT);
  return page;
}

async function assertOpen(page, id) {
  await page.waitForFunction((key) => document.querySelector(`[data-row="${key}"] > .wa-hit`)?.getAttribute('aria-expanded') === 'true', `entry-${id}`, { timeout: 15000 });
}

/** Opens F, then its Notes and History; each of its three parts says `words` with Reload. */
async function failingParts(page, entries, words) {
  const f = item(page, entries.f);
  await row(page, entries.f).click();
  await assertOpen(page, entries.f);
  await f.getByRole('button', { name: 'Notes', exact: true }).click();
  await f.getByRole('button', { name: /^History/ }).click();
  await f.getByRole('button', { name: 'Reload' }).nth(2).waitFor({ timeout: 20000 });
  assert.equal(await f.getByRole('button', { name: 'Reload' }).count(), 3, 'facts, Notes and History each offer Reload');
  const texts = await f.locator('p.ws-quiet:has(button)').allTextContents();
  assert.deepEqual(texts.map((text) => text.replace(/\s*Reload$/, '')), words);
}

/** Presses Reload with the comment not sent: the first press says so and stays; the second reloads. */
async function reloadAnyway(page, entries) {
  const f = item(page, entries.f);
  const loads = [];
  page.on('load', () => loads.push(Date.now()));
  await f.getByRole('button', { name: 'Reload' }).first().click();
  await f.getByText(UNSENT).waitFor();
  assert.equal(await f.getByRole('button', { name: 'Reload anyway' }).count(), 1);
  assert.equal(await item(page, entries.x).getByRole('textbox', { name: 'Comment' }).inputValue(), COMMENT, 'the comment is still in its box');
  assert.equal(loads.length, 0, 'the first press does not reload');
  await Promise.all([page.waitForEvent('load'), f.getByRole('button', { name: 'Reload anyway' }).click()]);
}

/** What build B's facts chunk says of each entry. */
async function factsOfB() {
  const name = (await readdir(join(dir, 'b', 'assets'))).find((file) => /workshop-facts.*\.js$/.test(file));
  assert.ok(name, "build B's facts chunk");
  const { default: facts } = await import(pathToFileURL(join(dir, 'b', 'assets', name)).href);
  return facts.entries;
}

/** A label of an entry's open view that only the facts chunk gives, or null when its facts give none the check knows. */
const labelOf = (own) => (own?.keeps?.length ? 'Must keep' : own?.surfaces ? 'Where it shows' : own?.suggest ? `Recommended: ${own.suggest.verdict}` : null);

/** The label of F to look for after the reload: read from build B's facts chunk. */
async function factLabel(entries) {
  const label = labelOf((await factsOfB())[entries.f]);
  assert.ok(label, `the facts of ${entries.f} have no label to look for`);
  return label;
}

/** After the reload: the page is B, F is open, F's parts load, and nothing offers Reload. */
async function afterReload(page, entries) {
  await page.waitForSelector('.ws[data-live="answered"]');
  assert.equal(await buildOfPage(page), B.id, 'the page is build B');
  await assertOpen(page, entries.f);
  const f = item(page, entries.f);
  await f.locator('.ws-fact-label', { hasText: await factLabel(entries) }).first().waitFor();
  await f.getByRole('button', { name: 'Notes', exact: true }).click();
  await f.locator('.ws-md').waitFor();
  await f.getByRole('button', { name: /^History/ }).click();
  await f.locator('.ws-h').first().waitFor();
  assert.equal(await page.getByRole('button', { name: /^Reload/ }).count(), 0, 'nothing offers Reload again');
}

await signIn();
const entries = await (async () => {
  const page = await context.newPage();
  await page.goto(`${dev.origin}/workshop`);
  await page.waitForSelector('.ws[data-live="answered"]');
  const chosen = await chooseEntries(page);
  await page.close();
  return chosen;
})();

test('a page left open across a deployment says so, and Reload asks twice while a comment is not sent', async () => {
  const page = await draft(entries);
  assert.equal(await buildOfPage(page), A.id, 'the page opened is build A');
  await deploy('b');
  await failingParts(page, entries, [CHANGED, CHANGED, CHANGED]);
  await reloadAnyway(page, entries);
  await afterReload(page, entries);
  await page.close();
});

test('a page whose network was cut says its parts could not load, and the same Reload brings it back once it is', async () => {
  const page = await draft(entries);
  assert.equal(await buildOfPage(page), B.id);
  await context.setOffline(true);
  await failingParts(page, entries, ['Details could not load.', 'Notes could not load.', 'History could not load.']);
  await context.setOffline(false);
  await reloadAnyway(page, entries);
  await afterReload(page, entries);
  await page.close();
});
