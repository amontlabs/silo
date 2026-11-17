#!/usr/bin/env node
//   node scripts/prerender.mjs [dist]   (default: website/dist)
// After `vite build`: writes the Workshop's markup for the plain address into dist/workshop/index.html, so the
// page is drawn before its script arrives (src/workshop/prerender.tsx). The page's code is loaded through Vite,
// with the same plugins as the build (the plan's data included). Any error fails the build.
import { readFile, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';

const website = fileURLToPath(new URL('..', import.meta.url));
const page = join(resolve(process.argv[2] ?? join(website, 'dist')), 'workshop', 'index.html');

const vite = await createServer({ root: website, logLevel: 'error', appType: 'custom', server: { middlewareMode: true, hmr: false, ws: false, watch: null } });
try {
  const { prerender, inject } = await vite.ssrLoadModule('/src/workshop/prerender.tsx');
  const { default: data } = await vite.ssrLoadModule('virtual:workshop-data');
  const parts = prerender(data);
  await writeFile(page, inject(await readFile(page, 'utf8'), parts));
  console.log(`prerendered the Workshop into ${page} (${Math.round((parts.root.length + parts.account.length) / 1024)} KB of markup)`);
} finally {
  await vite.close();
}
