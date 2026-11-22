import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { recordAnimations } from './cards-test-support';
import { FADE, LINGER, PICTURES } from './cards';
import { LENGTH, STILL, TIMED, endOf } from './film/story';
import { homePage, measurableSvg } from './film/test-support';
import { mountPictures } from './pictures';

// The page as it is served, with what a browser gives and jsdom lacks: nothing is in view until a test says so.
const page = new DOMParser().parseFromString(homePage(), 'text/html').body.innerHTML;

type Seen = (entries: { target: Element; intersectionRatio: number }[]) => void;
let watchers: { seen: Seen; targets: Element[] }[] = [];
let reduced = false;
let motionChanged: (() => void)[] = [];
let animations: ReturnType<typeof recordAnimations>;
let visibility = 'visible';

/** Puts `target` in view by `ratio`, as its observer would report it. */
const see = (target: Element, ratio: number) =>
  watchers.filter((watcher) => watcher.targets.includes(target)).forEach((watcher) => watcher.seen([{ target, intersectionRatio: ratio }]));
const NAMES = Object.keys(PICTURES);
const $ = (selector: string) => document.querySelector<HTMLElement>(selector)!;
const root = () => $('.film');
const figure = () => $('.film figure');
/** The moment the film shows, read from its progress line. */
const moment = () =>
  [...document.querySelectorAll<HTMLElement>('.film-progress i')].reduce(
    (sum, bar, i) => sum + Number(/[\d.]+/.exec(bar.style.transform)?.[0] ?? 0) * TIMED[i].dur,
    0,
  );
/** How many shots have been played whole before the one on screen. */
const shot = () => TIMED.filter(({ start, dur }) => moment() >= start + dur - 1).length;
/** Moving, held by the page, or paused by the visitor. */
const filmIs = () => ($('.film-pause').textContent === 'Play' ? 'paused' : root().film!.running ? 'moving' : 'held');
const card = (name: string) => $(`.fc-item[data-picture="${name}"]`);
const playing = () => animations.playing();
const lasts = (name: string) => PICTURES[name][0] + LINGER;
const setMotion = (on: boolean) => {
  reduced = on;
  motionChanged.forEach((run) => run());
};
const pointer = (target: Element, type: string, kind: string) => {
  const event = new MouseEvent(type);
  Object.defineProperty(event, 'pointerType', { value: kind });
  target.dispatchEvent(event);
};
/** The cards that play over the next `ms`, in order. */
const turnsOver = (ms: number) => {
  const turns: string[] = [];
  for (let passed = 0; passed <= ms; passed += 100) {
    const name = playing()[0];
    if (name && name !== turns[turns.length - 1]) turns.push(name);
    vi.advanceTimersByTime(100);
  }
  return turns;
};
/** What of the cards is moving or fading, its copies included. */
const cardsMove = () => animations.live().filter(({ target }) => target.isConnected);
const segment = (i: number) => document.querySelectorAll<HTMLButtonElement>('.film-progress button')[i];
/** Whether the film has two frames on screen. */
const crossing = () => document.querySelectorAll('.film-window > .f-canvas').length > 1;
const show = (state: string) => {
  visibility = state;
  document.dispatchEvent(new Event('visibilitychange'));
};

