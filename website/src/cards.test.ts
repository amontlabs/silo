import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { mountCards, type Watch } from './cards';

const page = new DOMParser().parseFromString(readFileSync(join(process.cwd(), 'index.html'), 'utf8'), 'text/html').body.innerHTML;

const card = (name: string) => document.querySelector<HTMLElement>(`.card[data-mini="${name}"]`)!;
const copies = (name: string) => [...card(name).querySelectorAll<SVGElement>('svg.ghost')];
const live = (name: string) => card(name).querySelector<SVGElement>('svg.mini:not(.ghost)')!;
const line = (name: string) => card(name).querySelector<HTMLElement>('.prog b')!;
const pointer = (target: Element, type: string) => {
  const event = new MouseEvent(type);
  Object.defineProperty(event, 'pointerType', { value: 'mouse' });
  target.dispatchEvent(event);
};

let seen: (index: number, inView: boolean) => void;
let animated: { target: Element; props: string[] }[] = [];
const watch: Watch = (_cards, report) => (seen = report);

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'performance', 'Date'] });
  document.body.innerHTML = page;
  animated = [];
  Element.prototype.animate = (() => ({})) as unknown as Element['animate'];
  // jsdom has no animations: the live picture reports the ones a test puts in `animated`.
  SVGElement.prototype.getAnimations = function () {
    return animated.map(({ target, props }) => ({
      effect: { target, getKeyframes: () => props.map((prop) => ({ offset: null, [prop]: '' })) },
    })) as unknown as Animation[];
  };
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const mount = (still = false) => mountCards(document.querySelector<HTMLElement>('#cards')!, { still, watch });

