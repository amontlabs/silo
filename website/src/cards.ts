// The cards of the home page: each has a small picture that plays one gesture. One card plays at a time. The
// cards in view take turns, and a card the visitor points at or presses plays at once.

import { add, follow, rem, scene, type Cue, type Run, type Scene } from './scene';

const move =
  (selector: string, x: number, y: number): Run =>
  (q) =>
    q(selector).forEach((part) => (part.style.transform = `translate(${x}px, ${y}px)`));
/** A request leaves the computer along `points`. */
const fly =
  (selector: string, points: number[][], ms: number): Run =>
  (q, instant) => {
    if (!instant) follow(q(selector)[0], points, ms, false);
  };

/** Each card's picture, by its `data-mini`: how long it lasts, and its cues. Everything is drawn in a 320 by 140 box. */
const MINIS: Record<string, [end: number, cues: Cue[]]> = {
  // Three commands are typed in main, and each is done without a question.
  full: [
    3500,
    [
      [300, add('.l1')],
      [1050, add('.k1')],
      [1400, add('.l2')],
      [2250, add('.k2')],
      [2550, add('.l3')],
      [3000, add('.k3')],
    ],
  ],
  // An agent presses the button of a macOS screen, then the button of a Linux one.
  screen: [
    3500,
    [
      [0, move('.ac', 112, 88)],
      [200, add('.ac')],
      [200, move('.ac', 78.1, 82.9)],
      [1000, add('.b1', 'on')],
      [1150, add('.k1')],
      [1700, move('.ac', 237.1, 82.9)],
      [2500, add('.b2', 'on')],
      [2650, add('.k2')],
      [3100, rem('.ac')],
    ],
  ],
  // A request to an allowed host arrives. A request to a host no rule allows stops at the gate and is denied.
  edge: [
    3300,
    [
      [300, fly('.p1', [[114, 70.5], [172, 70.5], [172, 40.5], [191, 40.5]], 900)],
      [1200, add('.m1', 'got')],
      [2000, fly('.p2', [[114, 70.5], [140, 70.5]], 450)],
      [2450, add('.gate', 'lit')],
      [2550, add('.xm')],
      [2700, add('.dn')],
    ],
  ],
  // Five lines are written one after the other, then the legend says what the two marks mean.
  record: [
    3700,
    [
      [300, add('.l1')],
      [900, add('.l2')],
      [1500, add('.l3')],
      [2100, add('.l4')],
      [2700, add('.l5')],
      [3200, add('.lg')],
    ],
  ],
  // You press the first alert: the others give way and what led to it opens under it.
  needs: [
    3500,
    [
      [0, move('.yc', 244, 118)],
      [400, add('.yc')],
      [400, move('.yc', 257.6, 20.9)],
      [1200, add('.r1', 'lit')],
      [1500, add('.g2, .g3', 'low')],
      [1700, rem('.yc')],
      [1700, add('.ws')],
      [1900, add('.w1')],
      [2000, add('.c1')],
      [2400, add('.wd')],
      [2500, add('.c2')],
    ],
  ],
  // Checkpoints appear on main's line. You go back to the second: a fork starts from it, and main goes on.
  back: [
    4200,
    [
      [0, move('.yc', 232, 112)],
      [200, add('.s1')],
      [700, add('.s2')],
      [1200, add('.s3')],
      [1700, add('.s4')],
      [2200, add('.yc')],
      [2200, move('.yc', 114.1, 58.9)],
      [2800, add('.s2', 'hot')],
      [2900, add('.wb')],
      [3200, rem('.yc')],
      [3300, add('.fk')],
      [3700, add('.s5')],
    ],
  ],
};

/** How long a card stays on its last moment before its turn ends, the pause between two turns, and the pause after the pointer leaves. */
const LINGER = 600;
const GAP = 400;
const AFTER_POINTER = 900;

/** How long a picture dissolves into its next look and a card's line fades out. CSS reads it as `--fade`. */
const FADE = 250;

