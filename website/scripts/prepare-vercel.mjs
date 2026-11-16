import { cp, mkdir, readdir, rm, stat, symlink, writeFile } from 'node:fs/promises';
import { join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

// Packages the built site and the Workshop API with Vercel's Build Output API v3:
// static files in static/, one Node.js function served at every
// /api/workshop/<endpoint> path (the other paths are symlinks to it), and the
// routes that serve the Workshop page at /workshop and /workshop/.
export const ENDPOINTS = ['state', 'me', 'issue', 'login', 'callback', 'logout', 'comment', 'vote', 'suggest', 'verdict', 'decide'];
export const RUNTIME = 'nodejs24.x';

const website = fileURLToPath(new URL('..', import.meta.url));
// Vercel's Git build reads .vercel/output in the project's root directory, the repository root.
export const DEFAULT_OUTPUT = join(website, '..', '.vercel', 'output');

async function exists(path) {
  return stat(path).then(() => true, () => false);
}

export async function prepareVercel({ siteDir = website, distDir = join(siteDir, 'dist'), outputDir = DEFAULT_OUTPUT } = {}) {
  const page = (await exists(join(distDir, 'workshop', 'index.html'))) ? '/workshop/index.html'
    : (await exists(join(distDir, 'workshop.html'))) ? '/workshop.html' : null;
  if (!page) throw new Error('The build has no Workshop page (workshop/index.html or workshop.html).');

  await rm(outputDir, { recursive: true, force: true });
  await mkdir(outputDir, { recursive: true });
  await cp(distDir, join(outputDir, 'static'), { recursive: true });

  const functions = join(outputDir, 'functions', 'api', 'workshop');
  const [first, ...others] = ENDPOINTS;
  const main = join(functions, `${first}.func`);
  await mkdir(join(main, 'api'), { recursive: true });
  await cp(join(siteDir, 'api', 'workshop.cjs'), join(main, 'api', 'workshop.cjs'));
  await mkdir(join(main, 'server'), { recursive: true });
  for (const name of await readdir(join(siteDir, 'server'))) {
    if (name.endsWith('.mjs') && !name.endsWith('.test.mjs')) await cp(join(siteDir, 'server', name), join(main, 'server', name));
  }
  await writeFile(join(main, '.vc-config.json'), JSON.stringify({
    runtime: RUNTIME,
    handler: 'api/workshop.cjs',
    launcherType: 'Nodejs',
    shouldAddHelpers: false,
    maxDuration: 15,
  }, null, 2) + '\n');
  for (const name of others) await symlink(`${first}.func`, join(functions, `${name}.func`), 'dir');

  const config = {
    version: 3,
    routes: [
      { src: '^/workshop/?$', dest: page },
      { handle: 'filesystem' },
    ],
  };
  await writeFile(join(outputDir, 'config.json'), JSON.stringify(config, null, 2) + '\n');
  return { outputDir, page, config };
}

// `--out <dir>` writes the output elsewhere.
if (import.meta.url === pathToFileURL(process.argv[1] || '').href) {
  const index = process.argv.indexOf('--out');
  await prepareVercel(index > 0 ? { outputDir: resolve(process.argv[index + 1]) } : {});
}
