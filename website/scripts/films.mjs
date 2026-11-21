// The films of films/, as the site plays them: one page per film at /films/<id>/, holding the film alone
// (no entries, no questions: that page stays a tool of films/), and the player they share at /films/player/.
//
// A film is code and sound, not a video. Its page loads the kit's vendor scripts, the player of films/player/
// (voice.js, kit.js) and the film's explainer.js as separate files, with voice.json inlined and each clip's
// source pointing at the film's voice/ folder. Everything comes from tracked files of the repository: no
// Python, no network. Vite's build calls `writeFilms` (vite.config.ts); the dev server serves `filmFiles`.
//
// A folder directly under films/ that holds a film.json, an explainer.js or a voice.json is a film, and a film
// is complete: one with a missing or broken file stops the build, with the file, the field and the fix, the way the plan's errors read.
//
// What the build reads or copies is a fixed set of files under films/, each reached through plain folders
// only: a link anywhere on the way, or a real path outside films/, stops the build. So no file outside
// films/ can be published, nor its text put into a page.
import { lstatSync, readdirSync, readFileSync, realpathSync } from 'node:fs';
import { copyFile, mkdir, rm, writeFile } from 'node:fs/promises';
import { dirname, join, sep } from 'node:path';
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
// The texts the icons' and the fonts' licences ask to go with every copy (films/kit/NOTICE.md).
const LICENCES = ['geist.txt', 'lucide.txt'];
const ID = /^[a-z0-9][a-z0-9-]*$/;
// A clip of a film: an MP3 directly in its voice/ folder, as tools/voice.py names them.
const CLIP = /^voice\/[A-Za-z0-9_-]+\.mp3$/;
/** The page every film's page is made from, under films/. */
const TEMPLATE = 'player/page.html';

/** The files the pages share: published path under /films/ → source path under films/. Nothing is found by listing a folder. */
const SHARED = new Map([
  ['player/player.css', 'kit/kit/player.css'],
  ['player/page.css', 'player/page.css'],
  ['player/voice.js', 'player/voice.js'],
  ['player/kit.js', 'player/kit.js'],
  ...VENDOR.map((name) => [`player/vendor/${name}`, `kit/kit/vendor/${name}`]),
  ...FONTS.map((name) => [`player/fonts/${name}`, `kit/kit/fonts/${name}`]),
  ...LICENCES.map((name) => [`player/licences/${name}`, `kit/licences/${name}`]),
]);

export class FilmError extends Error {}

const problem = (file, field, message, fix) => new FilmError(`${file}: ${field}: ${message}. Fix: ${fix}`);

/**
 * The file at `rel` (a path under films/, with /), checked before the build reads or copies it: every folder
 * on the way is a plain folder, the file a plain file, and its real path is inside the real path of films/.
 * Gives null for a file that is missing, and throws for a link, naming the file and the link.
 */
function plainFile(filmsDir, rel) {
  const parts = rel.split('/');
  let at = filmsDir;
  for (const [i, part] of parts.entries()) {
    at = join(at, part);
    let stat;
    try {
      stat = lstatSync(at);
    } catch {
      return null;
    }
    const last = i === parts.length - 1;
    const shown = `films/${parts.slice(0, i + 1).join('/')}`;
    if (stat.isSymbolicLink()) throw problem(`films/${rel}`, '(file)', `is reached through a link, ${shown}`, `make ${shown} a plain ${last ? 'file' : 'folder'}: the site publishes nothing a link leads to`);
    if (!last && !stat.isDirectory()) return null;
    if (last && !stat.isFile()) throw problem(`films/${rel}`, '(file)', 'is not a plain file', 'make it a plain file');
  }
  const root = realpathSync(filmsDir);
  if (!realpathSync(at).startsWith(root + sep)) throw problem(`films/${rel}`, '(file)', 'is outside films/', 'keep it under films/ as a plain file');
  return at;
}

