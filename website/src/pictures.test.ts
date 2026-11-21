import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { END, STEPS, STILL_STEP, endOf, film } from './flow';
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
const flow = () => $('#flow');
const stage = () => $('.stage');
/** A number the film wrote on a part of its stage. */
const drawn = (selector: string, name: string) => Number($(`.stage ${selector}`).style.getPropertyValue(`--${name}`));
const step = () => [...document.querySelectorAll('.steps li')].findIndex((li) => li.classList.contains('on'));
/** Moving, held by the page, or paused by the visitor. */
const flowIs = () => (flow().classList.contains('paused') ? 'paused' : flow().classList.contains('held') ? 'held' : 'moving');
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
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'requestAnimationFrame', 'cancelAnimationFrame', 'performance', 'Date'] });
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
  it('show once their script has drawn them: the flow at its still, every card at its end', () => {
    mountPictures();
    expect(document.documentElement.classList.contains('pictures-pending')).toBe(false);
    expect(flow().classList.contains('live')).toBe(true);
    expect($('#cards').classList.contains('live')).toBe(true);
    expect(step()).toBe(STILL_STEP);
    expect(drawn('.scene', 'o')).toBe(1);
    expect(drawn('.cross', 'o')).toBe(1);
    // The first card's last moment: the three commands are typed and checked.
    expect(card('full').querySelectorAll('.ty.on')).toHaveLength(3);
    expect(card('full').querySelectorAll('.ok.on')).toHaveLength(3);
    // No pointer is part of a still.
    expect(document.querySelectorAll('.mv.on')).toHaveLength(0);
  });

  it('show the still until the flow first moves, then play it from its start', () => {
    // The still as the page shows it without script: a stage of the served markup, held where flow.css puts it.
    const copy = document.createElement('div');
    copy.innerHTML = stage().outerHTML;
    film(copy.firstElementChild as HTMLElement).hold(endOf(STILL_STEP));
    mountPictures();
    // Out of view, the flow is held: the stage is the still.
    expect(stage().innerHTML).toBe(copy.firstElementChild!.innerHTML);
    vi.advanceTimersByTime(5000);
    expect(stage().innerHTML).toBe(copy.firstElementChild!.innerHTML);
    // In view, it plays from moment 0.
    see(stage(), 1);
    expect(flowIs()).toBe('moving');
    expect(step()).toBe(0);
    expect(drawn('.scene', 'o')).toBe(0);
    expect(drawn('.a1', 'o')).toBe(0);
    vi.advanceTimersByTime(2000);
    expect(step()).toBe(0);
    expect(drawn('.a1', 'o')).toBe(1);
  });

  it('name in the step row the step the stage shows, even when a frame comes after the step has started', () => {
    mountPictures();
    see(stage(), 1);
    // Frames come every 16 ms. Go back starts at 22600, between the frames at 22592 and 22608.
    expect(STEPS[6] % 16).not.toBe(0);
    vi.advanceTimersByTime(STEPS[6]);
    // The last frame drawn is still in Cost: the row names Cost.
    expect(step()).toBe(5);
    expect(drawn('.cp-name', 'o')).toBe(0);
    vi.advanceTimersByTime(8);
    // The frame at 22608 is in Go back: the row names it, and its bar starts 8 ms in.
    const late = Math.ceil(STEPS[6] / 16) * 16 - STEPS[6];
    expect(step()).toBe(6);
    expect(document.querySelectorAll('.steps button')[6].getAttribute('aria-current')).toBe('step');
    expect(document.querySelectorAll('.steps button')[5].hasAttribute('aria-current')).toBe(false);
    expect(document.querySelectorAll<HTMLElement>('.steps li')[6].style.getPropertyValue('--skip')).toBe(`${-late}ms`);
    // Pause pressed now shows the end of the step the stage shows.
    $('.ctl').click();
    expect(step()).toBe(6);
    expect(drawn('.cp-name', 'o')).toBe(1);
  });

  it('move one at a time: the flow while 30% of it is in view, the cards only while the flow is not playing', () => {
    mountPictures();
    see(card('full'), 1);
    see(card('screen'), 1);
    expect(flowIs()).toBe('held');
    expect(playing()).toEqual(['full']);

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
    expect(playing()).toEqual(['screen']);
  });

  it('give the turn to the next card in view, and to none that is out of view', () => {
    mountPictures();
    see(card('full'), 1);
    see(card('edge'), 0.6);
    see(card('needs'), 0.2);
    expect(turnsOver(16000)).toEqual(['full', 'edge', 'full', 'edge']);
  });

  it('hold the flow at its moment when it leaves the view, and carry on from there', () => {
    mountPictures();
    see(stage(), 1);
    vi.advanceTimersByTime(4000);
    expect(step()).toBe(1);
    // The first command is being typed.
    const typed = drawn('.a1 .cmd', 'n');
    expect(typed).toBeGreaterThan(0);
    expect(typed).toBeLessThan(30);
    see(stage(), 0);
    vi.advanceTimersByTime(60000);
    expect(step()).toBe(1);
    expect(drawn('.a1 .cmd', 'n')).toBe(typed);
    see(stage(), 1);
    vi.advanceTimersByTime(STEPS[2] - 4000 - 1);
    expect(step()).toBe(1);
    vi.advanceTimersByTime(1);
    expect(step()).toBe(2);
  });

  it('play the flow in a loop', () => {
    mountPictures();
    see(stage(), 1);
    vi.advanceTimersByTime(END - 1);
    expect(step()).toBe(STEPS.length - 1);
    // The first frame at or after the end starts the film again.
    vi.advanceTimersByTime(16);
    expect(step()).toBe(0);
    expect(flowIs()).toBe('moving');
    vi.advanceTimersByTime(2000);
    expect(drawn('.a1', 'o')).toBe(1);
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

  it('play the card the visitor points at and hold the flow until they leave it', () => {
    mountPictures();
    see(stage(), 1);
    vi.advanceTimersByTime(4000);
    pointer(card('edge'), 'pointerenter', 'mouse');
    expect(playing()).toEqual(['edge']);
    expect(flowIs()).toBe('held');
    // The card ends, the pointer stays: the flow still waits.
    vi.advanceTimersByTime(10000);
    expect(playing()).toEqual([]);
    expect(flowIs()).toBe('held');
    expect(step()).toBe(1);
    pointer(card('edge'), 'pointerleave', 'mouse');
    expect(flowIs()).toBe('moving');
    vi.advanceTimersByTime(5000);
    expect(playing()).toEqual([]);
  });

  it('leave the flow paused after a card, when the visitor paused it', () => {
    mountPictures();
    see(stage(), 1);
    vi.advanceTimersByTime(4000);
    $('.ctl').click();
    expect(flowIs()).toBe('paused');
    expect($('.ctl').textContent).toBe('Play');
    pointer(card('edge'), 'pointerenter', 'mouse');
    pointer(card('edge'), 'pointerleave', 'mouse');
    vi.advanceTimersByTime(30000);
    expect(flowIs()).toBe('paused');
    expect(step()).toBe(1);
  });

  it('take a touch as a press, not as pointing: the card plays once, then the flow carries on', () => {
    mountPictures();
    see(stage(), 1);
    pointer(card('record'), 'pointerenter', 'touch');
    expect(playing()).toEqual([]);
    expect(flowIs()).toBe('moving');
    card('record').click();
    expect(playing()).toEqual(['record']);
    expect(flowIs()).toBe('held');
    vi.advanceTimersByTime(3700 + 600);
    expect(playing()).toEqual([]);
    expect(flowIs()).toBe('moving');
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
      () => see(stage(), 1),
      () => see(stage(), 0),
      () => show('hidden'),
      () => show('visible'),
      () => $('.ctl').click(),
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
    // Back to rest: pointer away, tab shown, flow out of view.
    pointer(card('edge'), 'pointerleave', 'mouse');
    show('visible');
    see(stage(), 0);
    vi.advanceTimersByTime(10000);
    // A minute of turns, looked at every 100 ms: every card takes one, and none stays beyond the time its card says
    // (`--dur`, its picture and the moment it lingers) by more than one look.
    const cards = new Set<string>();
    let started = 0;
    let late = 0;
    let before: string | undefined;
    for (let passed = 0; passed <= 60000; passed += 100) {
      const name = playing()[0];
      if (name) {
        cards.add(name);
        if (name !== before) started = Date.now();
        late = Math.max(late, Date.now() - started - parseFloat(card(name).style.getPropertyValue('--dur')));
      }
      before = name;
      vi.advanceTimersByTime(100);
    }
    expect(cards.size).toBe(NAMES.length);
    expect(late).toBeLessThanOrEqual(100);
  });

  it('pause at the end of the step that was playing, and go on with the next step', () => {
    mountPictures();
    see(stage(), 1);
    vi.advanceTimersByTime(4000);
    expect(drawn('.a3 .ok', 'd')).toBe(0);
    $('.ctl').click();
    // The end of Full access: the three commands are typed and checked.
    expect(drawn('.a3 .cmd', 'n')).toBe(11);
    expect(drawn('.a3 .ok', 'd')).toBe(1);
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
    expect(drawn('.n1', 'o')).toBe(0);
    expect(steps[4].getAttribute('aria-current')).toBe('step');
    $('.ctl').click();
    steps[5].click();
    expect(step()).toBe(5);
    // The end of Cost: the bar of the costliest line is whole.
    expect(drawn('.o1 .bar', 'w')).toBe(836);
    expect(steps[4].hasAttribute('aria-current')).toBe(false);
  });

  it('hold everything while the tab is hidden, and carry on when it shows again', () => {
    mountPictures();
    see(stage(), 1);
    vi.advanceTimersByTime(4000);
    show('hidden');
    expect(flowIs()).toBe('held');
    vi.advanceTimersByTime(60000);
    expect(step()).toBe(1);
    show('visible');
    expect(flowIs()).toBe('moving');

    see(stage(), 0);
    see(card('full'), 1);
    expect(playing()).toEqual(['full']);
    show('hidden');
    expect(playing()).toEqual([]);
    vi.advanceTimersByTime(60000);
    expect(playing()).toEqual([]);
    show('visible');
    expect(playing()).toHaveLength(1);
  });

  it('run nothing for the flow while it is held: no frame and no timer', () => {
    mountPictures();
    see(stage(), 1);
    vi.advanceTimersByTime(4000);
    see(stage(), 0);
    // The cards do not take turns either: none is in view.
    expect(vi.getTimerCount()).toBe(0);
  });

  it('move nothing on their own with reduced motion, until the visitor presses Play or a card', () => {
    reduced = true;
    mountPictures();
    see(stage(), 1);
    see(card('full'), 1);
    expect(flowIs()).toBe('paused');
    expect($('.ctl').textContent).toBe('Play');
    expect(step()).toBe(STILL_STEP);
    // The last moment of Boundary: crates.io was reached, and the request to paste.rs is stopped at the edge.
    expect(drawn('.h2 .hdot', 'got')).toBe(1);
    expect(drawn('.cross', 'o')).toBe(1);
    vi.advanceTimersByTime(60000);
    expect(flowIs()).toBe('paused');
    expect(step()).toBe(STILL_STEP);
    expect(playing()).toEqual([]);

    pointer(card('full'), 'pointerenter', 'mouse');
    expect(playing()).toEqual([]);
    card('full').click();
    expect(playing()).toEqual(['full']);
    vi.advanceTimersByTime(10000);
    expect(playing()).toEqual([]);

    // Play starts the flow from its start.
    $('.ctl').click();
    expect(flowIs()).toBe('moving');
    expect(step()).toBe(0);
    expect(drawn('.scene', 'o')).toBe(0);
    see(stage(), 0);
    vi.advanceTimersByTime(60000);
    expect(playing()).toEqual([]);
  });
});