/** Properties a keyframe lists that are not styles. */
const NOT_STYLE = new Set(['offset', 'computedOffset', 'easing', 'composite']);

/**
 * A copy of `picture` as it looks now, laid over it and dissolving into whatever shows beneath. The copy keeps the
 * classes of the picture. What is halfway through a transition or an animation (a part fading in, a dot on its way)
 * gets its computed value written onto the copy, which itself runs nothing. It removes itself when its fade is over.
 */
function dissolve(card: HTMLElement, picture: Element) {
  const copy = picture.cloneNode(true) as Element;
  const live = [picture, ...picture.querySelectorAll('*')];
  const parts = [copy, ...copy.querySelectorAll('*')];
  for (const part of parts) part.removeAttribute('id');
  for (const animation of picture.getAnimations?.({ subtree: true }) ?? []) {
    const target = (animation.effect as KeyframeEffect | null)?.target;
    const part = target && parts[live.indexOf(target)];
    if (!part || !(part instanceof HTMLElement || part instanceof SVGElement)) continue;
    const now = getComputedStyle(target) as unknown as Record<string, string>;
    const style = part.style as unknown as Record<string, string>;
    for (const keyframe of (animation.effect as KeyframeEffect).getKeyframes()) {
      for (const property of Object.keys(keyframe)) if (!NOT_STYLE.has(property)) style[property] = now[property];
    }
  }
  const room = card.getBoundingClientRect();
  const box = picture.getBoundingClientRect();
  copy.classList.add('ghost', 'jump');
  copy.setAttribute('inert', '');
  const style = (copy as HTMLElement | SVGElement).style;
  style.left = `${box.left - room.left}px`;
  style.top = `${box.top - room.top}px`;
  style.width = `${box.width}px`;
  style.height = `${box.height}px`;
  // Right above the picture and under any copy still fading: what the visitor sees stays continuous.
  picture.after(copy);
  if (typeof copy.animate === 'function') copy.animate([{ opacity: 1 }, { opacity: 0 }], { duration: FADE, easing: 'linear', fill: 'forwards' });
  window.setTimeout(() => copy.remove(), FADE);
}

/** Tells `seen` each time a card comes into view or leaves it. */
export type Watch = (cards: Element[], seen: (index: number, inView: boolean) => void) => void;

/** A card counts as in view once half of it is. */
const watchInView: Watch = (cards, seen) => {
  const observer = new IntersectionObserver(
    (entries) => {
      for (const entry of entries) seen(cards.indexOf(entry.target), entry.intersectionRatio >= 0.5);
    },
    { threshold: 0.5 },
  );
  cards.forEach((card) => observer.observe(card));
};

export interface Cards {
  /** True while the visitor points at a card, or a card they pressed is playing. */
  readonly busy: boolean;
  /** Says whether the cards may take turns on their own. */
  turns(may: boolean): void;
  /** Stops the card that is playing and leaves it at its end. */
  rest(): void;
}

/**
 * Starts the cards in `list`. With `still` they never take turns and pointing at one does nothing; pressing
 * one still plays it. `onChange` is called whenever `busy` changes.
 */
