import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { recordAnimations } from './cards-test-support';
import { AFTER_POINTER, FADE, GAP, LINGER, PICTURES, mountCards, type Watch } from './cards';

const page = new DOMParser().parseFromString(readFileSync(join(process.cwd(), 'index.html'), 'utf8'), 'text/html').body.innerHTML;
const NAMES = ['rules', 'secrets', 'record', 'screen', 'recipes', 'extensions', 'disk', 'call', 'registry'];

const card = (name: string) => document.querySelector<HTMLElement>(`.fc-item[data-picture="${name}"]`)!;
const copies = (name: string) => [...card(name).querySelectorAll<SVGSVGElement>('svg.fc-ghost')];
const picture = (name: string) => card(name).querySelector<SVGSVGElement>('.fc-pic svg:not(.fc-ghost)')!;
const part = (name: string, named: string) => picture(name).querySelector<SVGElement>(`[data-part="${named}"]`)!;
const lineOf = (name: string) => card(name).querySelector<HTMLElement>('.fc-fill')!;
const lasts = (name: string) => PICTURES[name][0] + LINGER;
const mouse = (target: Element, type: string, kind = 'mouse') => {
  const event = new MouseEvent(type);
  Object.defineProperty(event, 'pointerType', { value: kind });
  target.dispatchEvent(event);
};
const key = (target: Element, name: string) => {
  const event = new KeyboardEvent('keydown', { key: name, cancelable: true });
  target.dispatchEvent(event);
  return event;
};

let seen: Parameters<Watch>[1];
let animations: ReturnType<typeof recordAnimations>;
let visibility = 'visible';
/** Whether the focus a card gets is one that shows, as a keyboard's does. */
let keyboard = true;
const watch: Watch = (_items, report) => (seen = report);

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout', 'performance', 'Date'] });
  document.body.innerHTML = page;
  animations = recordAnimations();
  visibility = 'visible';
  keyboard = true;
  Object.defineProperty(document, 'visibilityState', { configurable: true, get: () => visibility });
  for (const item of document.querySelectorAll<HTMLElement>('.fc-item')) {
    const matches = (selector: string) => (selector === ':focus-visible' ? keyboard : Element.prototype.matches.call(item, selector));
    item.matches = matches as typeof item.matches;
  }
});
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

const mount = (still = false, onChange?: () => void) => mountCards(document.getElementById('cards')!, { still, watch, onChange });
/** jsdom computes no animation: `target` says it has `now`. */
const looks = (target: Element, now: Partial<CSSStyleDeclaration>) => {
  const real = window.getComputedStyle.bind(window);
  vi.spyOn(window, 'getComputedStyle').mockImplementation(((el: Element) => (el === target ? now : real(el))) as typeof getComputedStyle);
};