describe('the dissolve of a card picture', () => {
  it('makes no copy on mount, none at a natural end, and none when nothing was playing', () => {
    const cards = mount();
    expect(document.querySelectorAll('svg.ghost')).toHaveLength(0);
    seen(0, true);
    cards.turns(true);
    expect(document.querySelectorAll('svg.ghost')).toHaveLength(1);
    vi.advanceTimersByTime(250);
    expect(document.querySelectorAll('svg.ghost')).toHaveLength(0);
    // The card ends by itself: nothing changes in its picture, so nothing dissolves.
    vi.advanceTimersByTime(3500 + 600);
    expect(document.querySelectorAll('svg.ghost')).toHaveLength(0);
    // Nothing is playing: a stop has no look to dissolve.
    cards.rest();
    expect(document.querySelectorAll('svg.ghost')).toHaveLength(0);
  });

  it('lays a copy of the look before over the picture when a play starts, and removes it when its fade is over', () => {
    mount();
    const before = live('full').innerHTML;
    pointer(card('full'), 'pointerenter');
    const [copy] = copies('full');
    expect(copies('full')).toHaveLength(1);
    expect(copy.innerHTML).toBe(before);
    expect(copy.classList.contains('mini')).toBe(true);
    // The picture underneath is already at its first moment.
    expect(live('full').innerHTML).not.toBe(before);
    vi.advanceTimersByTime(249);
    expect(copies('full')).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(copies('full')).toHaveLength(0);
  });

  it('makes the copy right after the picture, not content: inert, hidden, with no id, its parts held still by `jump`', () => {
    mount();
    live('full').querySelector('.l1')!.id = 'part';
    pointer(card('full'), 'pointerenter');
    const [copy] = copies('full');
    expect(copy.getAttribute('aria-hidden')).toBe('true');
    expect(copy.hasAttribute('inert')).toBe(true);
    expect(copy.querySelector('[id]')).toBeNull();
    expect(copy.classList.contains('jump')).toBe(true);
    expect(live('full').nextElementSibling).toBe(copy);
  });

  it('carries the classes the picture had at the instant the pointer leaves, before the end', () => {
    mount();
    pointer(card('full'), 'pointerenter');
    vi.advanceTimersByTime(1000);
    const classes = [...live('full').querySelectorAll('.on')].map((part) => part.getAttribute('class'));
    expect(classes.length).toBeGreaterThan(0);
    pointer(card('full'), 'pointerleave');
    const [copy] = copies('full');
    expect([...copy.querySelectorAll('.on')].map((part) => part.getAttribute('class'))).toEqual(classes);
    // The live picture is at its end now: the three commands are typed.
    expect(live('full').querySelectorAll('.ty.on')).toHaveLength(3);
    vi.advanceTimersByTime(250);
    expect(copies('full')).toHaveLength(0);
  });

  it('writes the computed value of what is animating onto the copy', () => {
    mount();
    pointer(card('edge'), 'pointerenter');
    vi.advanceTimersByTime(1500);
    const dot = live('edge').querySelector('.p1')!;
    animated = [{ target: dot, props: ['transform'] }];
    const real = window.getComputedStyle.bind(window);
    vi.spyOn(window, 'getComputedStyle').mockImplementation(((el: Element) =>
      el === dot ? ({ transform: 'matrix(1, 0, 0, 1, 172, 70)' } as CSSStyleDeclaration) : real(el)) as typeof getComputedStyle);
    pointer(card('edge'), 'pointerleave');
    const [copy] = copies('edge');
    expect(copy.querySelector<SVGElement>('.p1')!.style.transform).toBe('matrix(1, 0, 0, 1, 172, 70)');
  });

  it('dissolves a card that another one replaces, and the one that starts', () => {
    mount();
    card('full').click();
    vi.advanceTimersByTime(250);
    card('screen').click();
    expect(copies('full')).toHaveLength(1);
    expect(copies('screen')).toHaveLength(1);
  });

  it('puts a newer copy under an older one, and keeps the older one fading', () => {
    mount();
    pointer(card('full'), 'pointerenter');
    const [older] = copies('full');
    vi.advanceTimersByTime(100);
    pointer(card('full'), 'pointerleave');
    const all = copies('full');
    expect(all).toHaveLength(2);
    expect(all[1]).toBe(older);
    expect(live('full').nextElementSibling).toBe(all[0]);
    // The older one goes first, then the newer one.
    vi.advanceTimersByTime(150);
    expect(copies('full')).toEqual([all[0]]);
    vi.advanceTimersByTime(100);
    expect(copies('full')).toHaveLength(0);
  });

  it('leaves no copy after a quick in and out, whatever the card did meanwhile', () => {
    mount();
    for (let k = 0; k < 6; k++) {
      pointer(card('full'), 'pointerenter');
      vi.advanceTimersByTime(20);
      pointer(card('full'), 'pointerleave');
      pointer(card('screen'), 'pointerenter');
      card('full').click();
      vi.advanceTimersByTime(20);
      pointer(card('screen'), 'pointerleave');
    }
    vi.advanceTimersByTime(250);
    expect(document.querySelectorAll('svg.ghost')).toHaveLength(0);
  });

  it('dissolves a play started again on a card that was playing, once', () => {
    mount();
    pointer(card('full'), 'pointerenter');
    vi.advanceTimersByTime(1000);
    card('full').click();
    // One copy: the look at the restart, not also the card's end.
    expect(copies('full')).toHaveLength(1);
  });

  it('dissolves a card on its turn that the film stops', () => {
    const cards = mount();
    seen(0, true);
    cards.turns(true);
    vi.advanceTimersByTime(1000);
    const classes = [...live('full').querySelectorAll('.on')].map((part) => part.getAttribute('class'));
    expect(classes.length).toBeGreaterThan(0);
    // The film starts playing: the cards may no longer take turns.
    cards.turns(false);
    expect(card('full').classList.contains('playing')).toBe(false);
    expect(copies('full')).toHaveLength(1);
    expect([...copies('full')[0].querySelectorAll('.on')].map((part) => part.getAttribute('class'))).toEqual(classes);
  });

  it('dissolves with reduced motion too: a pressed card, then the hidden tab that stops it', () => {
    const cards = mount(true);
    card('full').click();
    expect(copies('full')).toHaveLength(1);
    vi.advanceTimersByTime(250);
    expect(copies('full')).toHaveLength(0);
    vi.advanceTimersByTime(500);
    cards.rest();
    expect(copies('full')).toHaveLength(1);
  });

  it('holds the dissolve in its own constant, written for the css', () => {
    mount();
    expect(document.querySelector<HTMLElement>('#cards')!.style.getPropertyValue('--fade')).toBe('250ms');
  });
});

