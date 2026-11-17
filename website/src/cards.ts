// The cards of the home page: each has a small picture that plays one gesture. One card plays at a time. The
// cards in view take turns, and a card the visitor points at or presses plays at once.

import { add, follow, rem, scene, type Cue, type Run, type Scene } from './scene';

const move =
  (selector: string, x: number, y: number): Run =>
  (q) =>
    q(selector).forEach((part) => (part.style.transform = `translate(${x}px, ${y}px)`));
/** A request leaves the computer along `points`. With `real`, it picks the secret up after the first leg. */
const fly =
  (selector: string, points: number[][], ms: number, real: boolean): Run =>
  (q, instant) => {
    if (!instant) follow(q(selector)[0], points, ms, real);
  };

/** Each card's picture, by its `data-mini`: how long it lasts, and its cues. Everything is drawn in a 320 by 140 box. */
const MINIS: Record<string, [end: number, cues: Cue[]]> = {
  // A recipe is built once, then three people's computers start from it.
  rec: [
    3300,
    [
      [200, add('.r1')],
      [350, add('.r2')],
      [500, add('.r3')],
      [800, add('.fillb')],
      [1550, add('.bt')],
      [1700, add('.wc')],
      [1950, add('.c1')],
      [2200, add('.c2')],
      [2450, add('.c3')],
    ],
  ],
  // Your browser, editor and terminal each reach the fork.
  tools: [
    3000,
    [
      [300, add('.w1')],
      [600, add('.a1')],
      [1100, add('.w2')],
      [1400, add('.a2')],
      [1900, add('.w3')],
      [2200, add('.a3')],
    ],
  ],
  // The computer holds a placeholder. Requests to the allowed host and repository get the real value outside; a third does not.
  keys: [
    4700,
    [
      [300, add('.l1')],
      [700, add('.l2')],
      [1100, add('.outside')],
      [1400, fly('.p1', [[147, 70.5], [172.5, 70.5], [191.5, 70.5], [191.5, 40.5], [205.5, 40.5]], 900, true)],
      [1630, add('.gate', 'lit')],
      [2300, add('.ms, .hs', 'got')],
      [2300, add('.ws', 'real')],
      [2450, fly('.p2', [[147, 70.5], [172.5, 70.5], [205.5, 70.5]], 800, true)],
      [3250, add('.mg, .hg', 'got')],
      [3250, add('.wg', 'real')],
      [3400, fly('.p3', [[147, 70.5], [172.5, 70.5], [191.5, 70.5], [191.5, 100.5], [201, 100.5]], 800, false)],
      [4200, add('.xm')],
    ],
  ],
  // The network goes from open to limited, then the agent asks for one more host and you allow it.
  net: [
    3900,
    [
      [0, move('.knobg', 21.5, 0)],
      [0, move('.yc', 296, 126)],
      [900, move('.knobg', 34.5, 0)],
      [900, add('.mo-o', 'off')],
      [900, add('.mo-l')],
      [900, add('.wo, .do', 'off')],
      [900, add('.xo')],
      [1900, add('.askc')],
      [2500, add('.yc')],
      [2500, move('.yc', 58, 113)],
      [3200, rem('.askc')],
      [3200, add('.pp, .wp')],
      [3500, rem('.yc')],
    ],
  ],
  // What a fork changed, past where a Git diff ends.
  dif: [
    3300,
    [
      [300, add('.r1')],
      [900, add('.cut')],
      [1400, add('.r2, .bar')],
      [1650, add('.r3')],
      [1900, add('.r4')],
      [2150, add('.r5')],
    ],
  ],
  // Three forks become ready to merge. A rule merges one, and you go to merge another.
  wait: [
    3900,
    [
      [0, move('.yc', 300, 128)],
      [600, add('.d1', 'gone')],
      [600, add('.b1')],
      [1000, add('.d2', 'gone')],
      [1000, add('.b2')],
      [1400, add('.d3', 'gone')],
      [1400, add('.b3, .c3')],
      [2200, rem('.b2, .c3')],
      [2200, add('.byr, .wm2, .c2')],
      [2200, add('.r2', 'done')],
      [2900, add('.yc')],
      [2900, move('.yc', 284, 33)],
      [3600, rem('.yc')],
    ],
  ],
  // Two steps break things, and you fork from the step before them.
  steps: [
    3600,
    [
      [0, move('.yc', 250, 120)],
      [400, add('.s14, .l4')],
      [900, add('.s15, .l5')],
      [1500, add('.yc')],
      [1500, move('.yc', 166, 59)],
      [2200, add('.s13', 'hot')],
      [2300, add('.wb')],
      [2600, add('.fk')],
      [3300, rem('.yc')],
    ],
  ],
  // A computer comes onto your Mac from the server, then one from a teammate.
  far: [
    4000,
    [
      [0, move('.fl1', 136.5, 43.5)],
      [0, move('.fl2', 136.5, 107.5)],
      [400, add('.cp1')],
      [900, add('.w1, .fl1')],
      [900, move('.fl1', 186, 43.5)],
      [1500, rem('.fl1')],
      [1500, add('.m1')],
      [2000, add('.cp2')],
      [2500, add('.w2, .fl2')],
      [2500, move('.fl2', 186, 107.5)],
      [3100, rem('.fl2')],
      [3100, add('.m2')],
    ],
  ],
  // An agent clicks through a Linux screen, another through a macOS one, and you open one yourself.
  scr: [
    3700,
    [
      [0, move('.ac', 150, 108)],
      [0, move('.ac2', 292, 122)],
      [0, move('.yc', 176, 128)],
      [200, add('.ac')],
      [200, move('.ac', 104, 66)],
      [800, add('.f1')],
      [1000, move('.ac', 58, 90)],
      [1600, add('.btnr, .dk2')],
      [1700, add('.ac2')],
      [1700, move('.ac2', 230, 86)],
      [2400, add('.mw')],
      [2700, add('.yc')],
      [2700, move('.yc', 132, 84)],
    ],
  ],
};

