// A reel is one drawing of the film: the shots are built once, with cues on one clock, and a cue only sets what
// is painted (opacity, position, how much of a line is drawn, text). So nothing changes layout while it plays,
// and the frame at a moment is the same whether playing or held.

import { END, FADE, PRESSED, out, shotOf, type Key, type Span } from './story';
import { CURSOR, E, G, T, show, translate, type Button, type Mark } from './svg';
import { along, easeInOut, part, visible, type Leg, type Point } from './time';

/** The sidebar's rows: a place or a computer, its height and the shot a press on it plays. */
export const ROWS = [
  ['Needs you', 56, 'needs'], ['Now', 82, 'now'], ['Timeline', 108, 'timeline'], ['Network', 134, 'network'], ['Disk', 160, 'disk'],
  ['main', 206, 'main'], ['fork', 232, 'fork'], ['macOS', 232, 'tests'], ['Linux', 258, 'tests'], ['request check', 232, 'ext'],
  ['Bin', 456, 'fork'],
] as const satisfies readonly (readonly [string, number, Key])[];
export type Row = (typeof ROWS)[number][0];

export interface Recorded {
  t: number;
  kind: Mark;
  words: string;
  accent: boolean;
}
/** A stretch of time in which something moves. */
export interface Motion {
  t: number;
  d: number;
  /** What moves, where a pan follows it or goes on beside it. What moves as one thing has one name. */
  of?: string;
  /** The part of the canvas it keeps to, where that is how it stays out of the camera's view. */
  across?: Span;
}
interface Fade {
  /** How far it rises as it arrives. */
  dy?: number;
  d?: number;
  dout?: number;
}

export class Reel {
  readonly cues: ((t: number) => void)[] = [];
  /** When each line reaches the record. */
  readonly record: Recorded[] = [];
  /** When the open place or computer changes, and to which. */
  readonly lit: [t: number, row: Row | null][] = [];
  /** When a computer is in the sidebar. */
  readonly present: Partial<Record<Row, [from: number, to: number]>> = {};
  badge: [from: number, to: number] = [0, 0];
  bin = Infinity;
  /** Every stretch of time in which something moves. */
  readonly motions: Motion[] = [];
  readonly presses: { t: number; x: number; y: number; w: number; h: number }[] = [];
  readonly pointers: { moves: readonly Leg[]; start: Point; a: number; b: number | null }[] = [];
  /** The whole of each text that types itself. */
  readonly typed = new Map<SVGElement, string>();
  private readonly faded = new WeakSet<SVGElement>();

  /** On a phone what arrives fades where it is, since there the camera is the one thing that may move. */
  constructor(readonly canvas: SVGGElement, readonly phone = false) {}

