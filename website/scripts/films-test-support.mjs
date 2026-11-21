// A made-up films/ folder for the tests of the films' pages (films.test.mjs, src/workshop/data.test.ts).
import { copyFileSync, mkdirSync, mkdtempSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { defaultFilmsDir } from './films.mjs';

export const VENDOR = ['gsap.min.js', 'MotionPathPlugin.min.js', 'DrawSVGPlugin.min.js', 'MorphSVGPlugin.min.js', 'CustomEase.min.js', 'icons.js'];

/** A films/ folder with the real page.html, stand-ins for the player and the kit, one small film and a folder that is not a film. */
// `template` is films/player/page.html of the repository, given by a test whose import.meta.url is not a file's.
export function made(root = mkdtempSync(join(tmpdir(), 'films-')), template = join(defaultFilmsDir(), 'player', 'page.html')) {
  const films = join(root, 'films');
  const put = (path, text) => { mkdirSync(join(films, path, '..'), { recursive: true }); writeFileSync(join(films, path), text); };
  mkdirSync(join(films, 'player'), { recursive: true });
  copyFileSync(template, join(films, 'player', 'page.html'));
  for (const name of ['voice.js', 'kit.js', 'page.css', 'page.js']) put(`player/${name}`, `/* ${name} */`);
  put('kit/kit/player.css', '/* player.css */');
  for (const name of VENDOR) put(`kit/kit/vendor/${name}`, `/* ${name} */`);
  for (const name of ['geist-regular.otf', 'geist-medium.otf', 'geist-mono.ttf']) put(`kit/kit/fonts/${name}`, name);
  put('kit/licences/geist.txt', 'OFL');
  put('kit/licences/lucide.txt', 'ISC');
  put('tools/build.py', '# not a film');
  put('tiny/film.json', JSON.stringify({ id: 'tiny', kind: 'teaches', title: 'A <tiny> film', line: 'Line.', shows: ['core'] }));
  put('tiny/explainer.js', 'window.TINY = "</script>";');
  put('tiny/narration.json', '{}');
  put('tiny/voice/a.mp3', 'clip a');
  put('tiny/voice/b.mp3', 'clip b');
  put('tiny/voice/unused.mp3', 'a clip voice.json no longer names');
  put('tiny/voice.json', JSON.stringify({
    engine: 'kokoro', voices: [{ id: 'af_heart', label: 'Heart', scenes: [
      { sentences: [{ text: 'One </script> two.', file: 'voice/a.mp3', start: 0.4, length: 1 }], total: 2 },
      { sentences: [{ text: 'Three.', file: 'voice/b.mp3', start: 0.4, length: 1 }], total: 2 },
    ] }],
  }));
  return { root, films, put };
}

