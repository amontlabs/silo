// The user opens the forwarded port in a browser on the Mac.
const { chromium } = require('playwright');
(async () => {
  const b = await chromium.launch(); const p = await b.newPage();
  await p.goto('http://127.0.0.1:3000/'); await p.waitForTimeout(3000); await b.close();
})();
