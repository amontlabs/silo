// The cards of the home page, six features and three examples of extensions. A picture is drawn at its end in
// index.html: playing it is a set of animations over that drawing, and stopping it is cancelling them. One card
// plays at a time. The cards in view take turns, and a card the visitor points at, focuses or presses plays at once.

type Style = { opacity?: number; transform?: string };
/** A moment, what a part is then, and how it eases to the next moment. */
type Key = readonly [at: number, style: Style, easing?: string];
/** A part, named by its `data-part`, and what it is at each moment. */
type Track = readonly [part: string, keys: readonly Key[]];
type Leg = readonly [at: number, x: number, y: number];

const EASE = 'cubic-bezier(0.65, 0, 0.35, 1)'; // a move
const OUT = 'cubic-bezier(0.33, 1, 0.68, 1)'; // an arrival

/** A part that arrives at `at`, rising by `dy`. */
const show = (part: string, at: number, dy = 4, d = 260): Track => [part, [
  [0, { opacity: 0, transform: `translateY(${dy}px)` }],
  [at, { opacity: 0, transform: `translateY(${dy}px)` }, OUT],
  [at + d, { opacity: 1, transform: 'translateY(0px)' }],
]];
/** A stroke drawn from its transform-origin over `d`. */
const draw = (part: string, at: number, d: number, ease = EASE): Track => [part, [
  [0, { transform: 'scaleX(0)' }],
  [at, { transform: 'scaleX(0)' }, ease],
  [at + d, { transform: 'scaleX(1)' }],
]];
/** A part that lands from `dx`, `dy` where it ends. */
const land = (part: string, at: number, dx: number, dy: number, d = 420): Track => [part, [
  [0, { opacity: 0, transform: `translate(${dx}px, ${dy}px)` }],
  [at, { opacity: 0, transform: `translate(${dx}px, ${dy}px)` }, OUT],
  [at + d / 3, { opacity: 1, transform: `translate(${dx * 0.3}px, ${dy * 0.3}px)` }, OUT],
  [at + d, { opacity: 1, transform: 'translate(0px, 0px)' }],
]];
/** A cover that uncovers `count` characters, one at a time. */
const type = (part: string, at: number, d: number, count: number): Track => [part, [
  [0, { transform: 'scaleX(1)' }],
  [at, { transform: 'scaleX(1)' }, `steps(${count}, end)`],
  [at + d, { transform: 'scaleX(0)' }],
]];
/** A dot that shows at `at`, goes through `legs` and is gone at `until`, unless it stays. */
const travel = (part: string, at: number, legs: readonly Leg[], until?: number): Track => {
  const look = (x: number, y: number, opacity: number) => ({ opacity, transform: `translate(${x}px, ${y}px)` });
  const [, fromX, fromY] = legs[0];
  const keys: Key[] = [[0, look(fromX, fromY, 0)], [at, look(fromX, fromY, 0)]];
  legs.forEach(([t, x, y], i) => keys.push([t, look(x, y, 1), i < legs.length - 1 ? EASE : undefined]));
  if (until !== undefined) {
    const [, x, y] = legs[legs.length - 1];
    keys.push([until, look(x, y, 1)], [until + 160, look(x, y, 0)]);
  }
  return [part, keys];
};

