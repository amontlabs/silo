import { test } from 'node:test';
import assert from 'node:assert/strict';
import { copyFileSync, cpSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, renameSync, rmSync, symlinkSync, unlinkSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { defaultFilmsDir, FilmError, filmFileFor, filmFiles, filmIds, readFilm, writeFilms } from './films.mjs';
import { made, VENDOR } from './films-test-support.mjs';

const files = (dir) => readdirSync(dir, { recursive: true, withFileTypes: true }).filter((e) => e.isFile()).map((e) => join(e.parentPath, e.name).slice(dir.length + 1)).sort();

test('writes each film\'s page, its explainer and the clips its voice names, and the player they share', async () => {
  const { root, films } = made();
  try {
    assert.deepEqual(filmIds(films), ['tiny']);
    const out = join(root, 'dist');
    assert.deepEqual(await writeFilms(out, films), ['tiny']);
    assert.deepEqual(files(join(out, 'films')), [
      'player/fonts/geist-medium.otf', 'player/fonts/geist-mono.ttf', 'player/fonts/geist-regular.otf',
      'player/kit.js', 'player/licences/geist.txt', 'player/licences/lucide.txt', 'player/page.css', 'player/player.css',
      ...VENDOR.map((name) => `player/vendor/${name}`).sort(), 'player/voice.js',
      'tiny/explainer.js', 'tiny/index.html', 'tiny/voice/a.mp3', 'tiny/voice/b.mp3',
    ].sort());
    const page = readFileSync(join(out, 'films', 'tiny', 'index.html'), 'utf8');
    assert.match(page, /<title>A &lt;tiny&gt; film<\/title>/);
    // The film alone: no title or line above it, no parts, no answers, no page.js.
    for (const gone of ['class="head"', 'class="parts"', 'class="send"', 'Copy my answers', 'window.FILM =', 'page.js', '<!--TITLE', '<!--LINE']) assert.ok(!page.includes(gone), gone);
    for (const kept of ['id="stage"', 'class="bigplay"', 'class="subtitle"', '<nav>', 'id="errors"']) assert.ok(page.includes(kept), kept);
    // Loaded in the order the player needs: the theme, the styles, the vendor scripts, the voice, the kit, the film.
    const order = ['/theme.js', '/films/player/player.css', '/films/player/page.css', ...VENDOR.map((name) => `/films/player/vendor/${name}`), 'FILM_VOICE_DATA', '/films/player/voice.js', '/films/player/kit.js', '/films/tiny/explainer.js'];
    const at = order.map((name) => page.indexOf(name));
    assert.ok(at.every((n, i) => n > 0 && (i === 0 || n > at[i - 1])), JSON.stringify(at));
    const line = page.split('\n').find((text) => text.includes('FILM_VOICE_DATA'));
    assert.equal(line.match(/<\/script/g).length, 1, 'a sentence cannot close the script');
    const voice = JSON.parse(line.match(/window\.FILM_VOICE_DATA = (.*);<\/script>/)[1]);
    assert.deepEqual(voice.voices[0].scenes.map((s) => s.sentences[0].src), ['/films/tiny/voice/a.mp3', '/films/tiny/voice/b.mp3']);
    assert.match(page, /<!-- The third-party files this page loads and their licences: \/films\/player\/licences\/geist\.txt and \/films\/player\/licences\/lucide\.txt, and films\/kit\/NOTICE\.md in the repository\. -->\n$/);
    assert.equal(readFileSync(join(out, 'films', 'tiny', 'explainer.js'), 'utf8'), 'window.TINY = "</script>";');
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('a film with a missing clip stops the build with the film, the file and the fix, and writes nothing', async () => {
  const { root, films } = made();
  try {
    unlinkSync(join(films, 'tiny', 'voice', 'b.mp3'));
    const out = join(root, 'dist');
    await assert.rejects(writeFilms(out, films), (error) => {
      assert.ok(error instanceof FilmError);
      assert.equal(error.message, 'films/tiny/voice/b.mp3: (file): is missing (voice.json names it at voices[0].scenes[1].sentences[0].file). Fix: run tools/voice.py tiny from films/, which speaks the missing sentences again');
      return true;
    });
    assert.deepEqual(readdirSync(root), ['films']);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('a clip that is a link, or any name but voice/<name>.mp3, is refused', () => {
  const { root, films } = made();
  try {
    unlinkSync(join(films, 'tiny', 'voice', 'a.mp3'));
    symlinkSync(join(films, 'tiny', 'voice', 'b.mp3'), join(films, 'tiny', 'voice', 'a.mp3'));
    assert.throws(() => readFilm(films, 'tiny'), (error) => error instanceof FilmError && error.message === 'films/tiny/voice/a.mp3: (file): is reached through a link, films/tiny/voice/a.mp3. Fix: make films/tiny/voice/a.mp3 a plain file: the site publishes nothing a link leads to');
    const original = readFileSync(join(films, 'tiny', 'voice.json'), 'utf8');
    for (const file of ['voice/../../other/x.mp3', 'voice/a.wav', 'voice/a.mp3.txt', 'voice/sub/a.mp3', 'explainer.js', '/etc/a.mp3', 'voice/.mp3', 'voice/a.MP3', 42]) {
      const voice = JSON.parse(original);
      voice.voices[0].scenes[0].sentences[0].file = file;
      writeFileSync(join(films, 'tiny', 'voice.json'), JSON.stringify(voice));
      assert.throws(() => readFilm(films, 'tiny'), (error) => error instanceof FilmError && error.message === `films/tiny/voice.json: voices[0].scenes[0].sentences[0].file: ${JSON.stringify(file)} is not a clip of the film's voice/ (voice/<name>.mp3). Fix: run tools/voice.py tiny from films/`, String(file));
    }
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('a link on the way to any file the build reads is refused, naming the file and the link', async () => {
  // Each case replaces a folder or a file of films/ with a link to a copy of it outside films/.
  const cases = [
    ['tiny/voice', 'films/tiny/voice/a.mp3', 'folder'],
    ['player', 'films/player/page.css', 'folder'],
    ['kit/kit', 'films/kit/kit/player.css', 'folder'],
    ['kit/kit/vendor', 'films/kit/kit/vendor/gsap.min.js', 'folder'],
    ['kit/kit/fonts', 'films/kit/kit/fonts/geist-regular.otf', 'folder'],
    ['kit/licences', 'films/kit/licences/geist.txt', 'folder'],
    ['player/page.html', 'films/player/page.html', 'file'],
    ['player/voice.js', 'films/player/voice.js', 'file'],
    ['tiny/explainer.js', 'films/tiny/explainer.js', 'file'],
  ];
  for (const [linked, file, kind] of cases) {
    const { root, films } = made();
    try {
      const outside = join(root, 'outside');
      cpSync(join(films, linked), outside, { recursive: true });
      rmSync(join(films, linked), { recursive: true });
      symlinkSync(outside, join(films, linked));
      const out = join(root, 'dist');
      await assert.rejects(writeFilms(out, films), (error) => {
        assert.ok(error instanceof FilmError, linked);
        // A link directly under films/ is refused before any file is read through it.
        const expected = linked.includes('/')
          ? `${file}: (file): is reached through a link, films/${linked}. Fix: make films/${linked} a plain ${kind}: the site publishes nothing a link leads to`
          : `films/${linked}: (folder): is a link. Fix: make films/${linked} a plain folder or file, or remove it: the site publishes nothing a link leads to`;
        assert.equal(error.message, expected);
        return true;
      });
      assert.ok(!existsSync(out), linked);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});

test('a shared file missing from the fixed list stops the build: a licence text cannot drop out', async () => {
  const { root, films } = made();
  try {
    unlinkSync(join(films, 'kit', 'licences', 'lucide.txt'));
    await assert.rejects(writeFilms(join(root, 'dist'), films), (error) => error instanceof FilmError && error.message === 'films/kit/licences/lucide.txt: (file): is missing. Fix: restore it from git (the site publishes it as /films/player/licences/lucide.txt)');
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('a folder that holds none of a film\'s files is not a film; any link directly under films/ stops the build', () => {
  const { root, films, put } = made();
  try {
    assert.deepEqual(filmIds(films), ['tiny']);
    assert.throws(() => readFilm(films, 'tools'), (error) => error instanceof FilmError && error.message === 'films/tools/film.json: (file): is missing. Fix: write film.json as films/README.md says, or move the folder out of films/');
    assert.throws(() => readFilm(films, 'nowhere'), /^Error: films\/nowhere: \(folder\): does not exist/);
    // Any link directly under films/ stops the build, whatever it leads to: a film, an empty folder, nothing.
    mkdirSync(join(root, 'empty'));
    for (const [name, target] of [['linked', join(films, 'tiny')], ['to-empty', join(root, 'empty')], ['dangling', join(root, 'nowhere')]]) {
      symlinkSync(target, join(films, name));
      const message = `films/${name}: (folder): is a link. Fix: make films/${name} a plain folder or file, or remove it: the site publishes nothing a link leads to`;
      assert.throws(() => filmIds(films), (error) => error instanceof FilmError && error.message === message, name);
      assert.throws(() => readFilm(films, name), (error) => error instanceof FilmError && error.message === message, name);
      unlinkSync(join(films, name));
    }
    // A name that starts with a dot is passed over, link or not.
    symlinkSync(join(root, 'nowhere'), join(films, '.cache'));
    assert.deepEqual(filmIds(films), ['tiny']);
    // The shared player's folder cannot be a film's address.
    put('player/film.json', JSON.stringify({ id: 'player', title: 'Player' }));
    assert.throws(() => filmFiles(films), /films\/player: \(folder\): "player" cannot be a film's address/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('films/ itself as a link stops the build, naming it', async () => {
  const { root, films } = made();
  try {
    const real = join(root, 'real-films');
    renameSync(films, real);
    symlinkSync(real, films);
    const message = 'films: (folder): is a link. Fix: make films/ a plain folder: the site publishes nothing a link leads to';
    assert.throws(() => filmIds(films), (error) => error instanceof FilmError && error.message === message);
    await assert.rejects(writeFilms(join(root, 'dist'), films), (error) => error instanceof FilmError && error.message === message);
    assert.ok(!existsSync(join(root, 'dist')));
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('a folder with any one of film.json, explainer.js or voice.json is a film, and an incomplete one stops the build', async () => {
  for (const [held, missing] of [['film.json', 'explainer.js'], ['explainer.js', 'film.json'], ['voice.json', 'film.json']]) {
    const { root, films, put } = made();
    try {
      put(`half/${held}`, held === 'explainer.js' ? '' : JSON.stringify({ id: 'half', title: 'Half', voices: [] }));
      assert.deepEqual(filmIds(films), ['half', 'tiny'], held);
      await assert.rejects(writeFilms(join(root, 'dist'), films), (error) => {
        assert.ok(error instanceof FilmError);
        assert.match(error.message, new RegExp(`^films/half/${missing.replace('.', '\\.')}: \\(file\\): is missing\\.`), held);
        return true;
      });
      // A film's file that is a link counts: the folder is a film, and the link stops the build.
      rmSync(join(films, 'half'), { recursive: true });
      mkdirSync(join(films, 'half'));
      symlinkSync(join(films, 'tiny', held), join(films, 'half', held));
      assert.deepEqual(filmIds(films), ['half', 'tiny'], `${held} as a link`);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});

test('every voice has, for each scene, the fields the player reads: one missing names the film, the voice and the field', () => {
  const cases = [
    [(v) => { v.scenes = []; }, 'voices[0].scenes: holds no scene (voice "af_heart")'],
    [(v) => { delete v.scenes; }, 'voices[0].scenes: holds no scene (voice "af_heart")'],
    [(v) => { delete v.id; }, 'voices[0].id: is missing or wrong'],
    [(v) => { delete v.label; }, 'voices[0].label: is missing or wrong (voice "af_heart")'],
    [(v) => { delete v.scenes[1].total; }, 'voices[0].scenes[1].total: is missing or wrong (voice "af_heart")'],
    [(v) => { delete v.scenes[1].sentences; }, 'voices[0].scenes[1].sentences: is missing or wrong (voice "af_heart")'],
    [(v) => { delete v.scenes[0].sentences[0].text; }, 'voices[0].scenes[0].sentences[0].text: is missing or wrong (voice "af_heart")'],
    [(v) => { v.scenes[0].sentences[0].start = '0.4'; }, 'voices[0].scenes[0].sentences[0].start: is missing or wrong (voice "af_heart")'],
    [(v) => { delete v.scenes[1].sentences[0].length; }, 'voices[0].scenes[1].sentences[0].length: is missing or wrong (voice "af_heart")'],
  ];
  for (const [change, expected] of cases) {
    const { root, films } = made();
    try {
      const voice = JSON.parse(readFileSync(join(films, 'tiny', 'voice.json'), 'utf8'));
      change(voice.voices[0]);
      writeFileSync(join(films, 'tiny', 'voice.json'), JSON.stringify(voice));
      assert.throws(() => readFilm(films, 'tiny'), (error) => error instanceof FilmError && error.message === `films/tiny/voice.json: ${expected}. Fix: run tools/voice.py tiny from films/`, expected);
    } finally {
      rmSync(root, { recursive: true, force: true });
    }
  }
});

test('a film whose film.json or voice.json is wrong or missing is refused with the fix', () => {
  const { root, films, put } = made();
  try {
    put('tiny/film.json', JSON.stringify({ id: 'other', title: 'T' }));
    assert.throws(() => readFilm(films, 'tiny'), /films\/tiny\/film\.json: id: is "other", not the folder's name\. Fix: set "id": "tiny"/);
    put('tiny/film.json', '{ nope');
    assert.throws(() => readFilm(films, 'tiny'), /films\/tiny\/film\.json: \(file\): is not JSON/);
    put('tiny/film.json', JSON.stringify({ id: 'tiny', title: 'T' }));
    unlinkSync(join(films, 'tiny', 'voice.json'));
    assert.throws(() => readFilm(films, 'tiny'), /films\/tiny\/voice\.json: \(file\): is missing\. Fix: run tools\/voice\.py tiny from films\//);
    unlinkSync(join(films, 'tiny', 'explainer.js'));
    assert.throws(() => readFilm(films, 'tiny'), /films\/tiny\/explainer\.js: \(file\): is missing/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('a films/ with no film publishes nothing, and a missing folder has no film', async () => {
  const { root, films } = made();
  try {
    rmSync(join(films, 'tiny'), { recursive: true });
    assert.deepEqual(await writeFilms(join(root, 'dist'), films), []);
    assert.deepEqual(filmIds(join(root, 'missing')), []);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('a request path finds its file: a film\'s address gives its page, with or without the closing slash', () => {
  const { files } = { files: new Map([['a/index.html', { text: 'page' }], ['a/voice/x.mp3', { from: '/x' }], ['player/kit.js', { from: '/k' }]]) };
  assert.deepEqual(filmFileFor(files, '/films/a/'), { text: 'page' });
  assert.deepEqual(filmFileFor(files, '/films/a'), { text: 'page' });
  assert.deepEqual(filmFileFor(files, '/films/a/voice/x.mp3'), { from: '/x' });
  assert.deepEqual(filmFileFor(files, '/films/player/kit.js'), { from: '/k' });
  for (const path of ['/films/', '/films/b/', '/films/a/../player/kit.js', '/workshop/', '/films']) assert.equal(filmFileFor(files, path), null, path);
});

test('every film of films/ makes a page', () => {
  const { ids, files: published } = filmFiles();
  assert.deepEqual(ids, filmIds());
  assert.ok(ids.length > 0);
  for (const id of ids) {
    const page = published.get(`${id}/index.html`).text;
    assert.equal(page.match(/<!--/g).length, 1, `${id}: only the licences' comment is left`);
  }
});

test('the hook the Vite configuration registers writes the films\' pages, and a film with a missing clip makes it fail', async () => {
  const config = (await import('../vite.config.ts')).default;
  const hook = config.plugins.flat().find((plugin) => plugin?.name === 'films');
  assert.ok(hook, 'vite.config.ts registers the films plugin');
  const { root, films } = made();
  try {
    const website = join(root, 'website');
    hook.configResolved({ root: website, build: { outDir: 'dist' } });
    const logged = [];
    await hook.writeBundle.call({ info: (text) => logged.push(text) });
    assert.deepEqual(readdirSync(join(website, 'dist', 'films')).sort(), ['player', 'tiny']);
    assert.match(readFileSync(join(website, 'dist', 'films', 'tiny', 'index.html'), 'utf8'), /<title>A &lt;tiny&gt; film<\/title>/);
    assert.deepEqual(logged, [`wrote 1 films into ${join(website, 'dist')}/films`]);
    unlinkSync(join(films, 'tiny', 'voice', 'b.mp3'));
    await assert.rejects(hook.writeBundle.call({ info() {} }), /^Error: films\/tiny\/voice\/b\.mp3: \(file\): is missing/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});

test('the fixed list of shared files is what a real film\'s page and the player\'s style ask for', () => {
  const { ids, files: published } = filmFiles();
  const shared = [...published.keys()].filter((path) => path.startsWith('player/')).map((path) => `/films/${path}`).sort();
  const page = published.get(`${ids[0]}/index.html`).text;
  const asked = new Set([...page.matchAll(/(?:src|href)="(\/films\/player\/[^"]+)"/g)].map((m) => m[1]));
  // The licence texts, named in the comment at the end of the page.
  for (const m of page.matchAll(/\/films\/player\/licences\/[a-z0-9.-]+\.txt/g)) asked.add(m[0]);
  // The fonts, which the kit's player.css asks for beside it.
  const css = readFileSync(published.get('player/player.css').from, 'utf8');
  for (const m of css.matchAll(/url\("([^"]+)"\)/g)) asked.add(new URL(m[1], 'http://site/films/player/player.css').pathname);
  assert.deepEqual([...asked].sort(), shared);
});
