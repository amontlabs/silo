import { readdirSync, realpathSync } from 'node:fs';
import { cp, mkdir, rm, stat, symlink, writeFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import { dirname, join, relative, resolve, sep } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { ENTRIES_FILE } from '../server/entries.mjs';
import { planDir as defaultPlanDir, planEntries } from './plan-entries.mjs';

// Packages the built site and the Workshop API with Vercel's Build Output API v3:
// static files in static/, one Node.js function served at every
// /api/workshop/<endpoint> path (the other paths are symlinks to it), and the
// routes that serve the Workshop page at /workshop and /workshop/, and each film's page at /films/<id>/.
//
// The function holds the files it loads at the paths they have in the
// repository: website/api/ and website/server/ at any depth,
// tools/workshop/model/ (the Workshop's model, with tools/workshop/package.json,
// which makes it ESM), and the plan tool's write/, check/, model/ and read/ that
// the owner's verdict runs (with tools/plan/package.json), and the site's
// build checks it runs on a verdict's plan: website/src/workshop/build-check.ts
// and the files of that folder it loads (the map), which Node loads as they
// are, its types stripped. Any other file it would load, in website/ or outside
// it, stops the packaging: a package in website/node_modules/ is one of them.
// Beside the server goes plan-entries.json, the plan's entries made from the
// plan at build time (scripts/plan-entries.mjs), which the server reads.
export const ENDPOINTS = ['state', 'me', 'issue', 'login', 'callback', 'logout', 'comment', 'comment-vote', 'vote', 'suggest', 'verdict'];
export const RUNTIME = 'nodejs24.x';
export const HANDLER = 'website/api/workshop.cjs';

const website = fileURLToPath(new URL('..', import.meta.url));
// Vercel's Git build reads .vercel/output in the project's root directory, the repository root.
export const DEFAULT_OUTPUT = join(website, '..', '.vercel', 'output');
/** The folder of the Workshop's model, relative to the repository: the function ships all of it. */
export const MODEL_DIR = join('tools', 'workshop', 'model');
/** The folders of the plan tool the owner's verdict loads (server/verdicts.mjs), relative to the repository. */
export const PLAN_DIRS = ['write', 'check', 'model', 'read'].map((name) => join('tools', 'plan', name));
/** The folder of the site's build checks the owner's verdict loads (src/workshop/build-check.ts), relative to website/. */
export const SITE_CHECK_DIR = join('src', 'workshop');

async function exists(path) {
  return stat(path).then(() => true, () => false);
}

// Every file the function loads, found by loading its entry point in a separate process with a
// resolve hook that records each file (ESM imports and CommonJS requires alike).
export function loadedFiles(siteDir) {
  const script = `
    import { registerHooks } from 'node:module';
    import { pathToFileURL } from 'node:url';
    const seen = new Set();
    registerHooks({ resolve(specifier, context, next) { const result = next(specifier, context); if (result.url.startsWith('file:')) seen.add(result.url); return result; } });
    await import(pathToFileURL(process.argv[1]).href);
    await (await import('node:module')).createRequire(process.argv[1])(process.argv[2]);
    process.stdout.write(JSON.stringify([...seen]));`;
  const urls = JSON.parse(execFileSync(process.execPath, ['--input-type=module', '-e', script, join(siteDir, 'server', 'node.mjs'), join(siteDir, 'api', 'workshop.cjs')], { cwd: siteDir, encoding: 'utf8', env: { ...process.env, WORKSHOP_APP_ID: '' } }));
  return urls.map((url) => fileURLToPath(url));
}

// Where each loaded file goes in the function, by its path relative to the repository; refuses any
// file it would not ship.
function functionFiles(siteDir) {
  // Node reports each file by its real path, so the folders are compared as real paths too.
  const site = realpathSync(resolve(siteDir));
  const root = dirname(site);
  const model = join(root, MODEL_DIR) + sep;
  const plan = PLAN_DIRS.map((dir) => join(root, dir) + sep);
  const checks = join(site, SITE_CHECK_DIR) + sep;
  const files = new Set();
  for (const file of loadedFiles(site)) {
    const allowed = [model, ...plan].find((dir) => file.startsWith(dir));
    if (allowed && !file.slice(allowed.length).includes(sep)) {
      files.add(file);
    } else if (file.startsWith(join(site, 'server') + sep) || file.startsWith(join(site, 'api') + sep)) {
      files.add(file);
    } else if (file.startsWith(checks) && !file.slice(checks.length).includes(sep) && file.endsWith('.ts')) {
      files.add(file);
    } else if (file.startsWith(site + sep)) {
      throw new Error(`The function would load ${file}, in website/ but outside website/server/, website/api/ and the site's checks of website/src/workshop/.`);
    } else {
      throw new Error(`The function would load ${file}, outside website/.`);
    }
  }
  // The whole model ships, and the package.json that makes it ESM.
  for (const name of readdirSync(join(root, MODEL_DIR))) if (name.endsWith('.js')) files.add(join(root, MODEL_DIR, name));
  files.add(join(root, 'tools', 'workshop', 'package.json'));
  files.add(join(root, 'tools', 'plan', 'package.json'));
  for (const name of readdirSync(join(site, 'server'))) if (name.endsWith('.mjs') && !name.endsWith('.test.mjs')) files.add(join(site, 'server', name));
  files.add(join(site, 'api', 'workshop.cjs'));
  return { root, files: [...files].sort() };
}

// Response headers for every path, static pages included: no page of the site may be framed, except a
// film's (/films/), which the Workshop shows in a frame of its own origin.
export const FRAME_HEADERS = { 'Content-Security-Policy': "frame-ancestors 'none'", 'X-Frame-Options': 'DENY' };
export const FILM_FRAME_HEADERS = { 'Content-Security-Policy': "frame-ancestors 'self'", 'X-Frame-Options': 'SAMEORIGIN' };

// Files whose name changes when their content does (Vite's hashed assets), and the font files, which are
// never edited in place: a new font gets a new name. A browser keeps them and does not ask again. The
// header is added in the `hit` phase, so only to a file that exists: a 404 for a file of another
// deployment is never kept.
export const KEPT_FILES = '^/(assets/.*|fonts/.*\\.woff2)$';
export const KEPT_HEADERS = { 'Cache-Control': 'public, max-age=31536000, immutable' };

// `planDir` is the plan whose entries the server knows: plan/, or the one WORKSHOP_PLAN names.
export async function prepareVercel({ siteDir = website, distDir = join(siteDir, 'dist'), outputDir = DEFAULT_OUTPUT, planDir = defaultPlanDir() } = {}) {
  outputDir = resolve(outputDir);
  if (!outputDir.endsWith(`${sep}.vercel${sep}output`)) throw new Error('The output directory must end in .vercel/output; nothing was written.');
  const page = (await exists(join(distDir, 'workshop', 'index.html'))) ? '/workshop/index.html'
    : (await exists(join(distDir, 'workshop.html'))) ? '/workshop.html' : null;
  if (!page) throw new Error('The build has no Workshop page (workshop/index.html or workshop.html).');

  // What the function loads, and the plan's entries, are known before anything is written.
  const { root, files } = functionFiles(siteDir);
  const entries = planEntries(planDir);
  await rm(outputDir, { recursive: true, force: true });
  await mkdir(outputDir, { recursive: true });
  await cp(distDir, join(outputDir, 'static'), { recursive: true });

  const functions = join(outputDir, 'functions', 'api', 'workshop');
  const [first, ...others] = ENDPOINTS;
  const main = join(functions, `${first}.func`);
  for (const file of files) {
    const destination = join(main, relative(root, file));
    await mkdir(dirname(destination), { recursive: true });
    await cp(file, destination);
  }
  const entriesFile = join(main, 'website', 'server', ENTRIES_FILE);
  await writeFile(entriesFile, JSON.stringify(entries) + '\n');
  const shipped = [...files.map((file) => relative(root, file)), relative(main, entriesFile)];
  await writeFile(join(main, '.vc-config.json'), JSON.stringify({
    runtime: RUNTIME,
    handler: HANDLER,
    launcherType: 'Nodejs',
    shouldAddHelpers: false,
    maxDuration: 15,
  }, null, 2) + '\n');
  for (const name of others) await symlink(`${first}.func`, join(functions, `${name}.func`), 'dir');

  const config = {
    version: 3,
    routes: [
      // A route's src matches without regard to case unless caseSensitive is set: /FILMS/ must not get the looser rule.
      { src: '^/(?!films/).*$', caseSensitive: true, headers: FRAME_HEADERS, continue: true },
      { src: '^/films/.*$', caseSensitive: true, headers: FILM_FRAME_HEADERS, continue: true },
      { src: '^/workshop/?$', dest: page },
      // A film's page (scripts/films.mjs), with or without the closing slash: it names its files by absolute paths.
      { src: '^/films/([a-z0-9][a-z0-9-]*)/?$', caseSensitive: true, dest: '/films/$1/index.html' },
      { handle: 'filesystem' },
      { handle: 'hit' },
      { src: KEPT_FILES, headers: KEPT_HEADERS, continue: true },
    ],
  };
  await writeFile(join(outputDir, 'config.json'), JSON.stringify(config, null, 2) + '\n');
  return { outputDir, page, config, files: shipped };
}

// `--out <dir>` writes the output elsewhere.
if (import.meta.url === pathToFileURL(process.argv[1] || '').href) {
  const index = process.argv.indexOf('--out');
  await prepareVercel(index > 0 ? { outputDir: resolve(process.argv[index + 1]) } : {});
}
