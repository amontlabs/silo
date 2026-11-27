// Opens the preview of port 3000 in Chromium. Prints the answer's status, whether it carried the
// policy, and whether the page's script ran.
const { chromium } = require('playwright');
(async () => {
  const b = await chromium.launch();
  const p = await b.newPage();
  let status = null, csp = false;
  try {
    const r = await p.goto('http://3000.c1a7.localhost:7000/');
    status = r.status(); csp = !!(await r.allHeaders())['content-security-policy'];
  } catch (e) { status = 'error ' + e.message.split('\n')[0]; }
  await p.waitForTimeout(1200);
  console.log(JSON.stringify({ status, csp_header: csp, script_ran: (await p.title()) === 'ran' }));
  await b.close();
})();