describe('a picture', () => {
  it.each(NAMES)('moves parts it has, each once, and every part it names moves (%s)', (name) => {
    const tracks = PICTURES[name][1].map(([named]) => named);
    const drawn = [...picture(name).querySelectorAll('[data-part]')].map((el) => el.getAttribute('data-part'));
    expect(drawn.sort()).toEqual([...tracks].sort());
    expect(new Set(tracks).size).toBe(tracks.length);
  });

  it.each(NAMES)('plays its moments in order, inside the time it has (%s)', (name) => {
    const [end, tracks] = PICTURES[name];
    for (const [named, keys] of tracks) {
      expect(keys[0][0], named).toBe(0);
      keys.forEach(([at], i) => i && expect(at, named).toBeGreaterThanOrEqual(keys[i - 1][0]));
      expect(keys[keys.length - 1][0], named).toBeLessThanOrEqual(end);
    }
  });

  // Stopping a picture is cancelling it, so what shows without script or motion has to be where it ends.
  it.each(NAMES)('ends as it is drawn, so cancelling it never moves it (%s)', (name) => {
    for (const [named, keys] of PICTURES[name][1]) {
      const { opacity = 1, transform = '' } = keys[keys.length - 1][1];
      const { classList } = part(name, named);
      expect(opacity, named).toBe(classList.contains('fc-moving') ? 0 : 1);
      if (!opacity) continue;
      const amounts = transform.replace(/^[a-zA-Z]+/, '').match(/-?\d+(\.\d+)?/g)!.map(Number);
      const rest = transform.startsWith('scale') && !classList.contains('fc-cover') ? 1 : 0;
      expect(amounts, `${named}: ${transform}`).toEqual(amounts.map(() => rest));
    }
  });

  it('is the same nine in the page and in the script', () => {
    expect([...document.querySelectorAll<HTMLElement>('.fc-item')].map((item) => item.dataset.picture)).toEqual(NAMES);
    expect(Object.keys(PICTURES)).toEqual(NAMES);
  });

  it('is left at its end on mount: nothing runs and no copy is made', () => {
    mount();
    expect(animations.live()).toEqual([]);
    expect(document.querySelectorAll('svg.fc-ghost')).toHaveLength(0);
  });

  it('is animated over the whole of its turn, and the line over it with it', () => {
    mount();
    card('rules').click();
    const runs = animations.live().filter(({ target }) => !target.matches('.fc-ghost'));
    expect(runs).toHaveLength(PICTURES.rules[1].length + 1);
    for (const { frames, options } of runs) {
      expect(options).toEqual({ duration: lasts('rules'), fill: 'both' });
      if (frames[0].offset === undefined) continue;
      expect(frames[0].offset).toBe(0);
      expect(frames[frames.length - 1].offset).toBe(1);
    }
    expect(animations.moving()).toEqual(['rules']);
  });
});

