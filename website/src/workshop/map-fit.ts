// Whether each part's title fits its box on the map, computed at build time from the face the stylesheet gives the
// title (workshop.css, `.ws-p-ttl`: IBM Plex Serif 400). Only the build and the tests import it: the page never does.
import type { Box, MapLayout } from './map.ts';

/**
 * The advance widths of IBM Plex Serif 400, in thousandths of an em: printable ASCII and a few typographic marks.
 * Made with `node website/scripts/font-widths.mjs website/public/fonts/ibm-plex-serif-400-normal.woff2`, which reads
 * the font file's cmap and hmtx tables. Kerning is left out: in these titles it only narrows a line.
 */
export const SERIF_WIDTHS: Record<string, number> = {
  0: 600, 1: 600, 2: 600, 3: 600, 4: 600, 5: 600, 6: 600, 7: 600, 8: 600, 9: 600, ' ': 232, '!': 280, '"': 415, '#': 695,
  $: 592, '%': 913, '&': 710, "'": 241, '(': 329, ')': 329, '*': 442, '+': 600, ',': 268, '-': 393, '.': 268, '/': 366,
  ':': 288, ';': 288, '<': 600, '=': 600, '>': 600, '?': 487, '@': 923, A: 686, B: 670, C: 649, D: 706, E: 641, F: 626,
  G: 721, H: 787, I: 346, J: 457, K: 707, L: 594, M: 872, N: 778, O: 716, P: 625, Q: 716, R: 672, S: 588, T: 652, U: 735,
  V: 664, W: 968, X: 669, Y: 634, Z: 637, '[': 284, '\\': 365, ']': 284, '^': 600, _: 564, '`': 600, a: 546, b: 598, c: 516,
  d: 608, e: 536, f: 342, g: 544, h: 624, i: 315, j: 303, k: 584, l: 300, m: 953, n: 639, o: 564, p: 613, q: 598, r: 443,
  s: 488, t: 353, u: 619, v: 534, w: 777, x: 561, y: 534, z: 508, '{': 327, '|': 298, '}': 327, '~': 600, '–': 587, '—': 783,
  '‘': 268, '’': 268, '“': 462, '”': 462, '…': 805,
};

/** IBM Plex Mono 400, the state line's face, gives every character the same advance (the same script, on its file). */
export const MONO_ADVANCE = 600;

/**
 * The map's width in pixels at the two ends of the windows where it shows. It shows from a 1200 px window up; the page
 * frame is at most 1136 px wide with a 1 px line on each side, and below a 1220 px window it keeps 40 px on each side.
 */
export const MAP_PIXELS = { narrowest: 1200 - 2 * 40 - 2, widest: 1136 - 2 } as const;

/** The paper a line keeps between its end and the box's right border, in map units: under 3 the end touches the border. */
export const RIGHT_ROOM = 3;

/** What the stylesheet gives a part's title on each map: its left offset and its size, in map units, at `unit` pixels per map unit. */
function titleStyle(view: 'full' | 'fold', inner: boolean, unit: number): { left: number; size: number } {
  // `max(12px, N * u)`: below 12 px the size stays at 12 px, which is more map units on a narrower map.
  const min = 12 / unit;
  if (view === 'full') return inner ? { left: 11, size: Math.max(min, 14) } : { left: 15, size: Math.max(min, 15) };
  return inner ? { left: 8, size: Math.max(min, 12) } : { left: 10, size: Math.max(min, 12.5) };
}

/** The characters of `text` that the width table lacks. */
export const unknownCharacters = (text: string) => [...new Set([...text].filter((char) => SERIF_WIDTHS[char] === undefined))];

/** The width of a title in the title's face, in units of its size. */
export const titleEms = (text: string) => [...text].reduce((sum, char) => sum + (SERIF_WIDTHS[char] ?? 0), 0) / 1000;

/** The width of a box a line needs: the offset before it, the line, and the room after it. */
const needed = (left: number, width: number) => Math.ceil(left + width + RIGHT_ROOM);

/** Each title that does not fit its box, on either map, at either end of the map's widths: one message each. */
export function titleProblems(parts: { id: string; title: string }[], layout: MapLayout, where: string): string[] {
  const problems: string[] = [];
  for (const part of parts) {
    const place = layout.places[part.id];
    if (!place) continue;
    const unknown = unknownCharacters(part.title);
    if (unknown.length) {
      problems.push(`The title of the part "${part.id}", "${part.title}", has ${unknown.map((char) => `"${char}"`).join(', ')}, which the width table of website/src/workshop/map-fit.ts lacks, so the build cannot tell whether it fits its box. Fix: add the character with website/scripts/font-widths.mjs, or use another one in plan/parts.json.`);
      continue;
    }
    for (const view of ['full', 'fold'] as const) {
      const box: Box = place[view];
      // The narrower map needs the most units when the 12 px floor applies, the wider one otherwise: both are read.
      const need = Math.max(...Object.values(MAP_PIXELS).map((px) => {
        const { left, size } = titleStyle(view, Boolean(place.inner), px / 1136);
        return needed(left, titleEms(part.title) * size);
      }));
      if (need > box[2]) {
        problems.push(`${where}: the title of the part "${part.id}", "${part.title}", needs a box ${need} map units wide on the ${view === 'full' ? 'full map' : 'folded map'}, and its box there is ${box[2]}. Fix: widen its box in map.ts, or shorten the title in plan/parts.json.`);
      }
    }
  }
  return problems;
}

/** The width of a box that the state line `words` needs on the full map, at `px` pixels of map width. */
export function stateNeeds(words: string, place: { inner?: boolean; wide?: boolean }, px: number): number {
  const unit = px / 1136;
  // workshop.css, `.ws-p-st`: its offset, a mark of 8 units (7 px at least), a gap of 6, then `max(12px, 11.5u)` mono at -0.02em.
  const left = place.wide ? 138 : place.inner ? 11 : 15;
  const size = Math.max(12 / unit, 11.5);
  const mark = Math.max(8, 7 / unit);
  return needed(left + mark + 6, [...words].length * (MONO_ADVANCE / 1000 - 0.02) * size);
}
