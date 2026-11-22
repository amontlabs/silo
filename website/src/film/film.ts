// The home page's film: eleven shots of Silo's window in a loop. This is its clock and its controls. The frame
// at a moment comes from the moment alone, so a jump or a held frame shows what playing shows.

import { Shown } from './chrome';
import { drawFilm, type Drawing, type Layout } from './drawing';
import { CROSSFADE, END, LENGTH, STILL, TIMED, endOf, shotAt, type Key } from './story';
import { clamp, easeInOut, visible } from './time';

/** The widths that choose the layout. film.css has the same two. */
export const PHONE = '(max-width: 999.98px)';
export const NARROW = '(max-width: 1064px)';
const layoutNow = (): Layout => (matchMedia(PHONE).matches ? 'phone' : matchMedia(NARROW).matches ? 'narrow' : 'window');

export interface FilmOptions {
  /** Reduced motion as it opens: it opens on its still, plays only when asked and jumps at once. */
  reduced: boolean;
  /** Whether it is in view as it mounts. Out of view it waits on its still, then plays from its start. */
  inView: boolean;
  /** Holds the frame at this moment, with no play, for checks. */
  hold?: number | null;
  /** The visitor pressed Play or chose a shot. */
  onTake?: () => void;
  /** It was paused, or a jump has crossed over. */
  onChange?: () => void;
}
export interface Film {
  /** True while it plays. */
  readonly running: boolean;
  /** True while anything in it moves: it plays, or a jump crosses over. */
  readonly moving: boolean;
  /** Says whether the page lets it play. */
  allow(may: boolean): void;
  /** Ends a jump that crosses over, at once. */
  land(): void;
  /** Says whether motion is reduced. As it becomes so, the film pauses on the end of the shot it is in. */
  reduce(on: boolean): void;
  /** Stops it on the frame at `t`. */
  hold(t: number): void;
}

declare global {
  interface HTMLElement {
    /** On the film's root, for the checks that hold its frames. */
    film?: Film;
  }
}

export function mountFilm(root: HTMLElement, options: FilmOptions): Film {
  const figure = root.querySelector('figure')!;
  const progress = root.querySelector<HTMLElement>('.film-progress')!;
  const pause = root.querySelector<HTMLButtonElement>('.film-pause')!;
  const { onTake, onChange } = options;

  let reduced = options.reduced;
  let held = options.hold != null;
  let paused = reduced;
  let allowed = false;
  // Until it has first played, a film that waits shows its still and starts from the beginning.
  let waiting = !reduced && !options.inView;
  let clock = options.hold ?? (reduced || waiting ? STILL : 0);
  let last: number | null = null;
  let frameId = 0;
  let shown = new Shown();
  let crossing: { ghost: SVGGElement; t0: number } | null = null;
  let drawing: Drawing;
  let layout: Layout;
  let width = 0;

  const bars = TIMED.map((shot, i) => {
    const segment = document.createElement('button');
    segment.type = 'button';
    segment.style.setProperty('--w', String(shot.dur));
    segment.setAttribute('aria-label', `${i + 1}. ${shot.name}`);
    segment.title = shot.name;
    segment.addEventListener('click', () => jump(i));
    const bar = document.createElement('i');
    segment.append(bar);
    progress.append(segment);
    return { shot, bar };
  });

  function render() {
    shown.reduced = reduced;
    drawing.render(clock, shown);
    if (crossing) crossing.ghost.style.opacity = String(1 - easeInOut(shown.jump!.u));
    const played = visible(clock, null, END)[0];
    for (const { shot, bar } of bars) {
      bar.style.transform = `scaleX(${clamp((clock - shot.start) / shot.dur).toFixed(4)})`;
      bar.style.opacity = String(played);
    }
  }
  function land() {
    crossing?.ghost.remove();
    crossing = null;
    shown.jump = null;
  }
  const running = () => allowed && !paused && !held;
  /** Asks for a frame only while something moves, so a held film runs nothing. */
  function wake() {
    if (frameId || (!running() && !crossing)) return;
    frameId = requestAnimationFrame(frame);
  }
  function rest() {
    if (running() || crossing || !frameId) return;
    cancelAnimationFrame(frameId);
    frameId = 0;
  }
  function frame(now: number) {
    frameId = 0;
    if (running()) {
      if (last !== null) clock = (clock + (now - last)) % LENGTH;
      last = now;
    } else {
      last = null;
    }
    if (crossing) shown.jump!.u = clamp((performance.now() - crossing.t0) / CROSSFADE);
    render();
    if (crossing && shown.jump!.u === 1) {
      land();
      onChange?.();
    }
    wake();
  }

  /**
   * Playing, a jump plays the shot from its start. Paused, it holds the shot's end. The canvas crosses over, unless
   * motion is reduced or the film is held: then it is there at once.
   */
  function jump(i: number) {
    const shot = TIMED[i];
    if (!held && !reduced) {
      crossing?.ghost.remove();
      shown.start();
      crossing = { ghost: drawing.ghost(), t0: performance.now() };
    }
    waiting = false;
    clock = held || paused ? endOf(shot) : shot.start;
    last = null;
    render();
    onTake?.();
    wake();
  }
  const go = (key: Key) => jump(TIMED.findIndex((shot) => shot.key === key));

  function build() {
    const crossed = crossing !== null;
    land();
    shown = new Shown();
    layout = layoutNow();
    width = figure.clientWidth;
    drawing = drawFilm(figure, layout, go);
    render();
    if (crossed) onChange?.();
  }

  const film: Film = {
    get running() {
      return running();
    },
    get moving() {
      return running() || crossing !== null;
    },
    allow(may) {
      allowed = may;
      if (running() && waiting) {
        waiting = false;
        clock = 0;
        render();
      }
      if (!running()) last = null;
      rest();
      wake();
    },
    land() {
      if (!crossing) return;
      land();
      render();
      rest();
    },
    reduce(on) {
      reduced = on;
      if (on) {
        land();
        if (!held) {
          paused = true;
          waiting = false;
          clock = endOf(shotAt(clock));
          last = null;
          name();
        }
      }
      render();
      rest();
    },
    hold(t) {
      held = true;
      land();
      clock = t;
      last = null;
      render();
      rest();
    },
  };

  const name = () => {
    pause.textContent = paused ? 'Play' : 'Pause';
  };
  pause.hidden = held;
  pause.addEventListener('click', () => {
    paused = !paused;
    last = null;
    name();
    if (paused) onChange?.();
    else onTake?.();
    rest();
    wake();
  });
  // A layout measures its words, so another one is drawn again. A phone's also follows its width.
  addEventListener('resize', () => {
    if (layoutNow() !== layout || (layout === 'phone' && figure.clientWidth !== width)) build();
  });

  name();
  root.classList.add('live');
  build();
  root.film = film;
  return film;
}
