// The phone's camera: which part of the canvas shows at a moment.

import { TIMED, type Span } from './story';
import { CANVAS, offsetOf } from './svg';
import { clamp, easeInOut, lerp, part } from './time';

/** The camera is never narrower than this, so the film's smallest phone text stays 11 px at 390 px wide. */
export const VIEW = 480;
/** A word fades over this much as the edge of the view reaches it, so none is ever read cut. */
const EDGE = 2;

interface Leg {
  t: number;
  d: number;
  to: Span;
}
/** Every move of the camera on the film's clock. A cut is a move of no length. */
const LEGS: readonly Leg[] = TIMED.flatMap(({ start, camera }) => [
  ...(camera.from ? [{ t: start, d: 0, to: camera.from }] : []),
  ...camera.pans.map(({ t, d, to }) => ({ t: start + t, d, to })),
]);

/** The left of a view `width` wide at `t`: centred on what is framed, and kept on the canvas. */
export function viewAt(t: number, width = VIEW) {
  let middle = VIEW / 2;
  for (const { t: from, d, to } of LEGS) {
    if (t < from) break;
    middle = lerp(middle, (to[0] + to[1]) / 2, d ? easeInOut(part(t, from, d)) : 1);
    if (t < from + d) break;
  }
  return Math.min(Math.max(middle - width / 2, 0), Math.max(CANVAS.w - width, 0));
}

/**
 * Measures the canvas's words once, and gives a function that fades each one out where the view would cut it.
 * It writes fill-opacity, so what the shots write as opacity stands.
 */
export function wordsInView(canvas: SVGGElement, typed: ReadonlyMap<SVGElement, string>) {
  const words = [...canvas.querySelectorAll<SVGTextElement>('text')].map((el) => {
    // A text that types itself takes the room it will have, so it never goes as it grows.
    const whole = typed.get(el);
    const now = el.textContent;
    if (whole !== undefined) el.textContent = whole;
    const box = el.getBBox();
    // A text's box can hold room beside its letters. The advance of its characters is what is read.
    const width = el.firstElementChild ? box.width : el.getComputedTextLength();
    const x = box.x + (box.width - width) / 2;
    // Only a typed text is written back: writing one with tspans would drop their spacing.
    if (whole !== undefined) el.textContent = now;
    const path: Element[] = [];
    for (let up: Element | null = el; up && up !== canvas; up = up.parentElement) path.push(up);
    return { el, x, width, path };
  });
  return (left: number, width: number) => {
    for (const word of words) {
      let x = word.x;
      for (const el of word.path) x += offsetOf(el);
      const shown = clamp(Math.min(x - left, left + width - x - word.width) / EDGE);
      word.el.style.fillOpacity = shown === 1 ? '' : shown.toFixed(3);
    }
  };
}