export function mountCards(list: HTMLElement, options: { still: boolean; watch?: Watch; onChange?: () => void }): Cards {
  const cards = [...list.querySelectorAll<HTMLElement>('.card')].map((card) => {
    const [end, cues] = MINIS[card.dataset.mini ?? ''];
    const el = card.querySelector('.mini')!;
    const picture: Scene = scene(el, cues, end);
    picture.hold();
    return { card, picture: Object.assign(picture, { el }) };
  });
  list.style.setProperty('--fade', `${FADE}ms`);
  const inView = cards.map(() => false);
  /** The card that is playing, and whether the visitor started it. */
  let current = -1;
  let byHand = false;
  /** The card under the pointer. */
  let pointed = -1;
  /** The last card that played: the next turn goes to the first card in view after it. */
  let last = -1;
  let mayTurn = false;
  /** Ends the card that is playing. */
  let endTimer = 0;
  /** The pause before the next turn. While it runs, no card starts on its own. */
  let gapTimer = 0;
  let waiting = false;
  let wasBusy = false;

  const busy = () => pointed >= 0 || (current >= 0 && byHand);
  const changed = () => {
    if (busy() === wasBusy) return;
    wasBusy = busy();
    options.onChange?.();
  };

  /** The timer that ends the fade of each card's line. */
  const lineTimers = cards.map(() => 0);
  const lineOf = (card: HTMLElement) => card.querySelector<HTMLElement>('.prog b')!;
  /** Fades a card's line out from the width it has, instead of letting it vanish. */
  function ease(i: number) {
    const line = lineOf(cards[i].card);
    line.style.transform = getComputedStyle(line).transform;
    line.classList.add('ebb');
    window.clearTimeout(lineTimers[i]);
    lineTimers[i] = window.setTimeout(() => clear(i), FADE);
  }
  /** Takes a card's line back to empty at once. */
  function clear(i: number) {
    window.clearTimeout(lineTimers[i]);
    const line = lineOf(cards[i].card);
    line.classList.remove('ebb');
    line.style.removeProperty('transform');
  }
  /** Ends the card that is playing: its line fades out from where it is. */
  function finish(i: number) {
    ease(i);
    cards[i].card.classList.remove('playing');
  }

  function stop() {
    window.clearTimeout(endTimer);
    if (current < 0) return;
    dissolve(cards[current].card, cards[current].picture.el);
    cards[current].picture.hold();
    finish(current);
    current = -1;
    byHand = false;
  }
  function play(i: number, hand: boolean) {
    const was = current;
    stop();
    current = i;
    byHand = hand;
    last = i;
    const { card, picture } = cards[i];
    // A card that was playing has just been given its dissolve by `stop`.
    if (was !== i) dissolve(card, picture.el);
    // The line starts from empty, even for a card that was already playing: its animation starts again once style is computed without it.
    clear(i);
    void lineOf(card).getBoundingClientRect();
    card.style.setProperty('--dur', `${picture.end + LINGER}ms`);
    card.classList.add('playing');
    picture.play();
    endTimer = window.setTimeout(() => {
      finish(i);
      current = -1;
      byHand = false;
      wait(GAP);
      changed();
    }, picture.end + LINGER);
  }
  function next() {
    if (!mayTurn || waiting || pointed >= 0 || current >= 0) return;
    for (let k = 1; k <= cards.length; k++) {
      const i = (last + k) % cards.length;
      if (inView[i]) return play(i, false);
    }
  }
  /** Leaves `ms` before the next turn. The pause is set before anyone is told a card ended, so nothing starts a card inside it. */
  function wait(ms: number) {
    window.clearTimeout(gapTimer);
    waiting = true;
    gapTimer = window.setTimeout(() => {
      waiting = false;
      next();
    }, ms);
  }

  cards.forEach(({ card }, i) => {
    // Only a mouse points: a finger's touch is a press, and it never tells when it has left.
    card.addEventListener('pointerenter', (event) => {
      if (event.pointerType !== 'mouse' || options.still) return;
      pointed = i;
      play(i, true);
      changed();
    });
    card.addEventListener('pointerleave', (event) => {
      if (event.pointerType !== 'mouse' || pointed !== i) return;
      pointed = -1;
      stop();
      wait(AFTER_POINTER);
      changed();
    });
    card.addEventListener('click', () => {
      play(i, true);
      changed();
    });
  });

  (options.watch ?? watchInView)(
    cards.map(({ card }) => card),
    (i, seen) => {
      inView[i] = seen;
      if (seen) next();
    },
  );
  list.classList.add('live');

  return {
    get busy() {
      return busy();
    },
    turns(may) {
      mayTurn = may && !options.still;
      if (mayTurn) next();
      else if (current >= 0 && !byHand) stop();
    },
    rest() {
      stop();
      changed();
    },
  };
}
