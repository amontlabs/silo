// The seam reading on made-up pictures, and what the player's soft shapes must keep doing. The reading itself, in
// the three engines, is `npm run film-seams`, which needs a site to read and their browsers.
import assert from 'node:assert/strict';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { createServer } from 'node:http';
import { extname, join } from 'node:path';
import { tmpdir } from 'node:os';
import { after, test } from 'node:test';
import { chromium, firefox, webkit } from 'playwright-core';
import { writeFilms } from './films.mjs';
import { LIMIT, seamStrengths } from './film-seams.mjs';

const W = 60;
const H = 40;
/** A picture of a soft shape's edge: levels falling smoothly from the left, with `mark` set at one column. */
function picture(mark) {
  const gray = [];
  for (let y = 0; y < H; y += 1) for (let x = 0; x < W; x += 1) gray.push(mark && x === mark.x ? mark.level : 230 - x * 0.5);
  return gray;
}

test('a smooth falloff has no seam on a line across it', () => {
  const [d] = seamStrengths(picture(null), W, H, { xs: [30], ys: [] });
  assert.ok(Math.abs(d) < 1, `read ${d}`);
});

test('a dark column reads negative and a light one positive, by how far it stands out', () => {
  const dark = seamStrengths(picture({ x: 30, level: 230 - 15 - 30 * 0.5 }), W, H, { xs: [30], ys: [] });
  assert.ok(dark[0] < -LIMIT, `read ${dark[0]}`);
  const light = seamStrengths(picture({ x: 30, level: 230 + 4 - 30 * 0.5 }), W, H, { xs: [30], ys: [] });
  assert.ok(light[0] > LIMIT * 0.8 && light[0] < LIMIT * 1.6, `read ${light[0]}`);
});

test('a line only part of the way along a shape is read by what most of it does, and one near the edge is skipped', () => {
  const gray = picture(null);
  for (let y = 0; y < 5; y += 1) gray[y * W + 30] -= 40;
  const [d] = seamStrengths(gray, W, H, { xs: [30], ys: [] });
  assert.ok(Math.abs(d) < 1, `read ${d}`);
  assert.deepEqual(seamStrengths(picture(null), W, H, { xs: [2, W - 2], ys: [1] }), []);
});

const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.json': 'application/json', '.svg': 'image/svg+xml', '.woff2': 'font/woff2', '.mp3': 'audio/mpeg' };

/**
 * The films' pages, written by the step the site's build uses into a folder of their own, and served from this process:
 * the player is the one of the tree the test runs in, whatever was built before. One site for all the tests.
 */
let site = null;
function serve() {
  site ||= (async () => {
    const dist = await mkdtemp(join(tmpdir(), 'film-seams-'));
    await writeFilms(dist);
    const server = createServer(async (req, res) => {
      const path = decodeURIComponent(new URL(req.url, 'http://x').pathname);
      const file = join(dist, path.endsWith('/') ? `${path}index.html` : path);
      const body = file.startsWith(dist) ? await readFile(file).catch(() => null) : null;
      if (!body) { res.writeHead(404).end(); return; }
      res.writeHead(200, { 'content-type': TYPES[extname(file)] || 'application/octet-stream' }).end(body);
    });
    await new Promise((done) => server.listen(0, '127.0.0.1', done));
    return { server, dist, base: `http://127.0.0.1:${server.address().port}` };
  })();
  return site;
}
after(async () => {
  if (!site) return;
  const { server, dist } = await site;
  server.close();
  await rm(dist, { recursive: true, force: true });
});

/** Runs in the page of a film: where the lines each soft shape's tiles meet along fall, in device pixels of the stage's grid. */
const LINES = `() => {
  const stage = document.querySelector('#stage').getBoundingClientRect(), dpr = window.devicePixelRatio;
  return window.__film.softs.flatMap((g) => {
    const [tl, , , br] = [...g.children].map((r) => r.getBoundingClientRect());
    return [(tl.right - stage.left) * dpr, (br.left - stage.left) * dpr, (tl.bottom - stage.top) * dpr, (br.top - stage.top) * dpr];
  });
}`;
const apart = (v) => Math.abs(v - Math.round(v));

// The reading in the three engines is `npm run film-seams`. This is the part of it that does not need their pixels: the
// player puts the lines of every soft shape on whole device pixels when the film is at rest, in each engine that is
// installed (skipped for one Playwright does not have). It reads no build of the site: it writes the pages itself.
for (const [name, engine] of Object.entries({ chromium, webkit, firefox })) {
  test(`${name}: at rest, the lines the tiles of a soft shape meet along are on whole device pixels`, async (t) => {
    let browser;
    try {
      browser = await engine.launch();
    } catch {
      t.skip(`${name} is not installed`);
      return;
    }
    const { base } = await serve();
    try {
      for (const scale of [1, 2, 3]) {
        const context = await browser.newContext({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: scale });
        const page = await context.newPage();
        await page.goto(`${base}/films/engine-one-call/`);
        await page.waitForSelector('body[data-ready="1"]');
        // The end of the second scene: the film is held there, with shadows on stage.
        await page.evaluate(() => { window.__player.pause(); window.__player.seek(window.__player.starts[2] - 0.05); });
        assert.ok(await page.evaluate(() => window.__film.rest()), 'the film is at rest there');
        // The tiles as the film draws them while it moves: lines at fractions of a pixel.
        await page.evaluate(() => window.__film.softs.forEach((g) => g.place()));
        const before = await page.evaluate(`(${LINES})()`);
        assert.ok(before.length >= 8, 'there are soft shapes on stage');
        assert.ok(before.some((v) => apart(v) > 0.1), `x${scale}: some line is at a fraction of a pixel before the snap`);
        await page.evaluate(() => window.__film.snap());
        const after = await page.evaluate(`(${LINES})()`);
        const worst = Math.max(...after.map(apart));
        assert.ok(worst < 0.06, `x${scale}: every line is on a whole pixel after it; the worst is ${worst.toFixed(3)} off`);
        // The shape that leaves the stage leaves the list.
        const kept = await page.evaluate(() => { const n = window.__film.softs.length; window.__film.softs[0].remove(); window.__film.snap(); return [n, window.__film.softs.length]; });
        assert.deepEqual(kept, [kept[0], kept[0] - 1]);
        await context.close();
      }
    } finally {
      await browser.close();
    }
  });
}