/** Each card's picture, by its `data-picture`: how long its gesture lasts, and its tracks. */
export const PICTURES: Record<string, readonly [end: number, tracks: readonly Track[]]> = {
  // sudo is typed inside. One request passes its rule, and the other meets none.
  rules: [3300, [
    type('typed', 250, 420, 6),
    travel('request-a', 750, [[850, 0, 0], [1700, 266, 0]], 1750),
    draw('trail-a', 850, 850),
    show('reached', 1700, 0, 160),
    travel('request-b', 2000, [[2100, -25, 0], [2650, 0, 0]]),
    draw('trail-b', 2100, 550),
    show('denied', 2800),
  ]],
  // The placeholder goes into the gate and the real value comes out of it: two chips on one path, cut at the boundary.
  secrets: [2700, [
    ['as-held', [
      [0, { opacity: 1, transform: 'translate(0px, 0px)' }],
      [250, { opacity: 1, transform: 'translate(0px, 0px)' }, EASE],
      [750, { opacity: 1, transform: 'translate(0px, 58px)' }, EASE],
      [2100, { opacity: 1, transform: 'translate(225px, 58px)' }],
      [2101, { opacity: 0, transform: 'translate(225px, 58px)' }],
    ]],
    ['as-real', [
      [0, { transform: 'translateX(-225px)' }],
      [750, { transform: 'translateX(-225px)' }, EASE],
      [2100, { transform: 'translateX(0px)' }],
    ]],
    show('from-held', 450, 0, 250),
    draw('trail', 800, 1250),
    show('to-host', 2150, 0, 200),
    show('reached', 2350, 0, 200),
  ]],
  // A file changes and is seen from outside. After it, a program's word comes out of the box.
  record: [3200, [
    draw('change', 300, 400),
    draw('seen', 750, 500),
    land('fact', 1050, 24, 0),
    show('fact-said', 1400),
    draw('said', 1900, 650, 'linear'),
    ['report', [
      [0, { opacity: 0, transform: 'translateX(-74px)' }],
      [1720, { opacity: 0, transform: 'translateX(-74px)' }],
      [1900, { opacity: 1, transform: 'translateX(-74px)' }, 'linear'],
      [2550, { opacity: 1, transform: 'translateX(0px)' }],
    ]],
    show('report-said', 2700),
  ]],
  // An agent's pointer opens a window in main and presses its button. Your screen has no track.
  screen: [3100, [
    travel('agent', 200, [[450, 12, -50], [1100, -44, 25], [1750, -44, 25], [2450, 0, 0]]),
    show('launched', 1220, 0, 120),
    ['opened', [
      [0, { opacity: 0, transform: 'scale(0.92)' }],
      [1380, { opacity: 0, transform: 'scale(0.92)' }, OUT],
      [1680, { opacity: 1, transform: 'scale(1)' }],
    ]],
    show('pressed', 2580, 0, 120),
  ]],
  // A text is written, two computers are made from it, and each passes its tests.
  recipes: [3700, [
    type('write-base', 200, 320, 13),
    type('write-nix', 580, 380, 16),
    type('write-pieces', 1020, 360, 15),
    draw('branches', 1500, 550),
    show('mac', 1950),
    show('linux', 2050),
    draw('mac-tests', 2350, 700, 'linear'),
    draw('linux-tests', 2450, 900, 'linear'),
    show('mac-passed', 3100, 0, 160),
    show('linux-passed', 3450, 0, 160),
  ]],
  // The record gathers, the extension reads it, and one alert comes of that.
  extensions: [3300, [
    show('mark-a', 200, 0, 160),
    show('mark-b', 420, 0, 160),
    show('mark-c', 640, 0, 160),
    show('mark-d', 860, 0, 160),
    draw('reads', 1100, 650),
    travel('read', 1150, [[1300, -72, 8], [1850, 0, 0]], 1900),
    draw('read-a', 1850, 300),
    draw('read-b', 2100, 250),
    land('alert', 2550, 0, -10, 300),
    show('alert-said', 2850),
  ]],
  // The clock ticks, /work is read, and a panel comes of it.
  disk: [2900, [
    ['tick', [
      [0, { transform: 'rotate(-30deg)' }],
      [350, { transform: 'rotate(-30deg)' }, OUT],
      [600, { transform: 'rotate(0deg)' }],
    ]],
    draw('asked', 750, 400),
    travel('read', 1100, [[1200, 0, 0], [1700, 34, 0]], 1720),
    draw('shown', 1850, 250),
    show('panel', 2050),
    show('warned', 2600, 0),
  ]],
  // A call waits at the boundary while the extension is asked. Its deny sends the call back.
  call: [3200, [
    ['call', [
      [0, { opacity: 0, transform: 'translateX(22px)' }],
      [150, { opacity: 0, transform: 'translateX(22px)' }, OUT],
      [700, { opacity: 1, transform: 'translateX(-8px)' }],
      [2350, { opacity: 1, transform: 'translateX(-8px)' }, EASE],
      [3050, { opacity: 1, transform: 'translateX(0px)' }],
    ]],
    show('asked', 900, 0, 250),
    travel('ask', 1000, [[1150, 0, 0], [1450, 0, 27], [1700, -28, 27]], 1720),
    land('deny', 1900, -10, 0, 300),
    draw('back', 2350, 700),
    show('head', 3000, 0, 140),
  ]],
  // Two rules land on the boundary, and the lane under them opens. No computer comes with them.
  registry: [2300, [
    land('host', 250, 0, -16),
    land('allow', 800, 0, 16),
    draw('opened-lane', 1400, 650),
    show('reached', 2050, 0, 200),
  ]],
};

