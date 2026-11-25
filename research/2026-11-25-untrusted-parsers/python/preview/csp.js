// Opens the preview of a port in Chromium, and says whether the page's script reached a host its
// Content-Security-Policy (connect-src 'self') does not allow.
const { chromium } = require('playwright');
(async () => {
  const b = await chromium.launch();
  const p = await b.newPage();
  const r = await p.goto('http://3000.c1a7.localhost:7000/');
  await p.waitForTimeout(1500);
  const h = await r.allHeaders();
  console.log(JSON.stringify({ csp_header: !!h['content-security-policy'], title: await p.title(),
    body_has_csp_text: (await p.content()).includes('content-security-policy') }));
  await b.close();
})();
