#!/usr/bin/env node
// Serves the built site (dist/) and the Workshop API on one port.
//   node scripts/workshop-dev.mjs --fake [--port 4174]   against a built-in fake GitHub
//   node scripts/workshop-dev.mjs --fake --progress      the same, in a later state with entries built and being built
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

const FEATURES = join(website, '..', 'docs', 'product', 'features.jsonl');

async function loadFeatures(path) {
  try {
    const text = await readFile(path, 'utf8');
    return text.split('\n').filter((line) => line.trim()).map((line) => JSON.parse(line));
  } catch {
    return [];
  }
}

// `featuresFile` is the plan the fake GitHub is seeded from.
export async function startDev({ fake = false, progress = false, featuresFile = FEATURES, port = 4174, host = '127.0.0.1', distDir = join(website, 'dist'), env = process.env, log } = {}) {
  const origin = `http://${host}:${port}`;
  let fakeGitHub = null;
  let plan = null;
  let config;
  if (fake) {
    fakeGitHub = createFakeGitHub({ repo: 'amontlabs/silo' });
    await fakeGitHub.listen();
    plan = seedDemo(fakeGitHub, await loadFeatures(featuresFile), { progress }).progress;
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
  const groups = plan.by === 'area' ? 'areas' : 'stages';
  const fallback = plan.by === 'area' ? '' : 'No entry has an area, so progress goes by stage: ';
  return `${fallback}built: the taken entries of the ${groups} ${plan.builtGroups.join(', ') || 'none'} (${plan.built.size}); being built: ${plan.building.size} of ${plan.buildingGroup ?? 'none'}.`;
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