  cue(run: (t: number) => void) {
    this.cues.push(run);
  }
  during(t: number, d: number, of?: string, across?: Span) {
    this.motions.push({ t, d, of, across });
  }
  /** Arrives at `a` (fades in and rises), leaves at `b` (fades where it is). */
  fade<El extends SVGElement>(el: El, a: number | null, b: number | null = null, o: Fade = {}): El {
    const dy = o.dy ?? 6;
    this.faded.add(el);
    this.cue((t) => {
      const [opacity, rise] = visible(t, a, b, o.d || FADE, o.dout || FADE);
      el.style.opacity = opacity.toFixed(3);
      show(el, opacity >= 0.003);
      if (dy) translate(el, 0, this.phone ? 0 : rise * dy);
    });
    return el;
  }
  /** All of a shot is one group, so it leaves as one picture and no line shows through what covers it. */
  scene(key: Key) {
    return this.fade(G(this.canvas, { 'data-shot': key }), shotOf(key).start, out(key), { dy: 0 });
  }
  /** Moves along eased legs, as a translation from where it was drawn. */
  move<El extends SVGElement>(el: El, legs: readonly Leg[], start: Point = [0, 0], of?: string): El {
    for (const leg of legs) this.during(leg.t, leg.d, of);
    this.cue((t) => {
      const [x, y] = along(t, start, legs);
      translate(el, x, y);
    });
    return el;
  }
  /** A path drawn from its start over `d` ms. */
  draw<El extends SVGPathElement>(el: El, a: number, d: number, of?: string): El {
    this.during(a, d, of);
    let length = 0;
    this.cue((t) => {
      length ||= el.getTotalLength();
      const u = easeInOut(part(t, a, d));
      el.style.strokeDasharray = u < 1 ? `${length} ${length}` : '';
      el.style.strokeDashoffset = (length * (1 - u)).toFixed(2);
      show(el, u > 0.001);
    });
    return el;
  }
  /** A straight stroke that grows from its start. `reach` says how much of it there is. */
  stroke(parent: Node, from: Point, to: Point, cls: string, reach: (t: number) => number) {
    const across = from[1] === to[1];
    const line = E('line', { x1: 0, y1: 0, x2: to[0] - from[0], y2: to[1] - from[1], class: cls }, parent);
    this.cue((t) => {
      const u = reach(t).toFixed(4);
      line.setAttribute('transform', `translate(${from[0]} ${from[1]}) scale(${across ? `${u} 1` : `1 ${u}`})`);
      show(line, Number(u) > 0.001);
    });
    return line;
  }
  eased(a: number, d: number, of?: string, across?: Span) {
    this.during(a, d, of, across);
    return (t: number) => easeInOut(part(t, a, d));
  }
  /** A command types itself from `a`, one character each `ms`. */
  typeText(el: SVGTextElement, words: string, a: number, ms: number) {
    this.during(a, (words.length - 1) * ms);
    this.typed.set(el, words);
    this.cue((t) => {
      el.textContent = words.slice(0, Math.max(0, Math.min(words.length, Math.floor((t - a) / ms) + 1)));
      show(el, t >= a);
    });
    return el;
  }
  /** A name that has done its job goes to the muted ink, in its place, and stays to the end of its shot. */
  mute(el: SVGElement, from: number) {
    const copy = el.cloneNode(true) as SVGElement;
    for (const words of [copy, ...copy.querySelectorAll<SVGElement>('tspan')]) {
      words.classList.add('f-muted');
      words.classList.remove('f-acc-t');
    }
    copy.style.fill = '';
    if (copy.tagName === 'path') copy.style.stroke = 'var(--wire)';
    el.after(copy);
    const typed = this.typed.get(el);
    if (typed) this.typed.set(copy, typed);
    this.cue((t) => {
      const base = this.faded.has(el) ? Number(el.style.opacity) : 1;
      const u = easeInOut(part(t, from, FADE));
      el.style.opacity = (base * (1 - u)).toFixed(3);
      copy.style.opacity = (base * u).toFixed(3);
      copy.style.visibility = el.style.visibility;
      const transform = el.getAttribute('transform');
      if (transform) copy.setAttribute('transform', transform);
      if (copy.tagName === 'text' && !copy.querySelector('tspan')) copy.textContent = el.textContent;
    });
    return copy;
  }
  /** A text that has done its job becomes a bar of its width, once the word is gone: a bar over a word strikes it out. */
  recede(el: SVGTextElement, from: number) {
    const b = el.getBBox();
    this.fade(el, null, from, { dy: 0 });
    const bar = E('rect', { x: b.x, y: b.y + b.height / 2 - 2.5, width: b.width, height: 5, rx: 2.5, class: 'f-bar' }, el.parentNode);
    return this.fade(bar, from + FADE, null, { dy: 0 });
  }
  /** The one press: down and released. It switches without a fade: halfway, its label would be grey on grey. */
  press(b: Button, t: number, fill = 'f-btn f-primary') {
    this.pressed(t, b);
    const on = this.fade(G(b.g), t, t + PRESSED - 120, { dy: 0, d: 1, dout: 1 });
    E('rect', { x: b.x, y: b.y, width: b.w, height: b.h, rx: 3, class: fill }, on);
    if (b.label) T(on, b.x + 14, b.y + 21, b.label, 'f-name f-btn-t f-on-ink');
  }
  pressed(t: number, { x, y, w, h }: { x: number; y: number; w: number; h: number }) {
    this.presses.push({ t, x, y, w, h });
  }
  pointer(parent: Node, moves: readonly Leg[], start: Point, a: number, b: number | null) {
    this.pointers.push({ moves, start, a, b });
    const outer = this.fade(G(parent), a, b, { dy: 0 });
    const inner = G(outer);
    E('path', { d: CURSOR, class: 'f-cursor' }, inner);
    this.move(inner, moves, start, 'the pointer');
    return outer;
  }
  rec(t: number, kind: Mark, words: string, accent = false) {
    this.record.push({ t, kind, words, accent });
  }
  /** The mark leads what it marks by one move, at a cut and inside a shot. */
  litFor(t: number, row: Row) {
    this.lit.push([t - FADE, row]);
  }
  litShot(key: Key, row: Row) {
    this.litFor(shotOf(key).start, row);
  }
  /** Puts the record and the sidebar's changes in order, once every shot is built. */
  finish() {
    this.lit.push([END, null]);
    this.lit.sort((a, b) => a[0] - b[0]);
    // A shot that opens on the row already marked does not move the mark.
    for (let i = this.lit.length - 1; i > 0; i--) if (this.lit[i][1] === this.lit[i - 1][1]) this.lit.splice(i, 1);
    this.record.sort((a, b) => a.t - b.t);
  }
}