/** How long a picture rests on its end before the next takes its turn. */
export const LINGER = 1300;
/** The pause between two turns. */
export const GAP = 400;
/** The pause once the visitor has left a card. */
export const AFTER_POINTER = 900;
/** How long a picture dissolves into its next look and a card's line fades out. */
export const FADE = 250;

/** Counts the copies, so that each has ids of its own. */
let copies = 0;

/**
 * Lays a copy of `picture` as it looks now over it, and fades the copy out. The copy runs nothing: what `running`
 * was moving gets its value of the moment written on it. Its clip paths are its own, so that they stay as they
 * were while those of the picture under it move.
 */
function dissolve(picture: SVGSVGElement, running: readonly Animation[]) {
  const copy = picture.cloneNode(true) as SVGSVGElement;
  const live = [...picture.querySelectorAll<SVGElement>('*')];
  const parts = [...copy.querySelectorAll<SVGElement>('*')];
  for (const animation of running) {
    const part = (animation.effect as KeyframeEffect).target as SVGElement;
    const now = getComputedStyle(part);
    const twin = parts[live.indexOf(part)];
    twin.style.opacity = now.opacity;
    twin.style.transform = now.transform;
  }
  const suffix = `-copy-${++copies}`;
  for (const part of parts) {
    if (part.id) part.id += suffix;
    const clip = part.getAttribute('clip-path');
    if (clip) part.setAttribute('clip-path', clip.replace(')', `${suffix})`));
  }
  copy.classList.add('fc-ghost');
  // Right above the picture and under any copy still fading, so that what the visitor sees stays continuous.
  picture.after(copy);
  copy.animate([{ opacity: 1 }, { opacity: 0 }], { duration: FADE, fill: 'forwards' });
  window.setTimeout(() => copy.remove(), FADE);
}

interface Card {
  item: HTMLElement;
  picture: SVGSVGElement;
  end: number;
  moves: { part: SVGElement; keys: readonly Key[] }[];
  line: HTMLElement;
  fill?: Animation;
  running: Animation[];
}

function start(card: Card) {
  const duration = card.end + LINGER;
  card.running = card.moves.map(({ part, keys }) => {
    const frames: Keyframe[] = keys.map(([at, style, easing]) => ({ ...style, offset: at / duration, easing: easing ?? 'linear' }));
    frames.push({ ...keys[keys.length - 1][1], offset: 1 });
    return part.animate(frames, { duration, fill: 'both' });
  });
  card.fill?.cancel();
  card.fill = card.line.animate([{ opacity: 1, transform: 'scaleX(0)' }, { opacity: 1, transform: 'scaleX(1)' }], { duration, fill: 'both' });
}
/** The picture is at its end again, as it is drawn. */
function settle(card: Card) {
  for (const animation of card.running) animation.cancel();
  card.running = [];
}
/** The card's line fades out from the width it has. */
function ebb(card: Card) {
  const width = getComputedStyle(card.line).transform;
  card.fill?.cancel();
  card.fill = card.line.animate([{ opacity: 1, transform: width }, { opacity: 0, transform: width }], { duration: FADE });
}

/** Tells `seen` which cards came into view or left it, all those of one moment at once. */
export type Watch = (items: Element[], seen: (changed: readonly (readonly [index: number, inView: boolean])[]) => void) => void;

/** A card counts as in view once half of it is. */
const watchInView: Watch = (items, seen) => {
  const observer = new IntersectionObserver(
    (entries) => seen(entries.map((entry) => [items.indexOf(entry.target), entry.intersectionRatio >= 0.5] as const)),
    { threshold: 0.5 },
  );
  items.forEach((item) => observer.observe(item));
};

export interface Cards {
  /** True while the visitor points at a card, or a card they started is playing. */
  readonly busy: boolean;
  /** Says whether the cards may take turns on their own. */
  turns(may: boolean): void;
  /** Says whether every card stays at its end, as reduced motion asks. */
  still(on: boolean): void;
  /** Stops the card that is playing and leaves it at its end. */
  rest(): void;
}

