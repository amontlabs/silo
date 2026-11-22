// What a moment makes of a move or a fade. Nothing here keeps a state.

import { FADE, REACT } from './story';

export type Point = readonly [x: number, y: number];
/** An eased move to `to`, bent toward `c` when it has one. */
export interface Leg {
  t: number;
  d: number;
  to: Point;
  c?: Point;
}

export const clamp = (u: number) => (u < 0 ? 0 : u > 1 ? 1 : u);
/** How much of the `d` ms from `a` has passed at `t`. */
export const part = (t: number, a: number, d: number) => clamp((t - a) / d);
export const easeOut = (u: number) => 1 - Math.pow(1 - u, 3);
export const easeInOut = (u: number) => (u < 0.5 ? 4 * u * u * u : 1 - Math.pow(-2 * u + 2, 3) / 2);
export const lerp = (a: number, b: number, u: number) => a + (b - a) * u;

/** The opacity of what arrives at `a` and leaves at `b`, and how far it still has to rise. */
export function visible(t: number, a: number | null, b: number | null, d = FADE, dout = FADE): [opacity: number, rise: number] {
  const i = a == null ? 1 : easeOut(part(t, a, d));
  const o = b == null ? 0 : easeInOut(part(t, b, dout));
  return [i * (1 - o), 1 - i];
}

/** Where what starts at `start` and moves along `legs` stands at `t`. */
export function along(t: number, start: Point, legs: readonly Leg[]): Point {
  let p = start;
  for (const leg of legs) {
    if (t <= leg.t) return p;
    const u = easeInOut(part(t, leg.t, leg.d));
    const c = leg.c ?? [(p[0] + leg.to[0]) / 2, (p[1] + leg.to[1]) / 2];
    if (u < 1) {
      return [
        (1 - u) * (1 - u) * p[0] + 2 * (1 - u) * u * c[0] + u * u * leg.to[0],
        (1 - u) * (1 - u) * p[1] + 2 * (1 - u) * u * c[1] + u * u * leg.to[1],
      ];
    }
    p = leg.to;
  }
  return p;
}

/** A press comes once the pointer that moved along this leg has landed. */
export const pressAfter = (leg: Leg) => leg.t + leg.d + REACT;
