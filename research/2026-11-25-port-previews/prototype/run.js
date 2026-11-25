// The preview tests. Each scenario starts the apps and Silo's preview component, drives Chromium the
// way a person would, and prints what reached where. Output: one JSON object per scenario.
// usage: node run.js <out dir> [scenario ...]     (run from a folder where require('playwright') works)
const { chromium } = require('playwright');
const { spawn } = require('child_process');
const fs = require('fs'), path = require('path');
const HERE = __dirname, OUT = path.resolve(process.argv[2] || path.join(HERE, 'out'));
fs.mkdirSync(OUT, { recursive: true });
// NAMES: how previews are named (see silo_preview.py). The scenarios set process.env.NAMES.
const SHELL = (cid, q) => (process.env.NAMES === 'one-site' ? `http://preview.silo.localhost:7000/${cid}/3000` : `http://silo.${cid}.localhost:7000/3000`) + (q ? '?' + q : '');
const APP = cid => process.env.NAMES === 'one-site' ? `http://3000.${cid}.silo.localhost:7000/` : `http://3000.${cid}.localhost:7000/`;
const sleep = ms => new Promise(r => setTimeout(r, ms));

function start(cmd, args, env) {
  const p = spawn(cmd, args, { env: { ...process.env, ...env }, stdio: ['ignore', 'pipe', 'inherit'] });
  return new Promise(res => p.stdout.on('data', d => { if (String(d).includes('ready')) res(p); }));
}
async function up(name, env = {}) {
  const hits = path.join(OUT, name + '.hits.jsonl'); fs.rmSync(hits, { force: true });
  const apps = await start('python3', [path.join(HERE, 'apps.py'), hits], env);
  const silo = await start('python3', [path.join(HERE, 'silo_preview.py')], { CONFIG: path.join(HERE, 'config.json'), LOG: hits, ...env });
  return { hits, down: async () => { apps.kill(); silo.kill(); await sleep(300); } };
}
const read = f => fs.existsSync(f) ? fs.readFileSync(f, 'utf8').trim().split('\n').filter(Boolean).map(JSON.parse) : [];

// The browser Silo starts for one computer, as #363 set it up: every request through that computer's
// rule proxy, and WebRTC may use no UDP that bypasses the proxy.
const ARGS_363 = proxyPort => [`--proxy-server=http://127.0.0.1:${proxyPort}`, '--proxy-bypass-list=<-loopback>',
  '--force-webrtc-ip-handling-policy=disable_non_proxied_udp'];
// The same, with no name resolved by the browser itself: every name goes to the proxy unresolved.
const SILO_ARGS = proxyPort => [...ARGS_363(proxyPort), '--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE 127.0.0.1'];

async function browser({ args = [], netlog, profile, bare } = {}) {
  const a = [...args]; if (netlog) a.push(`--log-net-log=${netlog}`, '--net-log-capture-mode=IncludeSensitive');
  const env = { ...process.env }; for (const k of Object.keys(env)) if (/_proxy$/i.test(k)) delete env[k];
  const opts = { args: a, acceptDownloads: true, env };
  if (bare) { opts.ignoreDefaultArgs = true; opts.args = ['--headless=new', '--remote-debugging-pipe', '--no-sandbox', ...a]; }
  if (profile) { const ctx = await chromium.launchPersistentContext(profile, opts); return { ctx, close: () => ctx.close() }; }
  const b = await chromium.launch(opts); const ctx = await b.newContext({ acceptDownloads: true });
  return { ctx, close: () => b.close() };
}

async function visit(ctx, url, wait = 2500, act) {
  const p = await ctx.newPage(); const seen = { dialogs: [], console: [] };
  p.on('dialog', d => { seen.dialogs.push(d.type() + ': ' + d.message()); d.dismiss().catch(() => {}); });
  p.on('console', m => seen.console.push(m.text().slice(0, 160)));
  await p.goto(url).catch(e => seen.error = e.message.split('\n')[0]);
  await p.waitForTimeout(800);
  if (act) await act(p).catch(e => seen.actError = e.message.split('\n')[0]);
  await p.waitForTimeout(wait);
  seen.url = p.url(); await p.close(); return seen;
}
const appFrame = p => p.frames().find(f => f.url().includes('3000.c1a7')) || p.mainFrame();

