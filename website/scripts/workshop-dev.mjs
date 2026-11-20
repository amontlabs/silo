#!/usr/bin/env node
// Serves the built site (dist/) and the Workshop API on one port.
//   node scripts/workshop-dev.mjs --fake [--port 4174]   against a built-in fake GitHub
//   node scripts/workshop-dev.mjs --fake --progress      the same, in a later state: verdicts, entries being built, history
//   node scripts/workshop-dev.mjs [--port 4174]          against GitHub, from WORKSHOP_* variables
// Run `npm run build` first. The site's origin is http://127.0.0.1:<port>.
import { createReadStream } from 'node:fs';
import { readFile, stat } from 'node:fs/promises';
import { createServer } from 'node:http';
import { extname, join, normalize, sep } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { readConfig } from '../server/config.mjs';
import { createNodeHandler, FRAME_HEADERS } from '../server/node.mjs';
import { createWorkshop } from '../server/workshop.mjs';
import { readPlan } from '../../tools/plan/read/index.js';
import { createFakeGitHub, seedDemo } from './fake-github.mjs';

const website = fileURLToPath(new URL('..', import.meta.url));

const TYPES = {
  '.html': 'text/html; charset=utf-8', '.js': 'text/javascript; charset=utf-8', '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8', '.json': 'application/json; charset=utf-8', '.svg': 'image/svg+xml',
  '.png': 'image/png', '.jpg': 'image/jpeg', '.webp': 'image/webp', '.woff2': 'font/woff2', '.woff': 'font/woff',
  '.mp4': 'video/mp4', '.webm': 'video/webm', '.ico': 'image/x-icon', '.txt': 'text/plain; charset=utf-8',
};

async function file(path) {
  return stat(path).then((s) => (s.isFile() ? path : null), () => null);
}

// The static file for a request path, resolved the way the deployed routes do.
export async function staticFile(distDir, pathname) {
  let decoded;
  try {
    decoded = decodeURIComponent(pathname);
  } catch {
    return null;
  }
  if (/^\/workshop\/?$/.test(decoded)) return (await file(join(distDir, 'workshop', 'index.html'))) || file(join(distDir, 'workshop.html'));
  const target = normalize(join(distDir, decoded));
  if (target !== distDir && !target.startsWith(distDir.endsWith(sep) ? distDir : distDir + sep)) return null;
  return (await file(target)) || file(join(target, 'index.html'));
}

// plan/, or the plan WORKSHOP_PLAN names, as the build reads it.
const PLAN = process.env.WORKSHOP_PLAN || join(website, '..', 'plan');

/** The features of the plan in `dir`, in the order of order.json. */
export function loadFeatures(dir) {
  const plan = readPlan(dir);
  return plan.stages.flatMap((stage) => stage.entries).map((id) => plan.entries[id]).filter((entry) => entry?.kind === 'feature');
}

// The Workshop page names where it reads a feature's file at a commit; with the fake GitHub, the fake.
const RAW_META = '<meta name="workshop-raw" content="';

// `planDir` is the plan the fake GitHub is seeded from.
export async function startDev({ fake = false, progress = false, planDir = PLAN, port = 4174, host = '127.0.0.1', distDir = join(website, 'dist'), env = process.env, log } = {}) {
  const origin = `http://${host}:${port}`;
  let fakeGitHub = null;
  let plan = null;
  let config;
  if (fake) {
    fakeGitHub = createFakeGitHub({ repo: 'amontlabs/silo' });
    await fakeGitHub.listen();
    plan = seedDemo(fakeGitHub, loadFeatures(planDir), { progress }).progress;
    config = readConfig(fakeGitHub.workshopEnv(origin), { apiBase: fakeGitHub.url, webBase: fakeGitHub.url });
  } else {
    config = readConfig({ ...env, WORKSHOP_SITE_ORIGIN: origin });
  }
  const api = createNodeHandler(createWorkshop({ config, ...(log ? { log } : {}) }));
  const server = createServer(async (req, res) => {
    let pathname;
    try {
      pathname = new URL(req.url, 'http://local.invalid').pathname;
    } catch {
      pathname = null;
    }
    if (pathname?.startsWith('/api/workshop/')) return api(req, res);
    for (const [name, value] of Object.entries(FRAME_HEADERS)) res.setHeader(name, value);
    if (pathname === null) {
      res.writeHead(400);
      return res.end();
    }
    if (req.method !== 'GET' && req.method !== 'HEAD') {
      res.writeHead(405);
      return res.end();
    }
    const path = await staticFile(distDir, pathname);
    if (!path) {
      res.writeHead(404, { 'Content-Type': 'text/plain; charset=utf-8' });
      return res.end('Not found');
    }
    res.writeHead(200, { 'Content-Type': TYPES[extname(path)] || 'application/octet-stream' });
    if (req.method === 'HEAD') return res.end();
    if (fakeGitHub && extname(path) === '.html' && /^\/workshop\/?$/.test(pathname)) {
      const html = await readFile(path, 'utf8');
      return res.end(html.replace('<head>', `<head>${RAW_META}${fakeGitHub.rawUrl}">`));
    }
    createReadStream(path).on('error', () => res.destroy()).pipe(res);
  });
  await new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(port, host, resolve);
  });
  return {
    origin,
    config,
    fakeGitHub,
    progress: plan,
    async close() {
      server.closeAllConnections?.();
      await new Promise((resolve) => server.close(resolve));
      await fakeGitHub?.close();
    },
  };
}

// What the --progress state shows, in one line.
export function progressLine(plan) {
  const counts = { take: 0, later: 0, drop: 0 };
  for (const verdict of plan.verdicts.values()) counts[verdict] += 1;
  return `Verdicts on the stages ${plan.stages.join(', ') || 'none'}: ${counts.take} taken, ${counts.later} later, ${counts.drop} dropped; being built: ${plan.building.size}; built: ${plan.built.size}.`;
}

if (import.meta.url === pathToFileURL(process.argv[1] || '').href) {
  const args = process.argv.slice(2);
  const portIndex = args.indexOf('--port');
  const port = portIndex >= 0 ? Number(args[portIndex + 1]) : 4174;
  const fake = args.includes('--fake');
  const progress = args.includes('--progress');
  if (progress && !fake) {
    console.error('--progress needs --fake.');
    process.exit(2);
  }
  const distDir = join(website, 'dist');
  if (!(await file(join(distDir, 'index.html')))) {
    console.error('No built site in dist/. Run `npm run build` first.');
    process.exit(1);
  }
  const dev = await startDev({ fake, progress, port, distDir });
  if (!fake && !dev.config.configured) console.log('WORKSHOP_* variables are not set: the API answers 503 not_configured. Use --fake for a fake GitHub.');
  console.log(`Workshop: ${dev.origin}/workshop${fake ? ` (fake GitHub at ${dev.fakeGitHub.url}; users owner-fake can decide, visitor-fake cannot)` : ''}`);
  const stop = () => dev.close().then(() => process.exit(0));
  if (dev.progress) console.log(progressLine(dev.progress));
  process.on('SIGINT', stop);
  process.on('SIGTERM', stop);
}
