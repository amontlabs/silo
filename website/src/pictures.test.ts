import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { END, STEPS, STILL_STEP } from './flow';
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
const $ = (selector: string) => document.querySelector<HTMLElement>(selector)!;
const flow = () => $('#flow');
const stage = () => $('.stage');
const card = (name: string) => $(`.card[data-mini="${name}"]`);
const playing = () => [...document.querySelectorAll<HTMLElement>('.card.playing')].map((el) => el.dataset.mini);
const step = () => [...document.querySelectorAll('.steps li')].findIndex((li) => li.classList.contains('on'));
/** Moving, held by the page, or paused by the visitor. */
const flowIs = () => (flow().classList.contains('paused') ? 'paused' : flow().classList.contains('held') ? 'held' : 'moving');
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
  vi.stubGlobal(
    'ResizeObserver',
    class {
      observe() {}
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

describe('the two pictures', () => {
  it('show once their script has drawn them: the flow at its start, every card at its end', () => {
    mountPictures();
    expect(document.documentElement.classList.contains('pictures-pending')).toBe(false);
    expect(flow().classList.contains('live')).toBe(true);
    expect($('#cards').classList.contains('live')).toBe(true);
    expect(step()).toBe(0);
    expect($('.stage .fa').classList.contains('on')).toBe(false);
    // The recipe card's last moment: the three computers are there.
    expect(card('rec').querySelectorAll('.p.on')).toHaveLength(3);
  });

  it('move one at a time: the flow while 30% of it is in view, the cards only while the flow is not playing', () => {
    mountPictures();
    see(card('rec'), 1);
    see(card('tools'), 1);
    expect(flowIs()).toBe('held');
    expect(playing()).toEqual(['rec']);

    see(stage(), 0.29);
    expect(flowIs()).toBe('held');
    see(stage(), 0.3);
    expect(flowIs()).toBe('moving');
    expect($('.ctl').textContent).toBe('Pause');
    expect(playing()).toEqual([]);
    vi.advanceTimersByTime(20000);
    expect(playing()).toEqual([]);

    see(stage(), 0);
    expect(flowIs()).toBe('held');
    expect(playing()).toEqual(['tools']);
  });

  it('give the turn to the next card in view, and to none that is out of view', () => {
    mountPictures();
    see(card('rec'), 1);
    see(card('keys'), 0.6);
    see(card('net'), 0.2);
    expect(turnsOver(16000)).toEqual(['rec', 'keys', 'rec', 'keys']);
  });

  it('hold the flow at its moment when it leaves the view, and carry on from there', () => {
    mountPictures();
    see(stage(), 1);
    vi.advanceTimersByTime(3000);
    expect(step()).toBe(1);
    see(stage(), 0);
    vi.advanceTimersByTime(60000);
    expect(step()).toBe(1);
    expect($('.stage .fa .t1').classList.contains('on')).toBe(true);
    see(stage(), 1);
    vi.advanceTimersByTime(STEPS[2] - 3000 - 1);
    expect(step()).toBe(1);
    vi.advanceTimersByTime(1);
    expect(step()).toBe(2);
  });

  it('play the flow in a loop', () => {
    mountPictures();
    see(stage(), 1);
    vi.advanceTimersByTime(END - 1);
    expect(step()).toBe(STEPS.length - 1);
    vi.advanceTimersByTime(1);
    expect(step()).toBe(0);
    expect(flowIs()).toBe('moving');
  });

  it('play the card the visitor points at and hold the flow until they leave it', () => {
    mountPictures();
    see(stage(), 1);
    vi.advanceTimersByTime(3000);
    pointer(card('keys'), 'pointerenter', 'mouse');
    expect(playing()).toEqual(['keys']);
    expect(flowIs()).toBe('held');
    // The card ends, the pointer stays: the flow still waits.
    vi.advanceTimersByTime(10000);
    expect(playing()).toEqual([]);
    expect(flowIs()).toBe('held');
    expect(step()).toBe(1);
    pointer(card('keys'), 'pointerleave', 'mouse');
    expect(flowIs()).toBe('moving');
    vi.advanceTimersByTime(5000);
    expect(playing()).toEqual([]);
  });

  it('leave the flow paused after a card, when the visitor paused it', () => {
    mountPictures();
    see(stage(), 1);
    vi.advanceTimersByTime(3000);
    $('.ctl').click();
    expect(flowIs()).toBe('paused');
    expect($('.ctl').textContent).toBe('Play');
    pointer(card('keys'), 'pointerenter', 'mouse');
    pointer(card('keys'), 'pointerleave', 'mouse');
    vi.advanceTimersByTime(30000);
    expect(flowIs()).toBe('paused');
    expect(step()).toBe(1);
  });

  it('take a touch as a press, not as pointing: the card plays once, then the flow carries on', () => {
    mountPictures();
    see(stage(), 1);
    pointer(card('dif'), 'pointerenter', 'touch');
    expect(playing()).toEqual([]);
    expect(flowIs()).toBe('moving');
    card('dif').click();
    expect(playing()).toEqual(['dif']);
    expect(flowIs()).toBe('held');
    vi.advanceTimersByTime(3300 + 600);
    expect(playing()).toEqual([]);
    expect(flowIs()).toBe('moving');
  });

  it('keep taking turns after the pointer leaves a card, once the pause after it is over', () => {
    mountPictures();
    see(card('rec'), 1);
    see(card('tools'), 1);
    see(card('keys'), 1);
    pointer(card('rec'), 'pointerenter', 'mouse');
    vi.advanceTimersByTime(1000);
    pointer(card('rec'), 'pointerleave', 'mouse');
    expect(playing()).toEqual([]);
    vi.advanceTimersByTime(899);
    expect(playing()).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(turnsOver(15000)).toEqual(['tools', 'keys', 'rec', 'tools']);
  });

  it('keep taking turns after a pressed card ends, once the pause between two turns is over', () => {
    mountPictures();
    see(card('rec'), 1);
    see(card('dif'), 1);
    card('dif').click();
    expect(playing()).toEqual(['dif']);
    vi.advanceTimersByTime(3300 + 600);
    expect(playing()).toEqual([]);
    vi.advanceTimersByTime(399);
    expect(playing()).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(turnsOver(9000)).toEqual(['rec', 'dif', 'rec']);
  });

  it('never leave a card playing for longer than its picture lasts', () => {
    mountPictures();
    for (const name of ['rec', 'tools', 'keys', 'net', 'dif', 'wait', 'steps', 'far', 'scr']) see(card(name), 1);
    // A visitor's hand in every order, between turns: whatever happened, a minute later the cards still take turns.
    const hands = [
      () => pointer(card('net'), 'pointerenter', 'mouse'),
      () => pointer(card('net'), 'pointerleave', 'mouse'),
      () => card('wait').click(),
      () => see(stage(), 1),
      () => see(stage(), 0),
      () => show('hidden'),
      () => show('visible'),
      () => $('.ctl').click(),
    ];
    let seed = 7;
    const pick = () => (seed = (seed * 1103515245 + 12345) % 2147483648);
    for (let round = 0; round < 300; round++) {
      hands[pick() % hands.length]();
      vi.advanceTimersByTime(pick() % 2500);
      expect(playing().length).toBeLessThanOrEqual(1);
    }
    // Back to rest: pointer away, tab shown, flow out of view.
    pointer(card('net'), 'pointerleave', 'mouse');
    show('visible');
    see(stage(), 0);
    vi.advanceTimersByTime(10000);
    expect(new Set(turnsOver(60000)).size).toBe(9);
  });

  it('pause at the end of the step that was playing, and go on with the next step', () => {
    mountPictures();
    see(stage(), 1);
    vi.advanceTimersByTime(3000);
    $('.ctl').click();
    expect($('.stage .fa .t4').classList.contains('on')).toBe(true);
    $('.ctl').click();
    expect(step()).toBe(2);
    expect(flowIs()).toBe('moving');
  });

  it('jump to a step by its name: its start while playing, its end while paused', () => {
    mountPictures();
    see(stage(), 1);
    const steps = [...document.querySelectorAll<HTMLButtonElement>('.steps button')];
    steps[4].click();
    expect(step()).toBe(4);
    expect($('.stage .fa .d1').classList.contains('on')).toBe(false);
    expect(steps[4].getAttribute('aria-current')).toBe('step');
    $('.ctl').click();
    steps[5].click();
    expect(step()).toBe(5);
    expect($('.stage .fa').classList.contains('merged')).toBe(true);
    expect($('.stage .pg').textContent).toBe('postgresql 17');
    expect(steps[4].hasAttribute('aria-current')).toBe(false);
  });

  it('hold everything while the tab is hidden, and carry on when it shows again', () => {
    mountPictures();
    see(stage(), 1);
    vi.advanceTimersByTime(3000);
    show('hidden');
    expect(flowIs()).toBe('held');
    vi.advanceTimersByTime(60000);
    expect(step()).toBe(1);
    show('visible');
    expect(flowIs()).toBe('moving');

    see(stage(), 0);
    see(card('rec'), 1);
    expect(playing()).toEqual(['rec']);
    show('hidden');
    expect(playing()).toEqual([]);
    vi.advanceTimersByTime(60000);
    expect(playing()).toEqual([]);
    show('visible');
    expect(playing()).toHaveLength(1);
  });

  it('move nothing on their own with reduced motion, until the visitor presses Play or a card', () => {
    reduced = true;
    mountPictures();
    see(stage(), 1);
    see(card('rec'), 1);
    expect(flowIs()).toBe('paused');
    expect($('.ctl').textContent).toBe('Play');
    expect(step()).toBe(STILL_STEP);
    // Both forks show what they changed, and wait for the decision.
    expect($('.stage .fa').classList.contains('decide')).toBe(true);
    expect($('.stage .fb .d2').classList.contains('on')).toBe(true);
    vi.advanceTimersByTime(60000);
    expect(flowIs()).toBe('paused');
    expect(step()).toBe(STILL_STEP);
    expect(playing()).toEqual([]);

    pointer(card('rec'), 'pointerenter', 'mouse');
    expect(playing()).toEqual([]);
    card('rec').click();
    expect(playing()).toEqual(['rec']);
    vi.advanceTimersByTime(10000);
    expect(playing()).toEqual([]);

    $('.ctl').click();
    expect(flowIs()).toBe('moving');
    expect(step()).toBe(STILL_STEP + 1);
    see(stage(), 0);
    vi.advanceTimersByTime(60000);
    expect(playing()).toEqual([]);
  });
});
