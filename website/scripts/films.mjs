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
 * voice.json. films/ must be a plain folder, and any link directly under it stops the build, whatever it leads
 * to, so a film cannot drop out of the site unseen; a name that starts with a dot, which tools leave there, is
 * passed over. No folder, no film.
 */
export function filmIds(filmsDir = defaultFilmsDir()) {
  let stat;
  try {
    stat = lstatSync(filmsDir);
  } catch {
    return [];
  }
  if (stat.isSymbolicLink()) throw problem('films', '(folder)', 'is a link', 'make films/ a plain folder: the site publishes nothing a link leads to');
  if (!stat.isDirectory()) throw problem('films', '(folder)', 'is not a folder', 'make films/ a plain folder');
  const ids = [];
  for (const entry of readdirSync(filmsDir, { withFileTypes: true })) {
    if (entry.name.startsWith('.')) continue;
    if (entry.isSymbolicLink()) throw problem(`films/${entry.name}`, '(folder)', 'is a link', `make films/${entry.name} a plain folder or file, or remove it: the site publishes nothing a link leads to`);
    if (entry.isDirectory() && holdsFilm(join(filmsDir, entry.name))) ids.push(entry.name);
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
  if (stat.isSymbolicLink()) throw problem(shown, '(folder)', 'is a link', `make films/${id} a plain folder or file, or remove it: the site publishes nothing a link leads to`);
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

/**
 * What the page opens on: the film itself, paused at the end of its last scene (the frame the player holds for
 * `?scene=<last>`), with the Play control over it. That moment shows the whole of what the film builds; the end of its
 * first scene is often an empty stage. The first press on Play, the play control or the space bar starts the film from
 * its beginning. An address that asks for a moment (`t`, `scene`, `embed`) is left as the player reads it.
 * In the Workshop's frame (`?in=workshop`) the page takes the Workshop's ground, keeps no margin of its own, and tells
 * the Workshop its height; a narrow frame shows the stage alone until the film plays.
 */
const PICTURE = `(() => {
  const q = new URLSearchParams(location.search);
  const root = document.documentElement;
  const framed = q.get('in') === 'workshop' && parent !== window;
  if (framed) root.classList.add('in-workshop');
  // \`shown\`: the picture was put up once; \`held\`: it is still what the stage shows, until the first press.
  let shown = false, held = false;
  const hold = () => {
    const timing = window.__timing, player = window.__player;
    if (shown || q.has('t') || q.has('scene') || q.has('embed') || !timing || !player || !timing.scenes.length) return;
    held = true;
    player.seek(timing.scenes[timing.scenes.length - 1][1]);
    shown = true;
    atStart();
  };
  const fromStart = (event) => {
    if (!held || (event.type === 'keydown' && event.key !== ' ')) return;
    if (event.type === 'click' && !event.target.closest('.bigplay, .pp')) return;
    held = false;
    window.__player && window.__player.seek(0);
  };
  addEventListener('click', fromStart, true);
  addEventListener('keydown', fromStart, true);
  // While the picture is held, the bar reads as at the start: no time gone, no scene played. The picture is the end of
  // the last scene; the bar is not.
  const clock = (t) => Math.floor(t / 60) + ':' + String(Math.floor(t % 60)).padStart(2, '0');
  const atStart = () => {
    const tl = window.__film && window.__film.timeline, time = document.querySelector('.time');
    if (!held || !tl || !time) return;
    const text = '0:00 / ' + clock(tl.duration());
    if (time.textContent !== text) time.textContent = text;
    document.querySelectorAll('.segs .seg').forEach((seg, k) => {
      if (seg.firstChild && seg.firstChild.style.transform !== 'scaleX(0)') seg.firstChild.style.transform = 'scaleX(0)';
      if (seg.classList.contains('on') !== (k === 0)) seg.classList.toggle('on', k === 0);
    });
  };
  const nav = document.querySelector('nav');
  if (nav) new MutationObserver(atStart).observe(nav, { subtree: true, childList: true, characterData: true, attributes: true, attributeFilter: ['style', 'class'] });
  const started = () => {
    const tl = window.__film && window.__film.timeline;
    if (tl && !tl.paused()) root.classList.add('started');
  };
  new MutationObserver(() => { if (document.body.dataset.ready === '1') hold(); started(); }).observe(document.body, { attributes: true, subtree: true, attributeFilter: ['data-ready', 'hidden'] });
  if (document.body.dataset.ready === '1') hold();
  if (framed && typeof ResizeObserver === 'function') {
    const tell = () => parent.postMessage({ film: location.pathname, height: Math.ceil(document.querySelector('main').getBoundingClientRect().height) }, location.origin);
    new ResizeObserver(tell).observe(document.querySelector('main'));
  }
})();`;

/** The page in the Workshop's frame: the Workshop's paper around the stage, in both themes, and no margin of its own. */
const FRAMED = [
  ':root.in-workshop, .in-workshop body { background: #f7f7f4; }',
  ':root.in-workshop[data-theme="dark"], :root.in-workshop[data-theme="dark"] body { background: #171a1b; }',
  '.in-workshop body { margin: 0; overflow: hidden; }',
  '.in-workshop main { max-width: none; padding: 0; }',
  '.in-workshop #frame .caption { margin: 0 0 8px; font-size: 16px; min-height: 24px; }',
  '.in-workshop .subtitle { margin-top: 10px; font-size: 17px; }',
  '@media (max-width: 480px) {',
  '  .in-workshop:not(.started) #frame > :not(.screen) { display: none; }',
  // Small, the control is a round play button of 40 px with no word, so the picture stays in sight.
  '  .in-workshop .bigplay { width: 40px; height: 40px; padding: 0; gap: 0; justify-content: center; border: 1px solid var(--ink); background: var(--bg); color: var(--ink); box-shadow: none; }',
  '  .in-workshop .bigplay span { display: none; }',
  '  .in-workshop .bigplay svg { width: 18px; height: 18px; margin-left: 2px; }',
  '  .in-workshop #frame .caption { font-size: 13px; min-height: 18px; margin-bottom: 6px; }',
  '  .in-workshop .subtitle { font-size: 13px; margin-top: 6px; }',
  '}',
].join('\n');

/** The page of one film: films/player/page.html without the review page's parts, the player and the film as files. */
export function filmPage(filmsDir, film) {
  const at = (path) => `${FILMS_URL}${path}`;
  const script = (path) => `<script src="${at(path)}"></script>`;
  let page = readFileSync(requiredFile(filmsDir, TEMPLATE, 'restore it from git'), 'utf8');
  page = swap(page, '<title><!--TITLE--></title>', `<title>${escapeHtml(film.title)}</title>`);
  // The site's theme (light, dark or the system's), kept by the home page and the Workshop, as on every page.
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
  page = swap(page, '<script><!--PAGE--></script>\n', `<script>${PICTURE}</script>\n`);
  page = swap(page, '</head>', `<style>${FRAMED}</style>\n</head>`);
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