/** The file at `rel`, as `plainFile` checks it; a missing one stops the build with `fix`. */
function requiredFile(filmsDir, rel, fix) {
  const path = plainFile(filmsDir, rel);
  if (!path) throw problem(`films/${rel}`, '(file)', 'is missing', fix);
  return path;
}

// A folder directly under films/ that holds one of these, as a file or as a link, is a film, and must be complete.
const FILM_FILES = ['film.json', 'explainer.js', 'voice.json'];

/** Whether the entry at `path` holds one of FILM_FILES. */
function holdsFilm(path) {
  return FILM_FILES.some((name) => {
    try {
      return Boolean(lstatSync(join(path, name), { throwIfNoEntry: false }));
    } catch {
      return false;
    }
  });
}

/**
 * The films of `filmsDir`, by name: each folder directly under it that holds a film.json, an explainer.js or a
 * voice.json. A link in place of such a folder stops the build, so a film cannot drop out of the site unseen.
 * No folder, no film.
 */
export function filmIds(filmsDir = defaultFilmsDir()) {
  let stat;
  try {
    stat = lstatSync(filmsDir);
  } catch {
    return [];
  }
  if (!stat.isDirectory()) return [];
  const ids = [];
  for (const entry of readdirSync(filmsDir, { withFileTypes: true })) {
    if (!(entry.isDirectory() || entry.isSymbolicLink()) || !holdsFilm(join(filmsDir, entry.name))) continue;
    if (entry.isSymbolicLink()) throw problem(`films/${entry.name}`, '(folder)', 'is a link to a film', 'make the film a plain folder directly under films/: the site publishes nothing a link leads to');
    ids.push(entry.name);
  }
  return ids.sort();
}

function readJson(filmsDir, rel) {
  const path = plainFile(filmsDir, rel);
  if (!path) return { missing: true };
  const shown = `films/${rel}`;
  try {
    return { value: JSON.parse(readFileSync(path, 'utf8')) };
  } catch (error) {
    throw problem(shown, '(file)', `is not JSON (${error.message})`, 'write it as JSON again');
  }
}

/**
 * One film, checked: its title, its voice with each clip's source on the site, and the files its page needs.
 * Refuses a folder that is not a film directly under `filmsDir` (a link to one included), and a film with a
 * missing or broken file, or a voice without a field the player reads.
 */