const S = {
  // 1. Another computer's preview, from c1's page. With the app's policy as #363 wrote it, and with
  // ws: added, as a policy written for dev servers' hot reload tends to be.
  async cross() {
    const r = {};
    for (const names of ['one-site', 'per-computer']) for (const ws of ['', '1']) for (const [mode, env, args] of [
      ['your browser', {}, []],
      ['one browser Silo starts for all computers', { PROXY: 'shared' }, SILO_ARGS(7100)],
      ['a browser Silo starts per computer', {}, SILO_ARGS(7101)],
    ]) {
      process.env.NAMES = names;
      const u = await up('cross', { ...env, CSP_WS: ws }); const b = await browser({ args });
      await visit(b.ctx, SHELL('c1a7', 't=cross'));
      await b.close(); await u.down();
      const h = read(u.hits);
      r[`${names} | ${ws ? "policy with ws:" : "policy 'self'"} | ${mode}`] = {
        c2_got: [...new Set(h.filter(x => x.to === 'c2' && x.secret).map(x => new URLSearchParams(x.path.split('?')[1]).get('via')))],
        proxy_denied: [...new Set(h.filter(x => x.denied).map(x => x.denied))] };
    }
    return r;
  },
  // 2. Cookies set by c1 that c2's previews then receive, in one cookie jar (your browser, or one
  // browser for all computers).
  async toss() {
    const r = {};
    for (const names of ['one-site', 'per-computer']) for (const [mode, env] of [['forwarder keeps Domain=', {}], ['forwarder strips Domain= from Set-Cookie', { SET_COOKIE: 'host-only' }]]) {
      process.env.NAMES = names;
      const u = await up('toss', env); const b = await browser();
      // The user's own app on the Mac set a cookie for localhost.
      await b.ctx.addCookies([{ name: 'mac_app', value: 'MAC-COOKIE', domain: 'localhost', path: '/' }]);
      await visit(b.ctx, SHELL('c1a7', 't=toss'));
      await visit(b.ctx, SHELL('c2b9'), 1000);
      await b.close(); await u.down();
      const h = read(u.hits);
      r[`${names} | ${mode}`] = { c1_page_holds: (h.find(x => x.what === 'toss') || {}).detail,
        c2_received: [...new Set(h.filter(x => x.to === 'c2').map(x => x.cookie).filter(Boolean))] };
    }
    return r;
  },
  // The app's own sign-in cookie (default SameSite), set and sent back inside Silo's page.
  async login() {
    const r = {};
    for (const names of ['one-site', 'per-computer']) {
      process.env.NAMES = names;
      const u = await up('login'); const b = await browser({ args: SILO_ARGS(7101) });
      await visit(b.ctx, SHELL('c1a7', 't=login'));
      await b.close(); await u.down();
      const h = read(u.hits);
      r[names] = { cookie_sent_back: (h.find(x => x.what === 'login-cookie-sent-back') || {}).cookie || 'none' };
    }
    return r;
  },
  // 3. A worker left by a computer that is gone, when a new computer takes its name.
  async persist() {
    const prof = fs.mkdtempSync('/tmp/silo-preview-profile-');
    let u = await up('persist'); let b = await browser({ profile: prof });
    await visit(b.ctx, SHELL('c1a7', 't=sw'), 3000); await b.close(); await u.down();
    const first = read(u.hits);
    u = await up('persist', { C1_PLAIN: '1' }); b = await browser({ profile: prof });
    const s = await visit(b.ctx, SHELL('c1a7'), 2500);
    const direct = await visit(b.ctx, APP('c1a7'), 2000);
    await b.close(); await u.down(); fs.rmSync(prof, { recursive: true, force: true });
    const h = read(u.hits);
    return { first_computer: first.filter(x => x.to === 'c1').map(x => x.what + ' ' + x.detail),
             new_computer_page_served: h.some(x => x.what === 'new-computer-page'),
             old_worker_answered: h.some(x => x.what === 'reborn'), direct_title_seen: direct.url };
  },
  // 4. Name lookups that carry the secret, read from Chromium's own network log.
  async dns() {
    const r = {};
    for (const [mode, args, sandbox] of [
      ['your browser, Silo page', [], null],
      ['browser Silo starts as #363 set it up, Silo page', ARGS_363(7101), null],
      ['browser Silo starts with no resolver of its own, Silo page', SILO_ARGS(7101), null],
    ].filter(([m]) => !process.env.ONLY || m.includes(process.env.ONLY))) {
      const u = await up('dns'); const nl = path.join(OUT, 'dns-' + (args.length ? 'silo' : 'own') + '.netlog.json');
      const b = await browser({ args, netlog: nl });
      await visit(b.ctx, SHELL('c1a7', 't=dns'), 6000); await b.close(); await u.down();
      const log = fs.readFileSync(nl, 'utf8');
      const names = [...new Set((log.match(/[a-z-]+-akiac1secret777\.exfil\.test/g) || []))];
      // Which of those names the browser asked its resolver for (a lookup leaves the Mac).
      let ev; try { ev = JSON.parse(log); } catch { ev = JSON.parse(log.trim().replace(/,$/, '') + ']}'); }
      const types = Object.fromEntries(Object.entries(ev.constants.logEventTypes).map(([k, v]) => [v, k]));
      const resolved = new Set();
      for (const e of ev.events) {
        const t = types[e.type] || '';
        const host = e.params && (e.params.host || (e.params.url || '') || '');
        if (/HOST_RESOLVER/.test(t) && /exfil\.test/.test(JSON.stringify(e.params || {}))) {
          const m = JSON.stringify(e.params).match(/[a-z-]+-akiac1secret777\.exfil\.test/); if (m) resolved.add(m[0].split('-')[0]);
        }
      }
      const h = read(u.hits);
      r[mode] = { names_in_netlog: names.map(n => n.split('-')[0]), looked_up: [...resolved],
                  proxy_denied: [...new Set(h.filter(x => x.denied).map(x => x.denied.split('-')[0]))],
                  webrtc_to_attacker: h.filter(x => x.to === 'attacker').map(x => x.via + (x.secret ? ' +secret' : '')),
                  candidates: h.filter(x => x.what === 'dns-webrtc-candidate').length };
    }
    return r;
  },
  // 5. What the browser draws over Silo's page, and the Mac's other programs and clipboard.
  async ui() {
    const r = {};
    for (const [mode, sb] of [['sandbox with allow-modals (#363)', 'allow-scripts allow-same-origin allow-forms allow-modals'],
                              ['sandbox without allow-modals', 'allow-scripts allow-same-origin allow-forms']]) {
      const u = await up('ui', { SANDBOX: sb }); const b = await browser({ args: SILO_ARGS(7101) });
      await b.ctx.grantPermissions(['clipboard-read', 'clipboard-write'], { origin: new URL(SHELL('c1a7')).origin });
      const out = {};
      for (const t of ['modal', 'notify', 'proto']) out[t] = await visit(b.ctx, SHELL('c1a7', 't=' + t), 1500);
      out.clip = await visit(b.ctx, SHELL('c1a7', 't=clip'), 800, async p => {
        await appFrame(p).click('#clip', { timeout: 2000 });
        await p.waitForTimeout(500);
        out.clipboard_after = await p.evaluate(() => navigator.clipboard.readText()).catch(e => 'unreadable: ' + e.message.split('\n')[0]);
      });
      await b.close(); await u.down();
      const h = read(u.hits);
      r[mode] = { reported: h.filter(x => x.to === 'c1').map(x => x.what + ': ' + x.detail),
                  dialogs: Object.fromEntries(Object.entries(out).filter(([k, v]) => v && v.dialogs && v.dialogs.length).map(([k, v]) => [k, v.dialogs])),
                  proto_console: (out.proto.console || []).filter(c => /vscode|protocol|sandbox/i.test(c)), proto_url: out.proto.url,
                  clipboard_after: out.clipboard_after };
    }
    return r;
  },
  // 6. What the browser sends by itself, with no page asking, in 40 seconds.
  async background() {
    const r = {};
    for (const [mode, bare] of [['Chromium with only Silo\'s flags', true], ['Chromium with Playwright\'s default flags', false]]) {
      const u = await up('bg'); const b = await browser({ args: SILO_ARGS(7101), bare });
      await visit(b.ctx, SHELL('c1a7'), 40000); await b.close(); await u.down();
      const h = read(u.hits);
      r[mode] = [...new Set(h.filter(x => x.denied).map(x => x.denied))];
    }
    return r;
  },
};

(async () => {
  const want = process.argv.slice(3).length ? process.argv.slice(3) : Object.keys(S);
  for (const k of want) {
    const res = await S[k]();
    const line = JSON.stringify({ scenario: k, ...res }, null, 1);
    console.log(line); fs.appendFileSync(path.join(OUT, 'results.json'), line + '\n');
  }
})();