describe('the dissolve of a picture', () => {
  it('makes one copy as a play starts, right after the picture, and none at a natural end', () => {
    mount();
    card('rules').click();
    const [copy] = copies('rules');
    expect(copies('rules')).toHaveLength(1);
    expect(picture('rules').nextElementSibling).toBe(copy);
    expect(copy.getAttribute('aria-hidden')).toBe('true');
    vi.advanceTimersByTime(FADE - 1);
    expect(copies('rules')).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(copies('rules')).toHaveLength(0);
    vi.advanceTimersByTime(lasts('rules') - FADE);
    expect(animations.playing()).toEqual([]);
    expect(animations.moving()).toEqual([]);
    expect(copies('rules')).toHaveLength(0);
  });

  it('writes on the copy what was moving, as it was when it stopped', () => {
    mount();
    mouse(card('rules'), 'pointerenter');
    vi.advanceTimersByTime(1200);
    looks(part('rules', 'request-a'), { opacity: '1', transform: 'matrix(1, 0, 0, 1, 90, 0)' });
    mouse(card('rules'), 'pointerleave');
    const twin = copies('rules')[0].querySelector<SVGElement>('[data-part="request-a"]')!;
    expect(twin.style.opacity).toBe('1');
    expect(twin.style.transform).toBe('matrix(1, 0, 0, 1, 90, 0)');
    expect(part('rules', 'request-a').style.transform).toBe('');
    expect(animations.moving()).toEqual([]);
  });

  it('gives each copy clip paths of its own, so the page never has an id twice', () => {
    mount();
    for (const name of NAMES) card(name).click();
    card('secrets').click();
    const ids = [...document.querySelectorAll('[id]')].map((el) => el.id);
    expect(new Set(ids).size).toBe(ids.length);
    const clipped = [...document.querySelectorAll('svg.fc-ghost [clip-path]')];
    expect(clipped.length).toBeGreaterThan(5);
    for (const el of clipped) {
      const id = /^url\(#(.+)\)$/.exec(el.getAttribute('clip-path')!)![1];
      expect(el.closest('svg.fc-ghost')!.querySelector(`[id="${id}"]`), id).not.toBeNull();
    }
  });

  it('dissolves the card another one replaces, and the one that starts', () => {
    mount();
    card('rules').click();
    vi.advanceTimersByTime(FADE);
    card('secrets').click();
    expect(copies('rules')).toHaveLength(1);
    expect(copies('secrets')).toHaveLength(1);
  });

  it('puts a newer copy under an older one, and keeps the older one fading', () => {
    mount();
    mouse(card('rules'), 'pointerenter');
    const [older] = copies('rules');
    vi.advanceTimersByTime(100);
    mouse(card('rules'), 'pointerleave');
    const [first, second] = copies('rules');
    expect(second).toBe(older);
    vi.advanceTimersByTime(FADE - 100);
    expect(copies('rules')).toEqual([first]);
    vi.advanceTimersByTime(100);
    expect(copies('rules')).toEqual([]);
  });

  it('dissolves a card played again while it plays, once', () => {
    mount();
    card('rules').click();
    vi.advanceTimersByTime(1000);
    key(card('rules'), 'Enter');
    expect(copies('rules')).toHaveLength(1);
  });

  it('dissolves a card on its turn that the film stops', () => {
    const cards = mount();
    seen([[0, true]]);
    cards.turns(true);
    vi.advanceTimersByTime(1000);
    cards.turns(false);
    expect(animations.playing()).toEqual([]);
    expect(copies('rules')).toHaveLength(1);
  });
});

describe('the line over a card', () => {
  it('fades out from the width it had, and starts from empty when the card plays again', () => {
    mount();
    mouse(card('rules'), 'pointerenter');
    vi.advanceTimersByTime(1000);
    looks(lineOf('rules'), { transform: 'matrix(0.4, 0, 0, 1, 0, 0)' });
    mouse(card('rules'), 'pointerleave');
    const line = () => animations.live().filter(({ target }) => target === lineOf('rules'));
    expect(line().map(({ frames, options }) => [frames, options])).toEqual([
      [[{ opacity: 1, transform: 'matrix(0.4, 0, 0, 1, 0, 0)' }, { opacity: 0, transform: 'matrix(0.4, 0, 0, 1, 0, 0)' }], { duration: FADE }],
    ]);
    vi.advanceTimersByTime(100);
    mouse(card('rules'), 'pointerenter');
    expect(line()).toHaveLength(1);
    expect(line()[0].frames[0].transform).toBe('scaleX(0)');
  });
});

describe('whose turn it is', () => {
  it('plays at once a card that gets the focus of a keyboard, and on Enter and Space', () => {
    mount();
    card('record').dispatchEvent(new FocusEvent('focus'));
    expect(animations.playing()).toEqual(['record']);
    vi.advanceTimersByTime(lasts('record'));
    expect(animations.playing()).toEqual([]);
    expect(key(card('record'), ' ').defaultPrevented).toBe(true);
    expect(animations.playing()).toEqual(['record']);
    card('record').dispatchEvent(new FocusEvent('blur'));
    expect(animations.playing()).toEqual([]);
    expect(key(card('screen'), 'Enter').defaultPrevented).toBe(true);
    expect(animations.playing()).toEqual(['screen']);
    expect(key(card('screen'), 'Tab').defaultPrevented).toBe(false);
  });

  it('gives the turn to the first in view after the last, of those that came into view at one moment', () => {
    const cards = mount();
    cards.turns(true);
    seen([[1, true]]);
    expect(animations.playing()).toEqual(['secrets']);
    seen([[0, true], [1, false], [2, true]]);
    expect(animations.playing()).toEqual(['record']);
  });

  it('plays nothing with reduced motion, whatever is done', () => {
    const cards = mount(true);
    seen([[0, true]]);
    cards.turns(true);
    mouse(card('rules'), 'pointerenter');
    card('rules').click();
    card('rules').dispatchEvent(new FocusEvent('focus'));
    key(card('rules'), 'Enter');
    vi.advanceTimersByTime(60000);
    expect(animations.live()).toEqual([]);
    expect(cards.busy).toBe(false);
  });

  it.each([3, 11, 29, 71])('one card plays at a time, the one the rules name, after every step (seed %i)', { timeout: 30_000 }, (start) => {
    let told = false;
    const cards = mount(false, () => (told = cards.busy));
    // The model: the rules, with no drawing.
    const model = {
      now: 0, current: -1, hand: false, until: 0, pointed: -1, last: -1, resume: -1,
      may: false, still: false, hidden: false, inView: NAMES.map(() => false),
    };
    const halt = () => {
      model.current = -1;
      model.hand = false;
    };
    const play = (i: number, hand: boolean) => {
      if (model.still || model.hidden) return;
      Object.assign(model, { current: i, last: i, hand, until: model.now + lasts(NAMES[i]), resume: -1 });
    };
    const next = () => {
      if (!model.may || model.still || model.resume >= 0 || model.pointed >= 0 || model.current >= 0) return;
      const after = NAMES.map((_, k) => (model.last + 1 + k) % NAMES.length).find((i) => model.inView[i]);
      if (after !== undefined) play(after, false);
    };
    const pass = (ms: number) => {
      const target = model.now + ms;
      for (;;) {
        const moments = [...(model.current >= 0 ? [model.until] : []), ...(model.resume >= 0 ? [model.resume] : [])];
        const at = Math.min(...moments);
        if (at > target) break;
        model.now = at;
        if (model.current >= 0 && model.until === at) {
          halt();
          model.resume = at + GAP;
        } else {
          model.resume = -1;
          next();
        }
      }
      model.now = target;
    };

    let seed = start;
    const pick = (below: number) => Math.floor(((seed = (seed * 48271) % 2147483647) / 2147483647) * below);
    let played = 0;
    for (let step = 0; step < 700; step++) {
      const i = pick(NAMES.length);
      const item = card(NAMES[i]);
      const op = pick(24);
      if (op === 0) {
        mouse(item, 'pointerenter');
        if (!model.still) {
          model.pointed = i;
          play(i, true);
        }
      } else if (op === 1 && model.pointed >= 0) {
        const left = model.pointed;
        mouse(card(NAMES[left]), 'pointerleave');
        model.pointed = -1;
        if (model.current === left) halt();
        model.resume = model.now + AFTER_POINTER;
      } else if (op === 2) {
        mouse(item, 'pointerenter', 'touch');
        mouse(item, 'pointerleave', 'touch');
      } else if (op === 3) {
        item.click();
        if (model.current !== i || !model.hand) play(i, true);
      } else if (op === 4) {
        keyboard = pick(4) > 0;
        item.dispatchEvent(new FocusEvent('focus'));
        if (keyboard) play(i, true);
      } else if (op === 5) {
        const left = model.current >= 0 ? model.current : i;
        card(NAMES[left]).dispatchEvent(new FocusEvent('blur'));
        if (model.current === left && model.hand && model.pointed !== left) {
          halt();
          model.resume = model.now + AFTER_POINTER;
        }
      } else if (op === 6) {
        key(item, pick(2) ? 'Enter' : ' ');
        play(i, true);
      } else if (op === 7 || op === 8 || op === 9) {
        const changes = Array.from({ length: 1 + pick(3) }, () => [pick(NAMES.length), pick(4) > 0] as const);
        seen(changes);
        for (const [k, on] of changes) model.inView[k] = on;
        if (model.current >= 0 && !model.hand && !model.inView[model.current]) halt();
        next();
      } else if (op === 10 || op === 11) {
        model.may = pick(4) > 0;
        cards.turns(model.may);
        if (model.may) next();
        else if (model.current >= 0 && !model.hand) halt();
      } else if (op === 12 && pick(3) === 0) {
        model.still = !model.still;
        cards.still(model.still);
        if (model.still) {
          model.pointed = -1;
          halt();
        } else next();
      } else if (op === 13) {
        model.hidden = !model.hidden;
        visibility = model.hidden ? 'hidden' : 'visible';
        if (model.hidden) {
          cards.rest();
          halt();
        }
      } else {
        const ms = pick(3) ? pick(1500) : pick(7000);
        vi.advanceTimersByTime(ms);
        pass(ms);
      }
      const expected = model.current >= 0 ? [NAMES[model.current]] : [];
      const where = `step ${step}, op ${op}`;
      expect(animations.playing(), where).toEqual(expected);
      expect(animations.moving(), where).toEqual(expected);
      expect(cards.busy, where).toBe(model.pointed >= 0 || (model.current >= 0 && model.hand));
      expect(told, where).toBe(cards.busy);
      played += expected.length;
    }
    expect(played).toBeGreaterThan(50);
    vi.advanceTimersByTime(FADE);
    expect(document.querySelectorAll('svg.fc-ghost').length).toBeLessThanOrEqual(2);
  });
});
