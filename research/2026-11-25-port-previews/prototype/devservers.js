// Real dev servers through Silo's preview: Vite and Next.js, each in the Silo page, in the browser
// Silo starts. For each: does the page load, what the console says, and does an edit of the source
// reach the open page by itself (hot reload over the dev server's WebSocket).
// usage: node devservers.js <vite app dir> <next app dir> <out dir>
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs'), path = require('path');
const [VITE, NEXT, OUT] = process.argv.slice(2).map(p => path.resolve(p));
const HERE = __dirname; fs.mkdirSync(OUT, { recursive: true });
const sleep = ms => new Promise(r => setTimeout(r, ms));
const ARGS = ['--proxy-server=http://127.0.0.1:7101', '--proxy-bypass-list=<-loopback>',
  '--force-webrtc-ip-handling-policy=disable_non_proxied_udp', '--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE 127.0.0.1'];
const env = { ...process.env }; for (const k of Object.keys(env)) if (/_proxy$/i.test(k)) delete env[k];

function waitFor(p, re) { return new Promise(res => { const f = d => { if (re.test(String(d))) res(); }; p.stdout.on('data', f); if (p.stderr) p.stderr.on('data', f); }); }
const SERVERS = {
  vite: { port: 5173, file: () => path.join(VITE, 'src/main.js'), start: () => spawn('npx', ['vite', '--host', '127.0.0.1', '--port', '5173', '--strictPort'], { cwd: VITE, env }), ready: /Local:/,
    write: v => `document.querySelector('#app').innerHTML = \`<h1 id="msg">${v}</h1>\`\nif (import.meta.hot) import.meta.hot.accept()\n` },
  next: { port: 3000, file: () => path.join(NEXT, 'app/page.js'), start: () => spawn('npx', ['next', 'dev', '-p', '3300', '-H', '127.0.0.1'], { cwd: NEXT, env }), ready: /Ready|ready/,
    write: v => `export default function Page() { return <h1 id="msg">${v}</h1>; }\n` },
};

(async () => {
  const results = {};
  for (const host of ['keep', 'rewrite']) for (const [name, s] of Object.entries(SERVERS)) {
    fs.writeFileSync(s.file(), s.write('version one'));
    const dev = s.start(); await waitFor(dev, s.ready); await sleep(1500);
    const log = path.join(OUT, `dev-${name}-${host}.jsonl`); fs.rmSync(log, { force: true });
    const silo = spawn('python3', [path.join(HERE, 'silo_preview.py')], { env: { ...env, CONFIG: path.join(HERE, 'config-dev.json'), LOG: log, HOST: host }, stdio: ['ignore', 'pipe', 'inherit'] });
    await waitFor(silo, /ready/);
    const b = await chromium.launch({ args: ARGS, env }); const ctx = await b.newContext(); const p = await ctx.newPage();
    const r = { console: [] }; let navs = 0;
    p.on('console', m => { if (m.type() === 'error' || m.type() === 'warning') r.console.push(m.text().slice(0, 140)); });
    p.on('framenavigated', f => { if (f !== p.mainFrame()) navs++; });
    await p.goto(`http://silo.c1a7.localhost:7000/${s.port}`).catch(e => r.error = e.message.split('\n')[0]);
    const frame = () => p.frames().find(f => f.url().includes(`${s.port}.c1a7.localhost`));
    const msg = async () => { const f = frame(); return f ? await f.locator('#msg').textContent({ timeout: 1000 }).catch(() => null) : null; };
    for (let i = 0; i < 40 && !(await msg()); i++) await sleep(500);
    r.loaded = await msg();
    // A mark in the app's window: it survives a hot update and is lost on a reload.
    await frame()?.evaluate(() => { window.__mark = 1; }).catch(() => {});
    fs.writeFileSync(s.file(), s.write('version two'));
    let t0 = Date.now(); for (let i = 0; i < 40 && (await msg()) !== 'version two'; i++) await sleep(500);
    r.after_edit = await msg(); r.edit_seconds = (Date.now() - t0) / 1000; r.reloaded_whole_frame = !(await frame()?.evaluate(() => window.__mark === 1).catch(() => false));
    r.proxy_denied = fs.existsSync(log) ? [...new Set(fs.readFileSync(log, 'utf8').trim().split('\n').filter(Boolean).map(JSON.parse).filter(x => x.denied).map(x => x.denied))] : [];
    await b.close(); silo.kill(); dev.kill('SIGTERM'); spawn('pkill', ['-f', name === 'vite' ? 'vite --host' : 'next dev']); await sleep(1500);
    results[`${name}, app sees Host ${host === 'keep' ? 'as the preview name' : 'as localhost'}`] = r;
    fs.writeFileSync(s.file(), s.write('version one'));
  }
  const out = JSON.stringify({ scenario: 'devservers', ...results }, null, 1);
  console.log(out); fs.appendFileSync(path.join(OUT, 'results.json'), out + '\n');
})();
