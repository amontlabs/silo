// The user opens the forwarded port in Chromium on the Mac, once for the page's background attempts
// and once for each way of leaving the page; then clicks the page's two links, as a person would.
// usage: node browse.js <url>   (the url of the preview in the mode under test)
const { chromium } = require('playwright');
const url = process.argv[2];
const LEAVES = ["window.open", "top-navigation", "form-top", "self-navigation", "meta-refresh", "download", "internal-navigation"];
(async () => {
  const b = await chromium.launch({ args: JSON.parse(process.env.ARGS || '[]') });
  const ctx = await b.newContext({ acceptDownloads: true });
  // The user's other app on localhost set a cookie; cookies ignore the port (RFC 6265 8.5).
  await ctx.addCookies([{ name: "mac_session", value: "MAC-APP-COOKIE", domain: "localhost", path: "/" }]);
  const out = { downloads: 0, popups: 0, final: {} }; let mine = 0;
  ctx.on('page', () => out.popups++);
  const open = async (u) => { mine++; const p = await ctx.newPage(); p.on('download', () => out.downloads++); await p.goto(u).catch(() => {}); await p.waitForTimeout(2500); return p; };
  const join = (q) => url + (url.includes('?') ? '&' : '?') + q;
  let p = await open(url); out.title = await p.title().catch(() => '?'); await p.close();
  for (const l of LEAVES) { p = await open(join('leave=' + encodeURIComponent(l))); out.final[l] = p.url(); await p.close(); }
  for (const id of ['link', 'blank']) {
    p = await open(join('leave=none'));
    const f = p.frames().find(f => f.url().includes('3000.c1') || f.url().includes(':7000/?') || f.url().endsWith(':7000/')) || p.mainFrame();
    await f.click('#' + id, { timeout: 2000 }).catch(e => {}); await p.waitForTimeout(1500);
    out.final['click ' + id] = p.url(); await p.close();
  }
  out.popups -= mine; console.log(JSON.stringify(out));
  await b.close();
})();
