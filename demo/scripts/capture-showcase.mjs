// Captures the showcase compositions with raw CDP and the system Chrome (PNG,
// with a transparent background where needed), then encodes WebP with Pillow
// (python3). No extra npm dependencies. Uses the running showcase server on :3410 or starts one.
//   docs/silo-showcase.webp          full composition, 3200x2200 (DPR 2)
//   docs/silo-showcase-screens.webp  ?variant=screens, transparent (WebP with alpha)
import { spawn, spawnSync } from 'node:child_process'
import { mkdtempSync, rmSync, writeFileSync, mkdirSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'

const demo = resolve(import.meta.dirname, '..')
const docs = resolve(demo, '../docs')
const origin = 'http://localhost:3410'
const chrome = process.env.CHROME_PATH ?? '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome'
const scale = 2

const shots = [
  { file: 'silo-showcase.webp', query: '?capture=1&theme=dark', width: 1600, height: 1100, transparent: true },
  { file: 'silo-showcase-screens.webp', query: '?variant=screens', transparent: true },
]

const up = () => fetch(`${origin}/showcase.html`).then((r) => r.ok, () => false)
let server
if (!(await up())) {
  server = spawn('npm', ['run', 'showcase'], { cwd: demo, stdio: 'ignore' })
  for (let i = 0; i < 60 && !(await up()); i++) await new Promise((r) => setTimeout(r, 500))
  if (!(await up())) throw new Error('showcase server did not start')
}

const profile = mkdtempSync(join(tmpdir(), 'silo-showcase-'))
const browser = spawn(chrome, ['--headless=new', '--remote-debugging-port=0', `--user-data-dir=${profile}`, '--hide-scrollbars', '--no-first-run', '--disable-gpu', 'about:blank'], { stdio: ['ignore', 'ignore', 'pipe'] })
const wsUrl = await new Promise((ok, fail) => {
  let log = ''
  browser.stderr.on('data', (d) => { log += d; const m = log.match(/ws:\/\/\S+/); if (m) ok(m[0]) })
  browser.on('exit', () => fail(new Error(`Chrome exited early:\n${log}`)))
})

const ws = new WebSocket(wsUrl)
await new Promise((ok, fail) => { ws.onopen = ok; ws.onerror = fail })
let id = 0
const pending = new Map()
ws.onmessage = ({ data }) => { const m = JSON.parse(data); pending.get(m.id)?.(m); pending.delete(m.id) }
const send = (method, params = {}, sessionId) => new Promise((ok, fail) => {
  const n = ++id
  pending.set(n, (m) => (m.error ? fail(new Error(`${method}: ${m.error.message}`)) : ok(m.result)))
  ws.send(JSON.stringify({ id: n, method, params, sessionId }))
})

try {
  const { targetId } = await send('Target.createTarget', { url: 'about:blank' })
  const { sessionId } = await send('Target.attachToTarget', { targetId, flatten: true })
  const cdp = (m, p) => send(m, p, sessionId)
  const evaluate = async (expression) => (await cdp('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true })).result.value
  mkdirSync(docs, { recursive: true })
  for (const shot of shots) {
    await cdp('Page.enable')
    await cdp('Emulation.setDeviceMetricsOverride', { width: shot.width ?? 2000, height: shot.height ?? 1600, deviceScaleFactor: scale, mobile: false })
    await cdp('Page.navigate', { url: `${origin}/showcase.html${shot.query}` })
    await cdp('Emulation.setDefaultBackgroundColorOverride', shot.transparent ? { color: { r: 0, g: 0, b: 0, a: 0 } } : {})
    // Wait for the production UI, then for fonts and a settled layout.
    await evaluate(`new Promise((ok, fail) => { const t = Date.now(); const f = () => document.querySelectorAll('.showcase-shot .silo-window').length === 3 && document.querySelector('button[aria-label="SSH access controls for lab"][aria-expanded="true"]') ? ok() : Date.now() - t > 20000 ? fail(new Error('showcase did not render')) : setTimeout(f, 100); f() })`)
    await evaluate('document.fonts.ready.then(() => new Promise((r) => setTimeout(r, 800)))')
    const rect = await evaluate(`(() => { const r = document.querySelector('.showcase-frame').getBoundingClientRect(); return { x: r.x, y: r.y, width: r.width, height: r.height } })()`)
    const { data } = await cdp('Page.captureScreenshot', { format: 'png', clip: { ...rect, scale: 1 }, captureBeyondViewport: false, fromSurface: true })
    const png = join(profile, `${shot.file}.png`)
    writeFileSync(png, Buffer.from(data, 'base64'))
    // Lossy WebP with a lossless alpha channel keeps soft shadows clean.
    const enc = spawnSync(process.env.PYTHON ?? 'python3', ['-c', 'import sys; from PIL import Image; Image.open(sys.argv[1]).save(sys.argv[2], "WEBP", quality=90, alpha_quality=100, method=6)', png, join(docs, shot.file)], { stdio: 'inherit' })
    if (enc.status !== 0) throw new Error('WebP encoding failed: python3 with Pillow is required (pip install pillow; set PYTHON to choose the interpreter)')
    console.log(`${shot.file}: ${rect.width * scale}x${rect.height * scale}`)
  }
} finally {
  ws.close()
  browser.kill()
  server?.kill()
  setTimeout(() => rmSync(profile, { recursive: true, force: true }), 500).unref()
}
