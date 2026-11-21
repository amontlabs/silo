// Whether each part's title fits its box on the map, computed at build time from the face the stylesheet gives the
// title (workshop.css, `.ws-p-ttl`: IBM Plex Serif 400). Only the build and the tests import it: the page never does.
import type { Box, MapLayout, Place } from './map.ts';

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
 * The map's width in pixels at the two ends of the windows where it shows. It shows from a 1200 px window up, less a
 * scroll bar where the system draws one (up to 17 px); the page frame is at most 1136 px wide with a 1 px line on each
 * side, and below a 1220 px window it keeps 40 px on each side.
 */
export const MAP_PIXELS = { narrowest: 1200 - 17 - 2 * 40 - 2, widest: 1136 - 2 } as const;

/** The paper a line keeps between its end and the box's right border, in map units: under 3 the end touches the border. */
export const RIGHT_ROOM = 3;

/** The face the width table was made from: the first family of `--serif` (site.css), and its file for weight 400. */
export const TITLE_FACE = { family: 'IBM Plex Serif', file: 'website/public/fonts/ibm-plex-serif-400-normal.woff2' } as const;

/**
 * What workshop.css gives a part's title, in map units: its left offset and its size on each map, the 12 px floor of
 * every size, its line height, its top on the full map (`--ty`, 11 by default) and in a tall box on the fold. A test
 * holds these equal to the stylesheet.
 */
export const TITLE_STYLE = {
  full: { left: 15, size: 15 },
  inner: { left: 11, size: 14 },
  fold: { left: 10, size: 12.5 },
  foldInner: { left: 8, size: 12 },
  floorPx: 12,
  lineHeight: 1.3,
  top: 11,
  foldTallTop: 8,
} as const;

/** What the stylesheet gives a part's title on one map: its left offset and its size, in map units, at `unit` pixels per map unit. */
function titleStyle(view: 'full' | 'fold', inner: boolean, unit: number): { left: number; size: number } {
  // `max(12px, N * u)`: below 12 px the size stays at 12 px, which is more map units on a narrower map.
  const style = view === 'full' ? (inner ? TITLE_STYLE.inner : TITLE_STYLE.full) : (inner ? TITLE_STYLE.foldInner : TITLE_STYLE.fold);
  return { left: style.left, size: Math.max(TITLE_STYLE.floorPx / unit, style.size) };
}

/** The top of a part's title from its box's top, in map units: `top` in workshop.css, `.ws-p-ttl` and its fold rules. */
function titleTop(view: 'full' | 'fold', place: Place, size: number, unit: number): number {
  if (view === 'full') return place.ty ?? TITLE_STYLE.top;
  if (place.tall) return TITLE_STYLE.foldTallTop;
  // Centred in the folded box: `(height - 2px - 1.3em) / 2`.
  return (place.fold[3] - 2 / unit - TITLE_STYLE.lineHeight * size) / 2;
}

/** A title as the page shows it: white space collapses to one space and none is kept at either end (`white-space: nowrap`). */
export const shownTitle = (title: string) => title.replace(/[ \t\n\f\r]+/g, ' ').trim();

/** The characters of a title, as the page shows it, that the width table lacks. */
export const unknownCharacters = (title: string) => [...new Set([...shownTitle(title)].filter((char) => !Object.hasOwn(SERIF_WIDTHS, char)))];

/** The width of a title as the page shows it, in its face, in units of its size. */
export const titleEms = (title: string) => [...shownTitle(title)].reduce((sum, char) => sum + (SERIF_WIDTHS[char] ?? 0), 0) / 1000;

/** The width of a box a line needs: the offset before it, the line, and the room after it. */
const needed = (left: number, width: number) => Math.ceil(left + width + RIGHT_ROOM);

const crosses = (a: Box, b: Box) => a[0] < b[0] + b[2] && b[0] < a[0] + a[2] && a[1] < b[1] + b[3] && b[1] < a[1] + a[3];

/**
 * Each title that does not fit its box, on either map, at either end of the map's widths: one message each. The room
 * of a title ends at its box's right border, or sooner where a part drawn inside the box (`inner`) crosses the
 * title's line: that part is drawn above it.
 */
export function titleProblems(parts: { id: string; title: string }[], layout: MapLayout, where: string): string[] {
  const problems: string[] = [];
  const inners = Object.entries(layout.places).filter(([, place]) => place.inner);
  for (const part of parts) {
    if (!Object.hasOwn(layout.places, part.id)) continue;
    const place = layout.places[part.id];
    const unknown = unknownCharacters(part.title);
    if (unknown.length) {
      const chars = unknown.join('');
      problems.push(`The title of the part "${part.id}", "${part.title}", has ${unknown.map((char) => `"${char}"`).join(', ')}, which the width table of website/src/workshop/map-fit.ts lacks, so the build cannot tell whether it fits its box. Fix: run \`node website/scripts/font-widths.mjs ${TITLE_FACE.file} '${chars}'\` and add the width it prints for each to SERIF_WIDTHS, or use another character in plan/parts.json.`);
      continue;
    }
    for (const view of ['full', 'fold'] as const) {
      const box: Box = place[view];
      let need = 0;
      // Under each part drawn inside this box that crosses the title's line: the room the title needs, and where that part begins.
      const under = new Map<string, { need: number; room: number }>();
      // The narrower map needs the most units when the 12 px floor applies, the wider one otherwise: both are read.
      for (const px of Object.values(MAP_PIXELS)) {
        const unit = px / 1136;
        const { left, size } = titleStyle(view, Boolean(place.inner), unit);
        const width = needed(left, titleEms(part.title) * size);
        need = Math.max(need, width);
        if (place.inner) continue;
        const line: Box = [box[0] + left, box[1] + titleTop(view, place, size, unit), box[2] - left, TITLE_STYLE.lineHeight * size];
        for (const [id, inner] of inners) {
          if (id === part.id || !crosses(line, inner[view])) continue;
          const room = inner[view][0] - box[0];
          if (width > room) under.set(id, { need: Math.max(width, under.get(id)?.need ?? 0), room });
        }
      }
      const map = view === 'full' ? 'full map' : 'folded map';
      if (need > box[2]) {
        problems.push(`${where}: the title of the part "${part.id}", "${part.title}", needs a box ${need} map units wide on the ${map}, and its box there is ${box[2]}. Fix: widen its box in ${where}, or shorten the title in plan/parts.json.`);
      }
      for (const [id, { need: width, room }] of under) {
        problems.push(`${where}: the title of the part "${part.id}", "${part.title}", runs under the part "${id}", drawn inside its box on the ${map}: the title needs ${width} map units from the box's left edge, and "${id}" begins ${room} units in. Fix: move "${id}" right or off the title's line in ${where}, or shorten the title in plan/parts.json.`);
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