/** How long a card stays on its last moment before its turn ends, the pause between two turns, and the pause after the pointer leaves. */
const LINGER = 600;
const GAP = 400;
const AFTER_POINTER = 900;

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
    const picture: Scene = scene(card.querySelector('.mini')!, cues, end);
    picture.hold();
    return { card, picture };
  });
  const inView = cards.map(() => false);
  /** The card that is playing, and whether the visitor started it. */
  let current = -1;
  let byHand = false;
  /** The card under the pointer. */
  let pointed = -1;
  /** The last card that played: the next turn goes to the first card in view after it. */
  let last = -1;
  let mayTurn = false;
  let timer = 0;
  let wasBusy = false;

  const busy = () => pointed >= 0 || (current >= 0 && byHand);
  const changed = () => {
    if (busy() === wasBusy) return;
    wasBusy = busy();
    options.onChange?.();
  };

  function stop() {
    window.clearTimeout(timer);
    if (current < 0) return;
    cards[current].picture.hold();
    cards[current].card.classList.remove('playing');
    current = -1;
    byHand = false;
  }
  function play(i: number, hand: boolean) {
    stop();
    current = i;
    byHand = hand;
    last = i;
    const { card, picture } = cards[i];
    card.style.setProperty('--dur', `${picture.end + LINGER}ms`);
    card.classList.add('playing');
    picture.play();
    timer = window.setTimeout(() => {
      card.classList.remove('playing');
      current = -1;
      byHand = false;
      changed();
      wait(GAP);
    }, picture.end + LINGER);
  }
  function next() {
    if (!mayTurn || pointed >= 0 || current >= 0) return;
    for (let k = 1; k <= cards.length; k++) {
      const i = (last + k) % cards.length;
      if (inView[i]) return play(i, false);
    }
  }
  function wait(ms: number) {
    window.clearTimeout(timer);
    timer = window.setTimeout(next, ms);
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
      changed();
      wait(AFTER_POINTER);
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