export function readFilm(filmsDir, id) {
  const shown = `films/${id}`;
  let stat;
  try {
    stat = lstatSync(join(filmsDir, id));
  } catch {
    throw problem(shown, '(folder)', 'does not exist', `make the film's folder directly under films/, or leave ${id} out`);
  }
  if (stat.isSymbolicLink()) throw problem(shown, '(folder)', 'is a link to a film', 'make the film a plain folder directly under films/: the site publishes nothing a link leads to');
  if (!stat.isDirectory()) throw problem(shown, '(folder)', 'is not a folder', 'keep each film as a plain folder directly under films/');
  if (!ID.test(id) || id === PLAYER) throw problem(shown, '(folder)', `"${id}" cannot be a film's address`, `name the folder with lowercase letters, digits and dashes, and not "${PLAYER}"`);
  const film = readJson(filmsDir, `${id}/film.json`);
  if (film.missing) throw problem(`${shown}/film.json`, '(file)', 'is missing', 'write film.json as films/README.md says, or move the folder out of films/');
  if (film.value?.id !== id) throw problem(`${shown}/film.json`, 'id', `is ${JSON.stringify(film.value?.id)}, not the folder's name`, `set "id": "${id}"`);
  const title = film.value.title;
  if (typeof title !== 'string' || !title.trim()) throw problem(`${shown}/film.json`, 'title', 'is missing', "give the film's title, the script's title");
  const explainer = requiredFile(filmsDir, `${id}/explainer.js`, 'write the film as films/README.md says');

  const voice = readJson(filmsDir, `${id}/voice.json`);
  if (voice.missing) throw problem(`${shown}/voice.json`, '(file)', 'is missing', `run tools/voice.py ${id} from films/`);
  const data = voice.value;
  if (!Array.isArray(data?.voices) || !data.voices.length) throw problem(`${shown}/voice.json`, 'voices', 'holds no voice', `run tools/voice.py ${id} from films/`);
  const clips = new Map();
  // The fields the player reads (films/player/voice.js), for every voice and every scene of it.
  const needs = (value, field, what, voiceId) => {
    if (!what(value)) throw problem(`${shown}/voice.json`, field, `is missing or wrong${voiceId === undefined ? '' : ` (voice ${JSON.stringify(voiceId)})`}`, `run tools/voice.py ${id} from films/`);
  };
  const text = (value) => typeof value === 'string' && value !== '';
  const number = (value) => typeof value === 'number' && Number.isFinite(value);
  const voices = data.voices.map((v, vi) => {
    needs(v, `voices[${vi}]`, (value) => value !== null && typeof value === 'object');
    needs(v.id, `voices[${vi}].id`, text);
    needs(v.label, `voices[${vi}].label`, text, v.id);
    if (!Array.isArray(v.scenes) || !v.scenes.length) throw problem(`${shown}/voice.json`, `voices[${vi}].scenes`, `holds no scene (voice ${JSON.stringify(v.id)})`, `run tools/voice.py ${id} from films/`);
    const scenes = v.scenes.map((scene, s) => {
      const at = `voices[${vi}].scenes[${s}]`;
      needs(scene, at, (value) => value !== null && typeof value === 'object', v.id);
      needs(scene.total, `${at}.total`, number, v.id);
      needs(scene.sentences, `${at}.sentences`, Array.isArray, v.id);
      const sentences = scene.sentences.map((sentence, j) => {
        const where = `${at}.sentences[${j}]`;
        needs(sentence, where, (value) => value !== null && typeof value === 'object', v.id);
        needs(sentence.text, `${where}.text`, (value) => typeof value === 'string', v.id);
        needs(sentence.start, `${where}.start`, number, v.id);
        needs(sentence.length, `${where}.length`, number, v.id);
        const file = sentence.file;
        if (typeof file !== 'string' || !CLIP.test(file)) {
          throw problem(`${shown}/voice.json`, `${where}.file`, `${JSON.stringify(file)} is not a clip of the film's voice/ (voice/<name>.mp3)`, `run tools/voice.py ${id} from films/`);
        }
        const from = plainFile(filmsDir, `${id}/${file}`);
        if (!from) throw problem(`${shown}/${file}`, '(file)', `is missing (voice.json names it at ${where}.file)`, `run tools/voice.py ${id} from films/, which speaks the missing sentences again`);
        clips.set(file, from);
        return { ...sentence, src: `${FILMS_URL}${id}/${file}` };
      });
      return { ...scene, sentences };
    });
    return { ...v, scenes };
  });
  if (!voices.some((v) => v.scenes.some((scene) => scene.sentences.length))) throw problem(`${shown}/voice.json`, 'voices', 'holds no sentence', `run tools/voice.py ${id} from films/`);
  return { id, title, explainer, voice: { ...data, voices }, clips: new Map([...clips].sort(([a], [b]) => (a < b ? -1 : 1))) };
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
  let page = readFileSync(requiredFile(filmsDir, TEMPLATE, 'restore it from git'), 'utf8');
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
  const licences = LICENCES.map((name) => at(`${PLAYER}/licences/${name}`)).join(' and ');
  return `${page.trimEnd()}\n<!-- The third-party files this page loads and their licences: ${licences}, and films/kit/NOTICE.md in the repository. -->\n`;
}

/** The player the pages share: published path under /films/ → source file, each one of the fixed list, checked. */
export function playerFiles(filmsDir = defaultFilmsDir()) {
  return new Map([...SHARED].map(([path, rel]) => [path, requiredFile(filmsDir, rel, `restore it from git (the site publishes it as /films/${path})`)]));
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
    files.set(`${id}/explainer.js`, { from: film.explainer });
    for (const [clip, from] of film.clips) files.set(`${id}/${clip}`, { from });
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