// The page listens on the document, which outlives a test: what one test's page added is taken off after it.
const listen = document.addEventListener.bind(document);
let listening: [string, EventListenerOrEventListenerObject][] = [];

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'performance', 'Date'] });
  // A frame every 100 ms: the film is drawn from its moment alone, and jsdom draws one slowly.
  vi.stubGlobal('requestAnimationFrame', (run: FrameRequestCallback) => setTimeout(() => run(performance.now()), 100));
  vi.stubGlobal('cancelAnimationFrame', (id: number) => clearTimeout(id));
  measurableSvg();
  document.addEventListener = ((type: string, listener: EventListenerOrEventListenerObject) => {
    listening.push([type, listener]);
    listen(type, listener);
  }) as typeof document.addEventListener;
  watchers = [];
  reduced = false;
  visibility = 'visible';
  vi.stubGlobal(
    'IntersectionObserver',
    class {
      targets: Element[] = [];
      constructor(public seen: Seen) {
        watchers.push(this);
      }
      observe(target: Element) {
        this.targets.push(target);
      }
    },
  );
  motionChanged = [];
  vi.stubGlobal('matchMedia', (query: string) => ({
    get matches() {
      return query.includes('reduced-motion') && reduced;
    },
    addEventListener: (_type: string, run: () => void) => motionChanged.push(run),
  }));
  animations = recordAnimations();
  Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => visibility });
  document.documentElement.className = 'pictures-pending';
  document.body.innerHTML = page;
});
afterEach(() => {
  listening.forEach(([type, listener]) => document.removeEventListener(type, listener));
  listening = [];
  document.addEventListener = listen;
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('the two pictures', () => {
  it('show once their script has drawn them: the film at its still, every card at its end', () => {
    mountPictures();
    expect(document.documentElement.classList.contains('pictures-pending')).toBe(false);
    expect(root().classList.contains('live')).toBe(true);
    expect(moment()).toBeCloseTo(STILL, -1);
    // A card is drawn at its end: nothing is over the drawing.
    expect(animations.live()).toEqual([]);
    expect(document.querySelectorAll('svg.fc-ghost')).toHaveLength(0);
  });

  it('show the still until the film first moves, then play it from its start', () => {
    mountPictures();
    vi.advanceTimersByTime(5000);
    expect(moment()).toBeCloseTo(STILL, -1);
    see(figure(), 1);
    expect(filmIs()).toBe('moving');
    expect(moment()).toBe(0);
    vi.advanceTimersByTime(2000);
    expect(moment()).toBeCloseTo(1900, -1);
  });

  it('start the film from its first frame when it is in view as the page opens', () => {
    const box = vi.spyOn(Element.prototype, 'getBoundingClientRect').mockReturnValue({ top: 100, bottom: 500, height: 400 } as DOMRect);
    mountPictures();
    expect(moment()).toBe(0);
    box.mockRestore();
  });

  it('hold the film at the moment the address names, and play nothing', () => {
    mountPictures({ hold: 31000 });
    see(figure(), 1);
    vi.advanceTimersByTime(3000);
    expect(moment()).toBeCloseTo(31000, -1);
    expect(filmIs()).not.toBe('moving');
    expect(($('.film-pause') as HTMLButtonElement).hidden).toBe(true);
  });

  it('move one at a time: the film while 30% of it is in view, the cards only while the film is not playing', () => {
    mountPictures();
    see(card('rules'), 1);
    see(card('secrets'), 1);
    expect(filmIs()).toBe('held');
    expect(playing()).toEqual(['rules']);

    see(figure(), 0.29);
    expect(filmIs()).toBe('held');
    see(figure(), 0.3);
    expect(filmIs()).toBe('moving');
    expect($('.film-pause').textContent).toBe('Pause');
    expect(playing()).toEqual([]);
    vi.advanceTimersByTime(20000);
    expect(playing()).toEqual([]);

    see(figure(), 0);
    expect(filmIs()).toBe('held');
    expect(playing()).toEqual(['secrets']);
  });

  it('give the turn to the next card in view, and to none that is out of view', () => {
    mountPictures();
    see(card('rules'), 1);
    see(card('recipes'), 0.6);
    see(card('extensions'), 0.2);
    expect(turnsOver(16000)).toEqual(['rules', 'recipes', 'rules', 'recipes']);
  });

  it('hold the film at its moment when it leaves the view, and carry on from there', () => {
    mountPictures();
    see(figure(), 1);
    vi.advanceTimersByTime(4000);
    const left = moment();
    expect(left).toBeCloseTo(3900, -1);
    see(figure(), 0);
    vi.advanceTimersByTime(60000);
    expect(moment()).toBe(left);
    see(figure(), 1);
    vi.advanceTimersByTime(1000);
    expect(moment()).toBeGreaterThan(left + 700);
    expect(moment()).toBeLessThan(left + 1100);
  });

  it('play the film in a loop', () => {
    mountPictures();
    see(figure(), 1);
    vi.advanceTimersByTime(LENGTH - 500);
    expect(shot()).toBe(TIMED.length - 1);
    vi.advanceTimersByTime(1000);
    expect(shot()).toBe(0);
    expect(filmIs()).toBe('moving');
  });

  it('play the card the visitor points at, and carry on from it once it ends', () => {
    mountPictures();
    see(card('rules'), 1);
    see(card('secrets'), 1);
    pointer(card('recipes'), 'pointerenter', 'mouse');
    expect(playing()).toEqual(['recipes']);
    // The card ends, the pointer stays: no other card starts.
    vi.advanceTimersByTime(10000);
    expect(playing()).toEqual([]);
    pointer(card('recipes'), 'pointerleave', 'mouse');
    vi.advanceTimersByTime(900);
    expect(turnsOver(10000)).toEqual(['rules', 'secrets', 'rules']);
  });

  it('play the card the visitor points at and hold the film until they leave it', () => {
    mountPictures();
    see(figure(), 1);
    vi.advanceTimersByTime(4000);
    pointer(card('recipes'), 'pointerenter', 'mouse');
    expect(playing()).toEqual(['recipes']);
    expect(filmIs()).toBe('held');
    // The card ends, the pointer stays: the film still waits.
    vi.advanceTimersByTime(10000);
    expect(playing()).toEqual([]);
    expect(filmIs()).toBe('held');
    expect(shot()).toBe(0);
    pointer(card('recipes'), 'pointerleave', 'mouse');
    expect(filmIs()).toBe('moving');
    vi.advanceTimersByTime(5000);
    expect(playing()).toEqual([]);
  });

  it('leave the film paused after a card, when the visitor paused it', () => {
    mountPictures();
    see(figure(), 1);
    vi.advanceTimersByTime(4000);
    $('.film-pause').click();
    expect(filmIs()).toBe('paused');
    expect($('.film-pause').textContent).toBe('Play');
    pointer(card('recipes'), 'pointerenter', 'mouse');
    pointer(card('recipes'), 'pointerleave', 'mouse');
    vi.advanceTimersByTime(30000);
    expect(filmIs()).toBe('paused');
    expect(shot()).toBe(0);
  });

  it('take a touch as a press, not as pointing: the card plays once, then the film carries on', () => {
    mountPictures();
    see(figure(), 1);
    pointer(card('record'), 'pointerenter', 'touch');
    expect(playing()).toEqual([]);
    expect(filmIs()).toBe('moving');
    card('record').click();
    expect(playing()).toEqual(['record']);
    expect(filmIs()).toBe('held');
    vi.advanceTimersByTime(lasts('record'));
    expect(playing()).toEqual([]);
    expect(filmIs()).toBe('moving');
  });

  it('keep taking turns after the pointer leaves a card, once the pause after it is over', () => {
    mountPictures();
    see(card('rules'), 1);
    see(card('secrets'), 1);
    see(card('recipes'), 1);
    pointer(card('rules'), 'pointerenter', 'mouse');
    vi.advanceTimersByTime(1000);
    pointer(card('rules'), 'pointerleave', 'mouse');
    expect(playing()).toEqual([]);
    vi.advanceTimersByTime(899);
    expect(playing()).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(turnsOver(15000)).toEqual(['secrets', 'recipes', 'rules', 'secrets']);
  });

  it('keep taking turns after a pressed card ends, once the pause between two turns is over', () => {
    mountPictures();
    see(card('rules'), 1);
    see(card('record'), 1);
    card('record').click();
    expect(playing()).toEqual(['record']);
    vi.advanceTimersByTime(lasts('record'));
    expect(playing()).toEqual([]);
    vi.advanceTimersByTime(399);
    expect(playing()).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(turnsOver(10000)).toEqual(['rules', 'record', 'rules']);
  });

  it('never move the film and a card at once, and always come back to taking turns', () => {
    mountPictures();
    for (const name of NAMES) see(card(name), 1);
    // A visitor's hand in every order, between turns: whatever happened, a minute later the cards still take turns.
    const hands = [
      () => pointer(card('recipes'), 'pointerenter', 'mouse'),
      () => pointer(card('recipes'), 'pointerleave', 'mouse'),
      () => card('extensions').click(),
      () => card('disk').click(),
      () => segment(pick(TIMED.length)).click(),
      () => pointer(card('rules'), 'pointerleave', 'mouse'),
      () => see(figure(), 1),
      () => see(figure(), 0),
      () => show('hidden'),
      () => show('visible'),
      () => setMotion(!reduced),
      () => $('.film-pause').click(),
    ];
    // A seeded generator (Park-Miller), the same hands on every run; the choices come from its whole value, as
    // its low bits repeat every few calls and would leave most hands unused.
    let seed = 7;
    const pick = (below: number) => Math.floor(((seed = (seed * 48271) % 2147483647) / 2147483647) * below);
    let moved = 0;
    const oneMoves = (round: number) => {
      if (root().film!.moving || reduced) expect(cardsMove(), `round ${round}`).toEqual([]);
      if (reduced) expect(crossing(), `round ${round}`).toBe(false);
    };
    for (let round = 0; round < 160; round++) {
      hands[pick(hands.length)]();
      // At once, and after a while that can be shorter than a fade or a crossing.
      oneMoves(round);
      vi.advanceTimersByTime(pick(2) ? pick(300) : pick(2500));
      oneMoves(round);
      const cards = playing();
      expect(cards.length).toBeLessThanOrEqual(1);
      expect(animations.moving()).toEqual(cards);
      if (filmIs() === 'moving' || reduced || visibility === 'hidden') expect(cards, `round ${round}`).toEqual([]);
      moved += cards.length;
    }
    expect(moved).toBeGreaterThan(0);
    // Back to rest: pointer away, tab shown, film out of view, motion allowed.
    pointer(card('recipes'), 'pointerleave', 'mouse');
    show('visible');
    setMotion(false);
    see(figure(), 0);
    // After reduced motion they wait for the visitor's hand.
    card('rules').click();
    vi.advanceTimersByTime(10000);
    // A minute of turns: every card takes one.
    expect(new Set(turnsOver(60000)).size).toBe(NAMES.length);
  });

  it('pause where the film is, and go on from there', () => {
    mountPictures();
    see(figure(), 1);
    vi.advanceTimersByTime(4000);
    const left = moment();
    $('.film-pause').click();
    vi.advanceTimersByTime(5000);
    expect(moment()).toBe(left);
    $('.film-pause').click();
    vi.advanceTimersByTime(1000);
    expect(moment()).toBeGreaterThan(left + 700);
    expect(filmIs()).toBe('moving');
  });

  it('jump by the progress line and by the sidebar: to the start while playing, to the end while paused', () => {
    mountPictures();
    see(figure(), 1);
    const segments = [...document.querySelectorAll<HTMLButtonElement>('.film-progress button')];
    segments[4].click();
    expect(moment()).toBeCloseTo(TIMED[4].start, -1);
    $('.film-pause').click();
    segments[5].click();
    expect(moment()).toBeCloseTo(endOf(TIMED[5]), -1);
    const disk = $('.f-side [data-go="disk"]');
    disk.dispatchEvent(new MouseEvent('click', { bubbles: true }));
    expect(moment()).toBeCloseTo(endOf(TIMED[6]), -1);
    $('.f-side [data-go="now"]').dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter' }));
    expect(moment()).toBeCloseTo(endOf(TIMED[1]), -1);
  });

  it('hold everything while the tab is hidden, and carry on when it shows again', () => {
    mountPictures();
    see(figure(), 1);
    vi.advanceTimersByTime(4000);
    show('hidden');
    expect(filmIs()).toBe('held');
    vi.advanceTimersByTime(60000);
    expect(shot()).toBe(0);
    show('visible');
    expect(filmIs()).toBe('moving');

    see(figure(), 0);
    see(card('rules'), 1);
    expect(playing()).toEqual(['rules']);
    show('hidden');
    expect(playing()).toEqual([]);
    vi.advanceTimersByTime(60000);
    expect(playing()).toEqual([]);
    show('visible');
    expect(playing()).toHaveLength(1);
  });

  it('run nothing for the film while it is held: no frame and no timer', () => {
    mountPictures();
    see(figure(), 1);
    vi.advanceTimersByTime(4000);
    see(figure(), 0);
    // The cards do not take turns either: none is in view.
    expect(vi.getTimerCount()).toBe(0);
  });

  it('move nothing with reduced motion until the visitor presses Play, and never a card', () => {
    reduced = true;
    mountPictures();
    see(figure(), 1);
    see(card('rules'), 1);
    expect(filmIs()).toBe('paused');
    expect($('.film-pause').textContent).toBe('Play');
    expect(moment()).toBeCloseTo(STILL, -1);
    vi.advanceTimersByTime(60000);
    expect(filmIs()).toBe('paused');
    expect(moment()).toBeCloseTo(STILL, -1);

    pointer(card('rules'), 'pointerenter', 'mouse');
    card('rules').click();
    expect(animations.live()).toEqual([]);

    // Play goes on from the still.
    $('.film-pause').click();
    expect(filmIs()).toBe('moving');
    vi.advanceTimersByTime(1000);
    expect(moment()).toBeGreaterThan(STILL + 700);
    see(figure(), 0);
    vi.advanceTimersByTime(60000);
    expect(playing()).toEqual([]);
  });

  it('pause the film on the end of its shot when motion is reduced, and start nothing when it no longer is', () => {
    mountPictures();
    see(figure(), 1);
    vi.advanceTimersByTime(TIMED[2].start + 1000);
    see(card('rules'), 1);
    setMotion(true);
    expect(filmIs()).toBe('paused');
    expect(moment()).toBeCloseTo(endOf(TIMED[2]), -1);
    vi.advanceTimersByTime(10000);
    expect(moment()).toBeCloseTo(endOf(TIMED[2]), -1);
    setMotion(false);
    see(card('secrets'), 1);
    show('hidden');
    show('visible');
    vi.advanceTimersByTime(60000);
    expect(filmIs()).toBe('paused');
    expect(moment()).toBeCloseTo(endOf(TIMED[2]), -1);
    expect(cardsMove()).toEqual([]);
  });

  it('keep the still of a film that had not played when motion is reduced', () => {
    mountPictures();
    setMotion(true);
    see(figure(), 1);
    expect(filmIs()).toBe('paused');
    expect(moment()).toBeCloseTo(STILL, -1);
  });

  it('end a crossing at once when motion is reduced', () => {
    mountPictures();
    see(figure(), 1);
    segment(4).click();
    expect(crossing()).toBe(true);
    setMotion(true);
    expect(crossing()).toBe(false);
    expect(root().film!.moving).toBe(false);
    expect(moment()).toBeCloseTo(endOf(TIMED[4]), -1);
  });

  it.each(['before Play', 'while it plays', 'since the page opened'])('jump with no crossfade under reduced motion, %s', (when) => {
    reduced = when !== 'since the page opened';
    mountPictures();
    if (!reduced) setMotion(true);
    see(figure(), 1);
    if (when === 'while it plays') $('.film-pause').click();
    segment(5).click();
    expect(crossing()).toBe(false);
    expect(moment()).toBeCloseTo(when === 'while it plays' ? TIMED[5].start : endOf(TIMED[5]), -1);
    expect($('.f-hl').getAttribute('transform')).not.toBe('translate(0.00 134.00)');
  });

  it('stop the card that plays at once when motion is reduced, and take turns again once the visitor starts one', () => {
    mountPictures();
    see(card('rules'), 1);
    see(card('secrets'), 1);
    expect(playing()).toEqual(['rules']);
    setMotion(true);
    expect(cardsMove()).toEqual([]);
    setMotion(false);
    vi.advanceTimersByTime(10000);
    expect(playing()).toEqual([]);
    card('rules').click();
    expect(playing()).toEqual(['rules']);
    expect(turnsOver(6000)).toEqual(['rules', 'secrets']);
  });

  it.each(['on its turn', 'pressed', 'pointed at'])('stop a card %s at once when Play is pressed', (how) => {
    mountPictures();
    see(figure(), 1);
    $('.film-pause').click();
    if (how === 'on its turn') see(card('record'), 1);
    else if (how === 'pressed') card('record').click();
    else pointer(card('record'), 'pointerenter', 'mouse');
    vi.advanceTimersByTime(1000);
    expect(playing()).toEqual(['record']);
    $('.film-pause').click();
    expect(filmIs()).toBe('moving');
    expect(cardsMove()).toEqual([]);
  });

  it.each(['on its turn', 'pressed'])('stop a card %s at once for a jump, and start none until it has crossed over', (how) => {
    mountPictures();
    see(figure(), 1);
    $('.film-pause').click();
    see(card('rules'), 1);
    if (how === 'pressed') card('record').click();
    vi.advanceTimersByTime(1000);
    segment(3).click();
    expect(crossing()).toBe(true);
    expect(cardsMove()).toEqual([]);
    vi.advanceTimersByTime(100);
    expect(cardsMove()).toEqual([]);
    vi.advanceTimersByTime(100);
    expect(crossing()).toBe(false);
    expect(playing()).toEqual(['rules']);
  });

  it('end a crossing at once for a card the visitor starts', () => {
    mountPictures();
    see(figure(), 1);
    segment(3).click();
    expect(crossing()).toBe(true);
    card('record').click();
    expect(crossing()).toBe(false);
    expect(playing()).toEqual(['record']);
  });

  it('drop a dissolve that had begun as the film starts', () => {
    mountPictures();
    see(card('rules'), 1);
    vi.advanceTimersByTime(lasts('rules') + FADE / 2);
    expect(cardsMove().length).toBeGreaterThan(0);
    see(figure(), 1);
    expect(cardsMove()).toEqual([]);

    pointer(card('record'), 'pointerenter', 'mouse');
    vi.advanceTimersByTime(1000);
    pointer(card('record'), 'pointerleave', 'mouse');
    expect(filmIs()).toBe('moving');
    expect(cardsMove()).toEqual([]);
  });

});