/** Starts the cards in `section`. `onChange` is called whenever `busy` changes. */
export function mountCards(section: HTMLElement, options: { still: boolean; watch?: Watch; onChange?: () => void }): Cards {
  const cards = [...section.querySelectorAll<HTMLElement>('.fc-item')].map((item): Card => {
    const [end, tracks] = PICTURES[item.dataset.picture!];
    const picture = item.querySelector<SVGSVGElement>('.fc-pic svg')!;
    const moves = tracks.map(([name, keys]) => ({ part: picture.querySelector<SVGElement>(`[data-part="${name}"]`)!, keys }));
    return { item, picture, end, moves, line: item.querySelector<HTMLElement>('.fc-fill')!, running: [] };
  });
  const inView = cards.map(() => false);
  let still = options.still;
  let mayTurn = false;
  /** The card that is playing, and whether the visitor started it. */
  let current = -1;
  let byHand = false;
  /** The card under the pointer. */
  let pointed = -1;
  /** The last card that played: the next turn goes to the first card in view after it. */
  let last = -1;
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

  function halt() {
    window.clearTimeout(endTimer);
    if (current < 0) return;
    const card = cards[current];
    dissolve(card.picture, card.running);
    settle(card);
    ebb(card);
    current = -1;
    byHand = false;
  }
  function play(i: number, hand: boolean) {
    if (still || document.visibilityState === 'hidden') return;
    const was = current;
    halt();
    window.clearTimeout(gapTimer);
    waiting = false;
    const card = cards[i];
    // A card that was playing has just been given its dissolve by `halt`.
    if (was !== i) dissolve(card.picture, card.running);
    current = last = i;
    byHand = hand;
    start(card);
    endTimer = window.setTimeout(() => {
      settle(card);
      ebb(card);
      current = -1;
      byHand = false;
      wait(GAP);
      changed();
    }, card.end + LINGER);
  }
  function next() {
    if (!mayTurn || still || waiting || pointed >= 0 || current >= 0) return;
    for (let k = 1; k <= cards.length; k++) {
      const i = (last + k) % cards.length;
      if (inView[i]) return play(i, false);
    }
  }
  /** Leaves `ms` before the next turn. The pause is set before anyone is told a card ended, so nothing starts inside it. */
  function wait(ms: number) {
    window.clearTimeout(gapTimer);
    waiting = true;
    gapTimer = window.setTimeout(() => {
      waiting = false;
      next();
    }, ms);
  }
  function byVisitor(i: number) {
    play(i, true);
    changed();
  }

  cards.forEach(({ item }, i) => {
    // Only a mouse points: a finger presses, and never tells when it has left.
    item.addEventListener('pointerenter', (event) => {
      if (event.pointerType !== 'mouse' || still) return;
      pointed = i;
      byVisitor(i);
    });
    item.addEventListener('pointerleave', (event) => {
      if (event.pointerType !== 'mouse' || pointed !== i) return;
      pointed = -1;
      if (current === i) halt();
      wait(AFTER_POINTER);
      changed();
    });
    item.addEventListener('focus', () => {
      if (item.matches(':focus-visible')) byVisitor(i);
    });
    item.addEventListener('blur', () => {
      if (current !== i || !byHand || pointed === i) return;
      halt();
      wait(AFTER_POINTER);
      changed();
    });
    item.addEventListener('click', () => {
      if (current !== i || !byHand) byVisitor(i);
    });
    item.addEventListener('keydown', (event) => {
      if (event.key !== 'Enter' && event.key !== ' ') return;
      event.preventDefault();
      byVisitor(i);
    });
  });

  (options.watch ?? watchInView)(
    cards.map(({ item }) => item),
    (changes) => {
      for (const [i, seen] of changes) inView[i] = seen;
      if (current >= 0 && !byHand && !inView[current]) halt();
      next();
    },
  );

  return {
    get busy() {
      return busy();
    },
    turns(may) {
      mayTurn = may;
      if (may) next();
      else if (current >= 0 && !byHand) halt();
    },
    still(on) {
      still = on;
      if (on) {
        pointed = -1;
        halt();
      } else next();
      changed();
    },
    rest() {
      halt();
      changed();
    },
  };
}
