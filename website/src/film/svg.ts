// The pieces the film is drawn with: SVG with the film's own classes, which all start with `f-`.

import type { Point } from './time';

const NS = 'http://www.w3.org/2000/svg';
type Attributes = Record<string, string | number | null | undefined>;
/** The canvas's size, in its units. */
export const CANVAS = { w: 988, h: 414 };
/** The content rectangle: every shot has this left, top and right. */
export const C = { l: 30, t: 26, r: 958, b: 356, w: 928, h: 330 };
/** main's width where it stands before its boundary, and the boundary itself. */
export const MAIN_W = 150;
export const BOUNDARY = 430;

export function E<K extends keyof SVGElementTagNameMap>(name: K, attrs: Attributes, parent?: Node | null, text?: string): SVGElementTagNameMap[K] {
  const el = document.createElementNS(NS, name);
  for (const [key, value] of Object.entries(attrs)) if (value != null) el.setAttribute(key, String(value));
  if (text != null) el.textContent = text;
  parent?.appendChild(el);
  return el;
}
export const G = (parent: Node, attrs: Attributes = {}) => E('g', attrs, parent);
export const T = (parent: Node, x: number, y: number, words: string, cls = 'f-tok', anchor?: 'middle' | 'end') =>
  E('text', { x, y, class: cls, 'text-anchor': anchor }, parent, words);
export const textWidth = (el: SVGTextContentElement) => el.getComputedTextLength();

const offsets = new WeakMap<Element, number>();
export function translate(el: SVGElement, x: number, y: number) {
  el.setAttribute('transform', `translate(${x.toFixed(2)} ${y.toFixed(2)})`);
  offsets.set(el, x);
}
/** How far to the side `translate` last put `el`. */
export const offsetOf = (el: Element) => offsets.get(el) ?? 0;
/** Hidden when it cannot be seen. Never "visible": that would show a child of a hidden group. */
export function show(el: SVGElement, on: boolean) {
  el.style.visibility = on ? '' : 'hidden';
}

export type Mark = 'f' | 'r' | 'o';
/** The record's marks: a filled diamond is a fact, a hollow one a report, a half-filled one an opinion. */
export function diamond(parent: Node, x: number, y: number, s: number, kind: Mark, cls?: string | null): SVGElement {
  const d = `M${x} ${y - s}L${x + s} ${y}L${x} ${y + s}L${x - s} ${y}Z`;
  if (kind === 'f') return E('path', { d, class: cls || 'f-ink' }, parent);
  if (kind === 'r') return E('path', { d, class: 'f-dia-o' }, parent);
  const g = G(parent);
  E('path', { d, class: 'f-dia-o' }, g);
  E('path', { d: `M${x} ${y - s}L${x} ${y + s}L${x - s} ${y}Z`, class: cls || 'f-ink' }, g);
  return g;
}

interface CardParts {
  tok: string;
  mark?: Mark;
  detail?: string;
  cls?: string;
  detCls?: string;
  w?: number;
  h?: number;
}
/** A card: an optional mark, the token, and one detail after a hairline. */
export function card(parent: Node, x: number, y: number, o: CardParts) {
  const g = G(parent);
  const h = o.h || 36;
  const rect = E('rect', { x, y, height: h, rx: 3, class: 'f-card' }, g);
  let cx = x + 13;
  if (o.mark) {
    diamond(g, cx + 5, y + h / 2, 5.5, o.mark);
    cx += 19;
  }
  const tok = T(g, cx, y + h / 2 + 5, o.tok, o.cls || 'f-tok');
  cx += textWidth(tok);
  if (o.detail) {
    E('line', { x1: cx + 12.5, y1: y + 9, x2: cx + 12.5, y2: y + h - 9, class: 'f-hair' }, g);
    const detail = T(g, cx + 25, y + h / 2 + 4.5, o.detail, o.detCls || 'f-lbl');
    cx += 25 + textWidth(detail);
  }
  const w = o.w || cx + 13 - x;
  rect.setAttribute('width', String(w));
  return { g, w, h, tok };
}

export interface Button {
  g: SVGGElement;
  x: number;
  y: number;
  w: number;
  h: number;
  label?: string;
}
/** A button at rest. `tip` is where the pointer lands: its lower right, clear of the words. */
export function button(parent: Node, x: number, y: number, label: string, o: { right?: boolean } = {}): Button & { tip: Point } {
  const g = G(parent);
  const rect = E('rect', { y, height: 32, rx: 3, class: 'f-btn' }, g);
  const text = T(g, 0, y + 21, label, 'f-name f-btn-t');
  const w = textWidth(text) + 28;
  const left = o.right ? x - w : x;
  rect.setAttribute('x', String(left));
  rect.setAttribute('width', String(w));
  text.setAttribute('x', String(left + 14));
  return { g, x: left, y, w, h: 32, label, tip: [left + w - 8, y + 27] };
}

/** A computer, or a place like one. */
export function box(parent: Node, x: number, y: number, w: number, h: number, name: string, o: { ghost?: boolean; sub?: string } = {}) {
  const g = G(parent);
  E('rect', { x, y, width: w, height: h, rx: 4, class: o.ghost ? 'f-box f-ghost' : 'f-box' }, g);
  const title = T(g, x + 18, y + 30, name, o.ghost ? 'f-name f-muted' : 'f-name');
  if (o.sub) T(g, x + 26 + textWidth(title), y + 30, o.sub, 'f-lbl');
  return g;
}

export const CURSOR = 'M0 0V18L4.8 13.6L8 21L11.2 19.6L8.1 12.4H14.4Z';
