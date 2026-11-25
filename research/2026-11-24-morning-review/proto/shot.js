// Pictures of the HTML view: node shot.js <morning.html> <folder>
const { chromium } = require('playwright');
(async () => {
  const [page, dir] = process.argv.slice(2);
  const b = await chromium.launch();
  for (const scheme of ['light', 'dark']) {
    const p = await b.newPage({ viewport: { width: 1240, height: 900 }, deviceScaleFactor: 2, colorScheme: scheme });
    await p.goto('file://' + page);
    const secs = await p.$$('section');
    for (const s of secs) {
      const t = await s.$eval('h2', e => e.textContent);
      const name = t === 'What needs you' ? 'first-screen' : t.startsWith('Chain') ? 'chain' : t === 'Receipt of claude #1' ? 'receipt-claude-1' : null;
      if (name && (scheme === 'light' || name === 'first-screen')) await s.screenshot({ path: `${dir}/${name}${scheme === 'dark' ? '-dark' : ''}.png` });
    }
    await p.close();
  }
  await b.close();
})();
