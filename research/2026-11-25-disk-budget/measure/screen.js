// How many bytes an hour a recording of computer use takes.
//
// An agent drives a browser for a few minutes: it opens pages, fills a form, scrolls a long page
// and types into an editor. Playwright records the screen (VP8, its own fixed quality); the
// recording is then encoded again by ffmpeg in the codecs a Mac recorder would use, at the frame
// rates worth keeping. Pages are local, so nothing here reaches the network.
//
// Usage: node screen.js <out-dir> [seconds]
const { chromium } = require('playwright');
const fs = require('fs');
const path = require('path');

const out = path.resolve(process.argv[2] || 'out');
const seconds = Number(process.argv[3] || 180);
fs.mkdirSync(out, { recursive: true });

const words = 'the quick brown fox jumps over the lazy dog while agents build and test the app'.split(' ');
const para = (n, seed) => Array.from({ length: n }, (_, i) => words[(i * 7 + seed) % words.length]).join(' ');

const pages = {
  form: `<html><body style="font:16px system-ui;margin:40px;background:#fafafa">
    <h1>Create an account</h1>
    ${['Name', 'Email', 'Company', 'Address', 'City', 'Notes'].map((l) =>
      `<p><label>${l}<br><input name="${l}" style="width:420px;font-size:16px;padding:6px"></label></p>`).join('')}
    <p><select name="plan"><option>Free</option><option>Team</option><option>Business</option></select></p>
    <button style="font-size:16px;padding:8px 16px">Submit</button></body></html>`,
  article: `<html><body style="font:17px Georgia;margin:40px auto;max-width:760px">
    ${Array.from({ length: 60 }, (_, i) => `<h3>Section ${i + 1}</h3><p>${para(120, i)}</p>`).join('')}</body></html>`,
  editor: `<html><body style="margin:0;background:#1e1e1e;color:#ddd">
    <textarea id="t" style="width:100vw;height:100vh;background:#1e1e1e;color:#d4d4d4;font:14px monospace;border:0;padding:20px"></textarea></body></html>`,
  table: `<html><body style="font:13px system-ui;margin:20px"><table border=1 cellspacing=0 cellpadding=4>
    ${Array.from({ length: 200 }, (_, r) => `<tr>${Array.from({ length: 10 }, (_, c) => `<td>${(r * 31 + c * 17) % 997}</td>`).join('')}</tr>`).join('')}</table></body></html>`,
};
for (const [k, v] of Object.entries(pages)) fs.writeFileSync(path.join(out, `${k}.html`), v);

(async () => {
  const browser = await chromium.launch();
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    recordVideo: { dir: out, size: { width: 1440, height: 900 } },
  });
  const page = await context.newPage();
  const t0 = Date.now();
  const left = () => seconds * 1000 - (Date.now() - t0);
  const open = (k) => page.goto('file://' + path.join(out, `${k}.html`));
  let round = 0;
  while (left() > 0) {
    await open('form');
    for (const l of ['Name', 'Email', 'Company', 'Address', 'City', 'Notes']) {
      await page.click(`input[name="${l}"]`);
      await page.keyboard.type(`${l.toLowerCase()} ${round}`, { delay: 60 });
      await page.waitForTimeout(400);
    }
    await page.selectOption('select', 'Team');
    await page.waitForTimeout(1500);
    if (left() <= 0) break;
    await open('article');
    for (let i = 0; i < 12 && left() > 0; i++) { await page.mouse.wheel(0, 600); await page.waitForTimeout(800); }
    await open('table');
    for (let i = 0; i < 6 && left() > 0; i++) { await page.mouse.wheel(0, 500); await page.waitForTimeout(1200); }
    await open('editor');
    await page.click('#t');
    for (let i = 0; i < 6 && left() > 0; i++) {
      await page.keyboard.type(`function step${round}_${i}(x) {\n  return x * ${i} + ${round};\n}\n`, { delay: 25 });
      await page.waitForTimeout(700);
    }
    // the agent reads: nothing moves for a while, as between turns
    await page.waitForTimeout(Math.min(8000, Math.max(0, left())));
    round++;
  }
  const video = page.video();
  await context.close();
  await browser.close();
  const p = await video.path();
  fs.renameSync(p, path.join(out, 'session.webm'));
  console.log(JSON.stringify({ seconds, file: 'session.webm', bytes: fs.statSync(path.join(out, 'session.webm')).size }));
})();
