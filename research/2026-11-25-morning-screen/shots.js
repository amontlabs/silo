// Pictures and the scripted check of the page: text cut or overlapping, sideways scroll, smallest
// text, errors. node shots.js <folder with the pages> <folder for the pictures>
const { chromium } = require('playwright');
const SHOTS = [
  ['morning.html', 'first-screen', 1280, 860, 'light', 'main:f481'],
  ['morning.html', 'first-screen-dark', 1280, 860, 'dark', 'main:f481'],
  ['morning.html', 'receipt-claude-1', 1280, 940, 'light', 'words:claude #1'],
  ['morning.html', 'tls-refused', 1280, 860, 'light', 'tls:codex #2'],
  ['morning.html', 'new-hosts', 1280, 860, 'light', 'hosts'],
  ['injected.html', 'injected-words', 1280, 940, 'light', 'words:claude #1'],
  ['option-a.html', 'option-a', 1280, 860, 'light', null],
  ['option-a.html', 'option-a-receipt', 1280, 940, 'light', 'run:claude #1'],
  ['morning.html', 'narrow', 390, 1400, 'light', 'words:claude #1'],
  ['state-empty.html', 'empty', 1280, 700, 'light', null],
  ['state-hundreds.html', 'hundreds', 1280, 860, 'light', null],
];
(async () => {
  const [dir, out] = process.argv.slice(2);
  const b = await chromium.launch();
  let bad = 0;
  for (const [file, name, w, h, scheme, open] of SHOTS) {
    const p = await b.newPage({ viewport: { width: w, height: h }, deviceScaleFactor: 2, colorScheme: scheme });
    const errs = [];
    p.on('pageerror', e => errs.push(e.message));
    await p.goto('file://' + dir + '/' + file + (open ? '?open=' + encodeURIComponent(open) : ''));
    await p.evaluate(() => { try { localStorage.clear(); } catch (e) {} });
    await p.waitForTimeout(150);
    const check = await p.evaluate(() => {
      const sideways = document.documentElement.scrollWidth > innerWidth + 1;
      let smallest = 99;
      const cut = [];
      for (const el of document.querySelectorAll('.what, .t, .lines .sec, h3, .said, .chain li, .tbl span')) {
        smallest = Math.min(smallest, parseFloat(getComputedStyle(el).fontSize));
        if (el.scrollWidth > el.clientWidth + 1 && getComputedStyle(el).textOverflow !== 'ellipsis') cut.push(el.textContent.slice(0, 40));
      }
      return { sideways, smallest, cut: cut.slice(0, 5) };
    });
    if (errs.length || check.sideways || check.cut.length || check.smallest < 12) bad++;
    console.log(name, JSON.stringify({ errs, ...check }));
    if (!['option-a', 'empty', 'hundreds'].includes(name)) await p.screenshot({ path: `${out}/${name}.png`, fullPage: name === 'narrow' });  // checked, not kept
    await p.close();
  }
  await b.close();
  process.exit(bad ? 1 : 0);
})();