describe('the line over a card', () => {
  /** jsdom computes no transform: the line of `name` reports `transform` as the width it has. */
  const widthOf = (name: string, transform: string) => {
    const el = line(name);
    const real = window.getComputedStyle.bind(window);
    vi.spyOn(window, 'getComputedStyle').mockImplementation(((part: Element) =>
      part === el ? ({ transform } as CSSStyleDeclaration) : real(part)) as typeof getComputedStyle);
  };

  it('fades out from the width it had when the card stops or ends, and is gone when the fade is over', () => {
    mount();
    widthOf('full', 'matrix(0.4, 0, 0, 1, 0, 0)');
    pointer(card('full'), 'pointerenter');
    expect(card('full').classList.contains('playing')).toBe(true);
    vi.advanceTimersByTime(1000);
    pointer(card('full'), 'pointerleave');
    expect(card('full').classList.contains('playing')).toBe(false);
    // The width is kept on the line itself, since the animation that gave it is gone with `playing`.
    expect(line('full').style.transform).toBe('matrix(0.4, 0, 0, 1, 0, 0)');
    expect(line('full').classList.contains('ebb')).toBe(true);
    vi.advanceTimersByTime(249);
    expect(line('full').style.transform).toBe('matrix(0.4, 0, 0, 1, 0, 0)');
    vi.advanceTimersByTime(1);
    expect(line('full').classList.contains('ebb')).toBe(false);
    expect(line('full').style.transform).toBe('');

    card('screen').click();
    vi.advanceTimersByTime(3500 + 600);
    expect(card('screen').classList.contains('playing')).toBe(false);
    expect(line('screen').classList.contains('ebb')).toBe(true);
    vi.advanceTimersByTime(250);
    expect(line('screen').classList.contains('ebb')).toBe(false);
  });

  it('starts from empty at once for a card that is started again while it plays', () => {
    mount();
    pointer(card('full'), 'pointerenter');
    vi.advanceTimersByTime(1000);
    // What the line is like each time its style is computed during the press.
    const computed: string[] = [];
    const el = line('full');
    vi.spyOn(el, 'getBoundingClientRect').mockImplementation(() => {
      computed.push([card('full').classList.contains('playing') && 'playing', el.classList.contains('ebb') && 'ebb', el.style.transform && 'held'].filter(Boolean).join(' '));
      return {} as DOMRect;
    });
    card('full').click();
    // Style was computed while the line was off, not fading and not held: its animation starts over when `playing` returns.
    expect(computed).toContain('');
    expect(card('full').classList.contains('playing')).toBe(true);
    expect(el.classList.contains('ebb')).toBe(false);
    expect(el.style.transform).toBe('');
  });

  it('starts from empty at once for a card that starts while its line still fades, and that fade ends nothing', () => {
    mount();
    widthOf('full', 'matrix(0.4, 0, 0, 1, 0, 0)');
    pointer(card('full'), 'pointerenter');
    vi.advanceTimersByTime(1000);
    pointer(card('full'), 'pointerleave');
    vi.advanceTimersByTime(100);
    expect(line('full').classList.contains('ebb')).toBe(true);
    pointer(card('full'), 'pointerenter');
    expect(line('full').classList.contains('ebb')).toBe(false);
    expect(line('full').style.transform).toBe('');
    expect(card('full').classList.contains('playing')).toBe(true);
    // The moment the earlier fade would have ended: the card plays on, its line untouched.
    vi.advanceTimersByTime(150);
    expect(card('full').classList.contains('playing')).toBe(true);
    expect(line('full').classList.contains('ebb')).toBe(false);
    expect(line('full').style.transform).toBe('');
  });
});
