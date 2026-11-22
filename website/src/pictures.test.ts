import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { LENGTH, STILL, TIMED, endOf } from './film/story';
import { homePage, measurableSvg } from './film/test-support';
import { mountPictures } from './pictures';

// The page as it is served, with what a browser gives and jsdom lacks: nothing is in view until a test says so.
const page = new DOMParser().parseFromString(homePage(), 'text/html').body.innerHTML;

type Seen = (entries: { target: Element; intersectionRatio: number }[]) => void;
let watchers: { seen: Seen; targets: Element[] }[] = [];
let reduced = false;
let visibility = 'visible';

/** Puts `target` in view by `ratio`, as its observer would report it. */
const see = (target: Element, ratio: number) =>
  watchers.filter((watcher) => watcher.targets.includes(target)).forEach((watcher) => watcher.seen([{ target, intersectionRatio: ratio }]));
const NAMES = ['full', 'screen', 'edge', 'record', 'needs', 'back'];
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
const card = (name: string) => $(`.card[data-mini="${name}"]`);
const playing = () => [...document.querySelectorAll<HTMLElement>('.card.playing')].map((el) => el.dataset.mini);
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
  vi.stubGlobal('matchMedia', (query: string) => ({ matches: query.includes('reduced-motion') && reduced }));
  Element.prototype.animate = (() => ({})) as unknown as Element['animate'];
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
    expect($('#cards').classList.contains('live')).toBe(true);
    expect(moment()).toBeCloseTo(STILL, -1);
    // The first card's last moment: the three commands are typed and checked.
    expect(card('full').querySelectorAll('.ty.on')).toHaveLength(3);
    expect(card('full').querySelectorAll('.ok.on')).toHaveLength(3);
    // No pointer is part of a still.
    expect(document.querySelectorAll('.mv.on')).toHaveLength(0);
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
    see(card('full'), 1);
    see(card('screen'), 1);
    expect(filmIs()).toBe('held');
    expect(playing()).toEqual(['full']);

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
    expect(playing()).toEqual(['screen']);
  });

  it('give the turn to the next card in view, and to none that is out of view', () => {
    mountPictures();
    see(card('full'), 1);
    see(card('edge'), 0.6);
    see(card('needs'), 0.2);
    expect(turnsOver(16000)).toEqual(['full', 'edge', 'full', 'edge']);
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
    see(card('full'), 1);
    see(card('screen'), 1);
    pointer(card('edge'), 'pointerenter', 'mouse');
    expect(playing()).toEqual(['edge']);
    // The card ends, the pointer stays: no other card starts.
    vi.advanceTimersByTime(10000);
    expect(playing()).toEqual([]);
    pointer(card('edge'), 'pointerleave', 'mouse');
    vi.advanceTimersByTime(900);
    expect(turnsOver(10000)).toEqual(['full', 'screen', 'full']);
  });

  it('play the card the visitor points at and hold the film until they leave it', () => {
    mountPictures();
    see(figure(), 1);
    vi.advanceTimersByTime(4000);
    pointer(card('edge'), 'pointerenter', 'mouse');
    expect(playing()).toEqual(['edge']);
    expect(filmIs()).toBe('held');
    // The card ends, the pointer stays: the film still waits.
    vi.advanceTimersByTime(10000);
    expect(playing()).toEqual([]);
    expect(filmIs()).toBe('held');
    expect(shot()).toBe(0);
    pointer(card('edge'), 'pointerleave', 'mouse');
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
    pointer(card('edge'), 'pointerenter', 'mouse');
    pointer(card('edge'), 'pointerleave', 'mouse');
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
    vi.advanceTimersByTime(3700 + 600);
    expect(playing()).toEqual([]);
    expect(filmIs()).toBe('moving');
  });

  it('keep taking turns after the pointer leaves a card, once the pause after it is over', () => {
    mountPictures();
    see(card('full'), 1);
    see(card('screen'), 1);
    see(card('edge'), 1);
    pointer(card('full'), 'pointerenter', 'mouse');
    vi.advanceTimersByTime(1000);
    pointer(card('full'), 'pointerleave', 'mouse');
    expect(playing()).toEqual([]);
    vi.advanceTimersByTime(899);
    expect(playing()).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(turnsOver(15000)).toEqual(['screen', 'edge', 'full', 'screen']);
  });

  it('keep taking turns after a pressed card ends, once the pause between two turns is over', () => {
    mountPictures();
    see(card('full'), 1);
    see(card('record'), 1);
    card('record').click();
    expect(playing()).toEqual(['record']);
    vi.advanceTimersByTime(3700 + 600);
    expect(playing()).toEqual([]);
    vi.advanceTimersByTime(399);
    expect(playing()).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(turnsOver(10000)).toEqual(['full', 'record', 'full']);
  });

  it('never leave a card playing for longer than its picture lasts', () => {
    mountPictures();
    // How long each card plays, on the fake clock, taken at the moments it starts and stops (the page adds and removes
    // `playing`) and never by looking now and then. `--dur` is what its picture and its linger last; a stay may not pass it.
    let overstay = -Infinity;
    const startedAt = new Map<string, { at: number; dur: number }>();
    for (const name of NAMES) {
      const list = card(name).classList;
      const add = list.add.bind(list);
      const remove = list.remove.bind(list);
      const toggle = list.toggle.bind(list);
      list.add = (...tokens: string[]) => {
        if (tokens.includes('playing') && !list.contains('playing')) startedAt.set(name, { at: Date.now(), dur: parseFloat(card(name).style.getPropertyValue('--dur')) });
        add(...tokens);
      };
      list.remove = (...tokens: string[]) => {
        const start = startedAt.get(name);
        if (tokens.includes('playing') && start && list.contains('playing')) {
          overstay = Math.max(overstay, Date.now() - start.at - start.dur);
          startedAt.delete(name);
        }
        remove(...tokens);
      };
      // The page adds and removes the class today. Should it toggle it one day, the same two paths measure it.
      list.toggle = (token: string, force?: boolean) => {
        const on = force ?? !list.contains(token);
        if (token !== 'playing') return toggle(token, force);
        if (on) list.add(token); else list.remove(token);
        return on;
      };
    }
    for (const name of NAMES) see(card(name), 1);
    // A visitor's hand in every order, between turns: whatever happened, a minute later the cards still take turns.
    const hands = [
      () => pointer(card('edge'), 'pointerenter', 'mouse'),
      () => pointer(card('edge'), 'pointerleave', 'mouse'),
      () => card('needs').click(),
      () => see(figure(), 1),
      () => see(figure(), 0),
      () => show('hidden'),
      () => show('visible'),
      () => $('.film-pause').click(),
    ];
    // A seeded generator (Park-Miller), the same hands on every run; the choices come from its whole value, as
    // its low bits repeat every few calls and would leave most hands unused. Each hand that starts a card draws
    // it in jsdom, which costs about 20 ms, so a hundred rounds keep the test well under its limit on a busy machine.
    let seed = 7;
    const pick = (below: number) => Math.floor(((seed = (seed * 48271) % 2147483647) / 2147483647) * below);
    for (let round = 0; round < 100; round++) {
      hands[pick(hands.length)]();
      vi.advanceTimersByTime(pick(2500));
      expect(playing().length).toBeLessThanOrEqual(1);
    }
    // Back to rest: pointer away, tab shown, film out of view.
    pointer(card('edge'), 'pointerleave', 'mouse');
    show('visible');
    see(figure(), 0);
    vi.advanceTimersByTime(10000);
    // A minute of turns: every card takes one.
    expect(new Set(turnsOver(60000)).size).toBe(NAMES.length);
    // No stay passed its card's time, by not even a millisecond; one still going is measured up to now.
    for (const [, start] of startedAt) overstay = Math.max(overstay, Date.now() - start.at - start.dur);
    expect(overstay).toBeLessThanOrEqual(0);
    // The test saw stays end, so the check above was made on something.
    expect(overstay).toBeGreaterThan(-Infinity);
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
    see(card('full'), 1);
    expect(playing()).toEqual(['full']);
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

  it('move nothing on their own with reduced motion, until the visitor presses Play or a card', () => {
    reduced = true;
    mountPictures();
    see(figure(), 1);
    see(card('full'), 1);
    expect(filmIs()).toBe('paused');
    expect($('.film-pause').textContent).toBe('Play');
    expect(moment()).toBeCloseTo(STILL, -1);
    vi.advanceTimersByTime(60000);
    expect(filmIs()).toBe('paused');
    expect(moment()).toBeCloseTo(STILL, -1);
    expect(playing()).toEqual([]);

    pointer(card('full'), 'pointerenter', 'mouse');
    expect(playing()).toEqual([]);
    card('full').click();
    expect(playing()).toEqual(['full']);
    vi.advanceTimersByTime(10000);
    expect(playing()).toEqual([]);

    // Play goes on from the still.
    $('.film-pause').click();
    expect(filmIs()).toBe('moving');
    vi.advanceTimersByTime(1000);
    expect(moment()).toBeGreaterThan(STILL + 700);
    see(figure(), 0);
    vi.advanceTimersByTime(60000);
    expect(playing()).toEqual([]);
  });
});
