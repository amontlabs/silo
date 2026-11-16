import { cp, mkdir, readdir, rm, stat, symlink, writeFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { dirname, join, resolve, sep } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

// Packages the built site and the Workshop API with Vercel's Build Output API v3:
// static files in static/, one Node.js function served at every
// /api/workshop/<endpoint> path (the other paths are symlinks to it), and the
// routes that serve the Workshop page at /workshop and /workshop/.
export const ENDPOINTS = ['state', 'me', 'issue', 'login', 'callback', 'logout', 'comment', 'vote', 'suggest', 'verdict', 'decide'];
export const RUNTIME = 'nodejs24.x';
// Packages the function imports at run time; only their files that load ship inside it.
export const FUNCTION_DEPENDENCIES = ['@vercel/functions'];

const website = fileURLToPath(new URL('..', import.meta.url));
// Vercel's Git build reads .vercel/output in the project's root directory, the repository root.
export const DEFAULT_OUTPUT = join(website, '..', '.vercel', 'output');

async function exists(path) {
  return stat(path).then(() => true, () => false);
}

// Copies into `target` only the files of `names` that load: the module files Node loads when the
// packages are required in a separate process, and each of their packages' package.json (needed to
// resolve the import). Nothing that is never loaded ships.
async function copyLoadedFiles(siteDir, names, target) {
  const script = 'for (const name of JSON.parse(process.argv[1])) require(name); process.stdout.write(JSON.stringify(Object.keys(require.cache)));';
  const loaded = JSON.parse(execFileSync(process.execPath, ['-e', script, JSON.stringify(names)], { cwd: siteDir, encoding: 'utf8' }));
  const modules = join(siteDir, 'node_modules') + sep;
  const files = new Set();
  for (const file of loaded) {
    if (!file.startsWith(modules)) throw new Error(`The function would load ${file}, outside node_modules.`);
    files.add(file);
    const parts = file.slice(modules.length).split(sep);
    const root = join(modules, ...parts.slice(0, parts[0].startsWith('@') ? 2 : 1));
    files.add(join(root, 'package.json'));
  }
  for (const file of files) {
    const destination = join(target, file.slice(siteDir.length + 1));
    await mkdir(dirname(destination), { recursive: true });
    await cp(file, destination);
  }
  return [...files].map((file) => file.slice(siteDir.length + 1)).sort();
}

// Response headers for every path, static pages included: no page of the site may be framed.
export const FRAME_HEADERS = { 'Content-Security-Policy': "frame-ancestors 'none'", 'X-Frame-Options': 'DENY' };

export async function prepareVercel({ siteDir = website, distDir = join(siteDir, 'dist'), outputDir = DEFAULT_OUTPUT } = {}) {
  outputDir = resolve(outputDir);
  if (!outputDir.endsWith(`${sep}.vercel${sep}output`)) throw new Error('The output directory must end in .vercel/output; nothing was written.');
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
  const dependencies = await copyLoadedFiles(resolve(siteDir), FUNCTION_DEPENDENCIES, main);
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
      { src: '^/(.*)$', headers: FRAME_HEADERS, continue: true },
      { src: '^/workshop/?$', dest: page },
      { handle: 'filesystem' },
    ],
  };
  await writeFile(join(outputDir, 'config.json'), JSON.stringify(config, null, 2) + '\n');
  return { outputDir, page, config, dependencies };
}

// `--out <dir>` writes the output elsewhere.
if (import.meta.url === pathToFileURL(process.argv[1] || '').href) {
  const index = process.argv.indexOf('--out');
  await prepareVercel(index > 0 ? { outputDir: resolve(process.argv[index + 1]) } : {});
}
