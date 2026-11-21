// Builds of the site made for a test, each into a directory of its own, and the build id each wrote into the
// Workshop's page and script (vite.config.ts, src/workshop/chunks.ts).
import { execFile } from 'node:child_process';
import { readFile } from 'node:fs/promises';
import { join } from 'node:path';
import { promisify } from 'node:util';
import { fileURLToPath } from 'node:url';

const run = promisify(execFile);
const website = fileURLToPath(new URL('..', import.meta.url));

/** `npm run build` into `dist`, from the plan in `planDir` (WORKSHOP_PLAN unless it is plan/), with `args` for Vite. */
export async function buildSite(dist, planDir, args = []) {
  const env = { ...process.env };
  if (planDir === join(website, '..', 'plan')) delete env.WORKSHOP_PLAN;
  else env.WORKSHOP_PLAN = planDir;
  const vite = join(website, 'node_modules', 'vite', 'bin', 'vite.js');
  await run(process.execPath, [vite, 'build', '--logLevel', 'error', '--outDir', dist, ...args], { cwd: website, env });
  await run(process.execPath, [join(website, 'scripts', 'prerender.mjs'), dist], { cwd: website, env });
}

/** The Workshop's page of a build, the id its `meta` holds, and the text of the script the page runs. */
export async function workshopOf(dist) {
  const html = await readFile(join(dist, 'workshop', 'index.html'), 'utf8');
  const metas = [...html.matchAll(/<meta name="workshop-build" content="([^"]*)"/g)].map((match) => match[1]);
  const src = html.match(/<script type="module"[^>]*src="([^"]+)"/)?.[1] ?? null;
  const script = src ? await readFile(join(dist, src), 'utf8') : null;
  return { html, metas, id: metas[0] ?? null, src, script };
}
