import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { mountPictures } from './pictures';

// The page as it is served, with what a browser gives and jsdom lacks: nothing is in view until a test says so.
const page = new DOMParser().parseFromString(readFileSync(join(process.cwd(), 'index.html'), 'utf8'), 'text/html').body.innerHTML;

type Seen = (entries: { target: Element; intersectionRatio: number }[]) => void;
let watchers: { seen: Seen; targets: Element[] }[] = [];
let reduced = false;
let visibility = 'visible';

/** Puts `target` in view by `ratio`, as its observer would report it. */
const see = (target: Element, ratio: number) =>
  watchers.filter((watcher) => watcher.targets.includes(target)).forEach((watcher) => watcher.seen([{ target, intersectionRatio: ratio }]));
const NAMES = ['full', 'screen', 'edge', 'record', 'needs', 'back'];
const $ = (selector: string) => document.querySelector<HTMLElement>(selector)!;
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
  vi.stubGlobal('matchMedia', () => ({ matches: reduced }));
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

describe('the pictures', () => {
  it('show once their script has drawn them: every card at its end', () => {
    mountPictures();
    expect(document.documentElement.classList.contains('pictures-pending')).toBe(false);
    expect($('#cards').classList.contains('live')).toBe(true);
    // The first card's last moment: the three commands are typed and checked.
    expect(card('full').querySelectorAll('.ty.on')).toHaveLength(3);
    expect(card('full').querySelectorAll('.ok.on')).toHaveLength(3);
    // No pointer is part of a still.
    expect(document.querySelectorAll('.mv.on')).toHaveLength(0);
  });

  it('give the turn to the next card in view, and to none that is out of view', () => {
    mountPictures();
    see(card('full'), 1);
    see(card('edge'), 0.6);
    see(card('needs'), 0.2);
    expect(turnsOver(16000)).toEqual(['full', 'edge', 'full', 'edge']);
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

  it('take a touch as a press, not as pointing: the card plays once', () => {
    mountPictures();
    pointer(card('record'), 'pointerenter', 'touch');
    expect(playing()).toEqual([]);
    card('record').click();
    expect(playing()).toEqual(['record']);
    vi.advanceTimersByTime(3700 + 600);
    expect(playing()).toEqual([]);
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
    for (const name of NAMES) see(card(name), 1);
    // A visitor's hand in every order, between turns: whatever happened, a minute later the cards still take turns.
    const hands = [
      () => pointer(card('edge'), 'pointerenter', 'mouse'),
      () => pointer(card('edge'), 'pointerleave', 'mouse'),
      () => card('needs').click(),
      () => show('hidden'),
      () => show('visible'),
    ];
    let seed = 7;
    const pick = () => (seed = (seed * 1103515245 + 12345) % 2147483648);
    for (let round = 0; round < 300; round++) {
      hands[pick() % hands.length]();
      vi.advanceTimersByTime(pick() % 2500);
      expect(playing().length).toBeLessThanOrEqual(1);
    }
    // Back to rest: pointer away, tab shown.
    pointer(card('edge'), 'pointerleave', 'mouse');
    show('visible');
    vi.advanceTimersByTime(10000);
    expect(new Set(turnsOver(60000)).size).toBe(NAMES.length);
  });

  it('hold everything while the tab is hidden, and carry on when it shows again', () => {
    mountPictures();
    see(card('full'), 1);
    expect(playing()).toEqual(['full']);
    show('hidden');
    expect(playing()).toEqual([]);
    vi.advanceTimersByTime(60000);
    expect(playing()).toEqual([]);
    show('visible');
    expect(playing()).toHaveLength(1);
  });

  it('move nothing on their own with reduced motion, until the visitor presses a card', () => {
    reduced = true;
    mountPictures();
    see(card('full'), 1);
    vi.advanceTimersByTime(60000);
    expect(playing()).toEqual([]);

    pointer(card('full'), 'pointerenter', 'mouse');
    expect(playing()).toEqual([]);
    card('full').click();
    expect(playing()).toEqual(['full']);
    vi.advanceTimersByTime(10000);
    expect(playing()).toEqual([]);
  });
});
