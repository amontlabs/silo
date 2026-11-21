// The films of films/, as the site plays them: one page per film at /films/<id>/, holding the film alone
// (no entries, no questions: that page stays a tool of films/), and the player they share at /films/player/.
//
// A film is code and sound, not a video. Its page loads the kit's vendor scripts, the player of films/player/
// (voice.js, kit.js) and the film's explainer.js as separate files, with voice.json inlined and each clip's
// source pointing at the film's voice/ folder. Everything comes from tracked files of the repository: no
// Python, no network. Vite's build calls `writeFilms` (vite.config.ts); the dev server serves `filmFiles`.
//
// A folder directly under films/ that holds a film.json is a film. A film with a missing or broken file
// stops the build, with the file, the field and the fix, the way the plan's errors read.
import { lstatSync, readdirSync, readFileSync } from 'node:fs';
import { copyFile, mkdir, rm, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

/** films/ of this repository. A function, so that importing this file reads no path of its own (a test in a browser-like environment does). */
export const defaultFilmsDir = () => fileURLToPath(new URL('../../films', import.meta.url));
/** Where the site publishes the films: /films/<id>/ and /films/player/. */
export const FILMS_URL = '/films/';
/** The folder of the shared player, beside the films' pages; no film may take its name. */
export const PLAYER = 'player';

// The kit's files left unchanged, as tools/build.py takes them, in the order the page loads them.
const VENDOR = ['gsap.min.js', 'MotionPathPlugin.min.js', 'DrawSVGPlugin.min.js', 'MorphSVGPlugin.min.js', 'CustomEase.min.js', 'icons.js'];
const FONTS = ['geist-regular.otf', 'geist-medium.otf', 'geist-mono.ttf'];
const ID = /^[a-z0-9][a-z0-9-]*$/;

export class FilmError extends Error {}

const problem = (file, field, message, fix) => new FilmError(`${file}: ${field}: ${message}. Fix: ${fix}`);

/** A plain file (not a link, not a folder) at `path`. */
function isPlainFile(path) {
  try {
    return lstatSync(path).isFile();
  } catch {
    return false;
  }
}

/** The films of `filmsDir`: each real folder directly under it that holds a film.json, by name. No folder, no film. */
export function filmIds(filmsDir = defaultFilmsDir()) {
  let stat;
  try {
    stat = lstatSync(filmsDir);
  } catch {
    return [];
  }
  if (!stat.isDirectory()) return [];
  return readdirSync(filmsDir, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && isPlainFile(join(filmsDir, entry.name, 'film.json')))
    .map((entry) => entry.name)
    .sort();
}

function readJson(path, shown) {
  if (!isPlainFile(path)) return { missing: true };
  try {
    return { value: JSON.parse(readFileSync(path, 'utf8')) };
  } catch (error) {
    throw problem(shown, '(file)', `is not JSON (${error.message})`, 'write it as JSON again');
  }
}

/**
 * One film, checked: its title, its voice with each clip's source on the site, and the files its page needs.
 * Refuses a folder that is not a film directly under `filmsDir` (a link leading elsewhere included), and a
 * film with a missing or broken file.
 */
export function readFilm(filmsDir, id) {
  const dir = join(filmsDir, id);
  const shown = `films/${id}`;
  let stat;
  try {
    stat = lstatSync(dir);
  } catch {
    throw problem(shown, '(folder)', 'does not exist', `make the film's folder directly under films/, or leave ${id} out`);
  }
  if (!stat.isDirectory()) throw problem(shown, '(folder)', 'is not a folder (a link leading elsewhere is refused)', 'keep each film as a real folder directly under films/');
  if (!ID.test(id) || id === PLAYER) throw problem(shown, '(folder)', `"${id}" cannot be a film's address`, `name the folder with lowercase letters, digits and dashes, and not "${PLAYER}"`);
  const film = readJson(join(dir, 'film.json'), `${shown}/film.json`);
  if (film.missing) throw problem(`${shown}/film.json`, '(file)', 'is missing: the folder is not a film', 'write film.json as films/README.md says, or move the folder out of films/');
  if (film.value?.id !== id) throw problem(`${shown}/film.json`, 'id', `is ${JSON.stringify(film.value?.id)}, not the folder's name`, `set "id": "${id}"`);
  const title = film.value.title;
  if (typeof title !== 'string' || !title.trim()) throw problem(`${shown}/film.json`, 'title', 'is missing', "give the film's title, the script's title");
  if (!isPlainFile(join(dir, 'explainer.js'))) throw problem(`${shown}/explainer.js`, '(file)', 'is missing', 'write the film as films/README.md says');

  const voice = readJson(join(dir, 'voice.json'), `${shown}/voice.json`);
  if (voice.missing) throw problem(`${shown}/voice.json`, '(file)', 'is missing', `run tools/voice.py ${id} from films/`);
  const data = voice.value;
  if (!Array.isArray(data?.voices) || !data.voices.length) throw problem(`${shown}/voice.json`, 'voices', 'holds no voice', `run tools/voice.py ${id} from films/`);
  const clips = new Set();
  const voices = data.voices.map((v, vi) => ({
    ...v,
    scenes: (Array.isArray(v.scenes) ? v.scenes : []).map((scene, s) => ({
      ...scene,
      sentences: (Array.isArray(scene.sentences) ? scene.sentences : []).map((sentence, j) => {
        const field = `voices[${vi}].scenes[${s}].sentences[${j}].file`;
        const file = sentence.file;
        if (typeof file !== 'string' || !/^voice\/[A-Za-z0-9._-]+$/.test(file) || file.includes('..')) {
          throw problem(`${shown}/voice.json`, field, `${JSON.stringify(file)} is not a clip of the film's voice/`, `run tools/voice.py ${id} from films/`);
        }
        if (!isPlainFile(join(dir, file))) throw problem(`${shown}/${file}`, '(file)', `is missing, or is not a plain file (voice.json names it at ${field})`, `run tools/voice.py ${id} from films/, which speaks the missing sentences again`);
        clips.add(file);
        return { ...sentence, src: `${FILMS_URL}${id}/${file}` };
      }),
    })),
  }));
  if (!voices.some((v) => v.scenes.some((scene) => scene.sentences.length))) throw problem(`${shown}/voice.json`, 'voices', 'holds no sentence', `run tools/voice.py ${id} from films/`);
  return { id, title, dir, voice: { ...data, voices }, clips: [...clips].sort() };
}

// Safe inside <script>…</script>, as tools/build.py makes it.
const scriptText = (text) => text.replace(/<\/script/gi, '<\\/script').replace(/<!--/g, '<\\!--');
const escapeHtml = (text) => text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

/** Replaces `from` in `page` once, or stops: films/player/page.html no longer has the shape this step reads. */
function swap(page, from, to) {
  if (!page.includes(from)) throw new FilmError(`films/player/page.html: (file): no longer holds ${JSON.stringify(from)}. Fix: make website/scripts/films.mjs read the page as it is now`);
  return page.replace(from, () => to);
}

/** The page of one film: films/player/page.html without the review page's parts, the player and the film as files. */
export function filmPage(filmsDir, film) {
  const at = (path) => `${FILMS_URL}${path}`;
  const script = (path) => `<script src="${at(path)}"></script>`;
  let page = readFileSync(join(filmsDir, 'player', 'page.html'), 'utf8');
  page = swap(page, '<title><!--TITLE--></title>', `<title>${escapeHtml(film.title)}</title>`);
  // The site's theme (light, dark or the system's), kept by the home page and the Workshop; `?theme=` wins.
  page = swap(page, '<style><!--STYLE--></style>', [
    '<script src="/theme.js"></script>',
    `<link rel="stylesheet" href="${at(`${PLAYER}/player.css`)}">`,
    `<link rel="stylesheet" href="${at(`${PLAYER}/page.css`)}">`,
  ].join('\n'));
  page = swap(page, '<!--VENDOR-->', VENDOR.map((name) => script(`${PLAYER}/vendor/${name}`)).join('\n'));
  // The film alone: no title and line above it, no parts and no answers under it.
  page = page.replace(/\n\s*<header class="head">[\s\S]*?<\/header>\n\n?/, () => '\n');
  if (page.includes('class="head"')) throw new FilmError('films/player/page.html: (file): its header no longer reads as this step expects. Fix: make website/scripts/films.mjs read the page as it is now');
  page = swap(page, '\n  <div class="parts"></div>\n', '\n');
  page = page.replace(/\n\s*<section class="send"[\s\S]*?<\/section>\n/, () => '\n');
  if (page.includes('class="send"')) throw new FilmError('films/player/page.html: (file): its copy button no longer reads as this step expects. Fix: make website/scripts/films.mjs read the page as it is now');
  page = swap(page, '<script>window.FILM = <!--FILM-->;</script>\n', '');
  page = swap(page, '<!--VOICE-->', scriptText(JSON.stringify(film.voice)));
  page = swap(page, '<!--PLAYER-->', [script(`${PLAYER}/voice.js`), script(`${PLAYER}/kit.js`)].join('\n'));
  page = swap(page, '<script><!--EXPLAINER--></script>', script(`${film.id}/explainer.js`));
  page = swap(page, '<script><!--PAGE--></script>\n', '');
  if (page.includes('<!--')) throw new FilmError(`films/player/page.html: (file): holds a part this step does not fill (${page.match(/<!--[^>]*-->/)?.[0]}). Fix: make website/scripts/films.mjs fill it or leave it out`);
  return `${page.trimEnd()}\n<!-- The third-party files this page loads and their licences: ${at(`${PLAYER}/licences/`)} and films/kit/NOTICE.md in the repository. -->\n`;
}

/** The player the pages share: published path under /films/ → source file. */
export function playerFiles(filmsDir = defaultFilmsDir()) {
  const kit = join(filmsDir, 'kit', 'kit');
  const files = new Map([
    [`${PLAYER}/player.css`, join(kit, 'player.css')],
    [`${PLAYER}/page.css`, join(filmsDir, 'player', 'page.css')],
    [`${PLAYER}/voice.js`, join(filmsDir, 'player', 'voice.js')],
    [`${PLAYER}/kit.js`, join(filmsDir, 'player', 'kit.js')],
    ...VENDOR.map((name) => [`${PLAYER}/vendor/${name}`, join(kit, 'vendor', name)]),
    ...FONTS.map((name) => [`${PLAYER}/fonts/${name}`, join(kit, 'fonts', name)]),
  ]);
  // The icons' and the fonts' licences ask that their text goes with every copy (films/kit/NOTICE.md).
  const licences = join(filmsDir, 'kit', 'licences');
  for (const name of readdirSync(licences).filter((name) => name.endsWith('.txt')).sort()) files.set(`${PLAYER}/licences/${name}`, join(licences, name));
  for (const [path, from] of files) if (!isPlainFile(from)) throw problem(from.slice(dirname(filmsDir).length + 1), '(file)', 'is missing', `restore it from git (the site publishes it as /films/${path})`);
  return files;
}

/**
 * Every file the site publishes under /films/: published path → `{ from }` (a file copied as it is) or
 * `{ text }` (a page). Throws a FilmError for the first film that is not complete.
 */
export function filmFiles(filmsDir = defaultFilmsDir()) {
  const ids = filmIds(filmsDir);
  if (!ids.length) return { ids, files: new Map() };
  const files = new Map([...playerFiles(filmsDir)].map(([path, from]) => [path, { from }]));
  for (const id of ids) {
    const film = readFilm(filmsDir, id);
    files.set(`${id}/index.html`, { text: filmPage(filmsDir, film) });
    files.set(`${id}/explainer.js`, { from: join(film.dir, 'explainer.js') });
    for (const clip of film.clips) files.set(`${id}/${clip}`, { from: join(film.dir, clip) });
  }
  return { ids, files };
}

/** Writes every film's page and the shared player into `outDir`/films/, and gives the films written. */
export async function writeFilms(outDir, filmsDir = defaultFilmsDir()) {
  const { ids, files } = filmFiles(filmsDir);
  const root = join(outDir, 'films');
  await rm(root, { recursive: true, force: true });
  for (const [path, file] of files) {
    const target = join(root, ...path.split('/'));
    await mkdir(dirname(target), { recursive: true });
    if ('text' in file) await writeFile(target, file.text);
    else await copyFile(file.from, target);
  }
  return ids;
}

/** The file for a request path under /films/ (`/films/<id>/` gives its page), or null. */
export function filmFileFor(files, pathname) {
  if (!pathname.startsWith(FILMS_URL)) return null;
  let path = pathname.slice(FILMS_URL.length);
  if (path === '' || path.endsWith('/')) path += 'index.html';
  else if (ID.test(path)) path += '/index.html';
  return files.get(path) ?? null;
}

/** The films the site plays: every film of films/, each one's page written by `writeFilms`. */
export function playableFilms(filmsDir = defaultFilmsDir()) {
  return filmFiles(filmsDir).ids;
}
