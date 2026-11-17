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
    vi.advanceTimersByTime(3300 + 600);
    expect(document.querySelectorAll('svg.ghost')).toHaveLength(0);
    pointer(card('tools'), 'pointerleave');
    expect(document.querySelectorAll('svg.ghost')).toHaveLength(0);
  });

  it('lays a copy of the look before over the picture when a play starts, and removes it when its fade is over', () => {
    mount();
    const before = live('rec').innerHTML;
    pointer(card('rec'), 'pointerenter');
    const [copy] = copies('rec');
    expect(copies('rec')).toHaveLength(1);
    expect(copy.innerHTML).toBe(before);
    expect(copy.classList.contains('mini')).toBe(true);
    // The picture underneath is already at its first moment.
    expect(live('rec').innerHTML).not.toBe(before);
    vi.advanceTimersByTime(249);
    expect(copies('rec')).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(copies('rec')).toHaveLength(0);
  });

  it('makes the copy over the picture, not content: inert, hidden, with no id and nothing that runs', () => {
    mount();
    live('rec').querySelector('.r1')!.id = 'part';
    pointer(card('rec'), 'pointerenter');
    const [copy] = copies('rec');
    expect(copy.getAttribute('aria-hidden')).toBe('true');
    expect(copy.hasAttribute('inert')).toBe(true);
    expect(copy.querySelector('[id]')).toBeNull();
    expect(copy.classList.contains('jump')).toBe(true);
    expect(copy.nextSibling).not.toBe(live('rec'));
    expect(live('rec').nextElementSibling).toBe(copy);
  });

  it('carries the classes the picture had at the instant the pointer leaves, before the end', () => {
    mount();
    pointer(card('rec'), 'pointerenter');
    vi.advanceTimersByTime(1000);
    const classes = [...live('rec').querySelectorAll('.on')].map((part) => part.getAttribute('class'));
    expect(classes.length).toBeGreaterThan(0);
    pointer(card('rec'), 'pointerleave');
    const [copy] = copies('rec');
    expect([...copy.querySelectorAll('.on')].map((part) => part.getAttribute('class'))).toEqual(classes);
    // The live picture is at its end now.
    expect(live('rec').querySelectorAll('.p.on')).toHaveLength(3);
    vi.advanceTimersByTime(250);
    expect(copies('rec')).toHaveLength(0);
  });

  it('writes the computed value of what is animating onto the copy', () => {
    mount();
    pointer(card('keys'), 'pointerenter');
    vi.advanceTimersByTime(1500);
    const dot = live('keys').querySelector('.p1')!;
    animated = [{ target: dot, props: ['transform'] }];
    const real = window.getComputedStyle.bind(window);
    vi.spyOn(window, 'getComputedStyle').mockImplementation(((el: Element) =>
      el === dot ? ({ transform: 'matrix(1, 0, 0, 1, 172, 70)' } as CSSStyleDeclaration) : real(el)) as typeof getComputedStyle);
    pointer(card('keys'), 'pointerleave');
    const [copy] = copies('keys');
    expect(copy.querySelector<SVGElement>('.p1')!.style.transform).toBe('matrix(1, 0, 0, 1, 172, 70)');
  });

  it('dissolves a card that another one replaces, and the one that starts', () => {
    mount();
    card('rec').click();
    vi.advanceTimersByTime(250);
    card('tools').click();
    expect(copies('rec')).toHaveLength(1);
    expect(copies('tools')).toHaveLength(1);
  });

  it('puts a newer copy under an older one, and keeps the older one fading', () => {
    mount();
    pointer(card('rec'), 'pointerenter');
    const [older] = copies('rec');
    vi.advanceTimersByTime(100);
    pointer(card('rec'), 'pointerleave');
    const all = copies('rec');
    expect(all).toHaveLength(2);
    expect(all[1]).toBe(older);
    expect(live('rec').nextElementSibling).toBe(all[0]);
    // The older one goes first, then the newer one.
    vi.advanceTimersByTime(150);
    expect(copies('rec')).toEqual([all[0]]);
    vi.advanceTimersByTime(100);
    expect(copies('rec')).toHaveLength(0);
  });

  it('leaves no copy after a quick in and out, whatever the card did meanwhile', () => {
    mount();
    for (let k = 0; k < 6; k++) {
      pointer(card('rec'), 'pointerenter');
      vi.advanceTimersByTime(20);
      pointer(card('rec'), 'pointerleave');
      pointer(card('tools'), 'pointerenter');
      card('rec').click();
      vi.advanceTimersByTime(20);
      pointer(card('tools'), 'pointerleave');
    }
    vi.advanceTimersByTime(250);
    expect(document.querySelectorAll('svg.ghost')).toHaveLength(0);
  });

  it('dissolves a play started again on a card that was playing, once', () => {
    mount();
    pointer(card('rec'), 'pointerenter');
    vi.advanceTimersByTime(1000);
    card('rec').click();
    // One copy: the look at the restart, not also the card's end.
    expect(copies('rec')).toHaveLength(1);
  });

  it('dissolves for a stop by the flow too, and with reduced motion', () => {
    const cards = mount(true);
    card('rec').click();
    vi.advanceTimersByTime(250);
    expect(copies('rec')).toHaveLength(0);
    vi.advanceTimersByTime(500);
    cards.rest();
    expect(copies('rec')).toHaveLength(1);
  });

  it('holds the dissolve in its own constant, written for the css', () => {
    mount();
    expect(document.querySelector<HTMLElement>('#cards')!.style.getPropertyValue('--fade')).toBe('250ms');
  });
});

describe('the line over a card', () => {
  it('fades out from the width it had when the card stops or ends, and is gone when the fade is over', () => {
    mount();
    pointer(card('rec'), 'pointerenter');
    expect(card('rec').classList.contains('playing')).toBe(true);
    vi.advanceTimersByTime(1000);
    pointer(card('rec'), 'pointerleave');
    expect(card('rec').classList.contains('playing')).toBe(false);
    expect(line('rec').classList.contains('ebb')).toBe(true);
    vi.advanceTimersByTime(250);
    expect(line('rec').classList.contains('ebb')).toBe(false);
    expect(line('rec').style.transform).toBe('');

    card('tools').click();
    vi.advanceTimersByTime(3000 + 600);
    expect(card('tools').classList.contains('playing')).toBe(false);
    expect(line('tools').classList.contains('ebb')).toBe(true);
    vi.advanceTimersByTime(250);
    expect(line('tools').classList.contains('ebb')).toBe(false);
  });

  it('starts from empty at once, also for a card that is started again while it plays', () => {
    mount();
    pointer(card('rec'), 'pointerenter');
    vi.advanceTimersByTime(1000);
    const flush = vi.spyOn(line('rec'), 'getBoundingClientRect');
    card('rec').click();
    // Style was computed between the line being taken off and put on again, so its animation starts over.
    expect(flush).toHaveBeenCalled();
    expect(card('rec').classList.contains('playing')).toBe(true);
    expect(line('rec').classList.contains('ebb')).toBe(false);
    expect(line('rec').style.transform).toBe('');
  });
});
