// A scene is a piece of the page and a list of cues, each a change made at a given moment. A scene can be
// shown as it is at any moment, or played on from one. The small pictures of the home page's cards are scenes.

export type Part = HTMLElement | SVGElement;
/** Finds parts of the scene. */
export type Query = (selector: string) => Part[];
/**
 * One change to the scene. `instant` is true when the scene is being put at a later moment in one go: a change
 * that only makes sense while playing, like a dot travelling, does nothing then.
 */
export type Run = (q: Query, instant: boolean) => void;
export type Cue = readonly [when: number, run: Run];

export interface Scene {
  /** How long the scene lasts, in milliseconds. */
  readonly end: number;
  /** Shows the scene as it is at `t`, its end by default, and leaves it there. */
  hold(t?: number): void;
  /** Shows the scene as it is at `from`, then plays on. */
  play(from?: number): void;
}

/**
 * Makes a scene of `el`. Each time it is held or played, `el` gets back the markup it had here, so no cue has
 * to be undone and nothing kept on an element inside `el` survives. `layout` runs on that fresh markup, before
 * any cue, for what has to be measured. A cue at moment 0 says where a part starts: it is always applied at
 * once, so a pointer is at its place before it shows instead of sliding there from the corner.
 */
export function scene(el: Element, cues: readonly Cue[], end: number, layout?: () => void): Scene {
  const initial = el.innerHTML;
  let timers: number[] = [];
  const q: Query = (selector) => [...el.querySelectorAll<Part>(selector)];
  const set = (t: number) => {
    timers.forEach(clearTimeout);
    timers = [];
    // Everything up to `t` is applied at once: `jump` switches transitions off until the result is laid out.
    el.classList.add('jump');
    el.innerHTML = initial;
    layout?.();
    for (const [when, run] of cues) if (when < t || when === 0) run(q, true);
    void el.getBoundingClientRect();
    el.classList.remove('jump');
  };
  return {
    end,
    hold: (t = end) => set(t),
    play: (from = 0) => {
      set(from);
      for (const [when, run] of cues) if (when >= from && when > 0) timers.push(window.setTimeout(() => run(q, false), when - from));
    },
  };
}

export const add =
  (selector: string, name = 'on'): Run =>
  (q) =>
    q(selector).forEach((part) => part.classList.add(name));

export const rem =
  (selector: string, name = 'on'): Run =>
  (q) =>
    q(selector).forEach((part) => part.classList.remove(name));

/**
 * Sends `dot` along `points` in `ms`, at a constant speed, and hides it on arrival. With `real`, the dot
 * changes once its first leg is done: that is where a request picks the real secret up.
 */
export function follow(dot: Part, points: readonly (readonly number[])[], ms: number, real: boolean) {
  const lengths = points.slice(1).map((point, i) => Math.hypot(point[0] - points[i][0], point[1] - points[i][1]));
  const total = lengths.reduce((sum, length) => sum + length, 0);
  let done = 0;
  const frames = points.map((point, i) => {
    if (i) done += lengths[i - 1];
    return { transform: `translate(${point[0]}px, ${point[1]}px)`, offset: done / total };
  });
  dot.classList.add('on');
  dot.animate(frames, { duration: ms, easing: 'linear', fill: 'forwards' });
  if (real) window.setTimeout(() => dot.classList.add('real'), (ms * lengths[0]) / total);
  window.setTimeout(() => dot.classList.remove('on'), ms);
}
