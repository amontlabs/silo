// The home page's script: it starts the pictures once the faces the film measures its words in are here.
//
// `?hold=<ms>` holds the film at a moment of its loop, and `?shot=<n>` at the end of its nth shot.

import { TIMED, endOf } from './film/story';
import { mountPictures } from './pictures';

const FACES = ["15px 'IBM Plex Mono'", "500 15px 'IBM Plex Sans'", "13px 'IBM Plex Sans'"];

function heldAt(query: URLSearchParams): number | null {
  const shot = TIMED[Number(query.get('shot')) - 1];
  if (shot) return endOf(shot);
  const hold = query.get('hold');
  return hold && Number.isFinite(Number(hold)) ? Number(hold) : null;
}

const start = () => mountPictures({ hold: heldAt(new URLSearchParams(location.search)) });
// Without its faces the film still draws, measured in the fonts it got.
Promise.all(FACES.map((face) => document.fonts.load(face))).then(start, start);
