import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { add, rem, scene, type Cue } from './scene';

const make = (cues: Cue[], layout?: () => void) => {
  const el = document.createElement('div');
  el.innerHTML = '<i class="a"></i><i class="b on"></i>';
  return { el, picture: scene(el, cues, 1000, layout) };
};
const state = (el: Element) => [...el.children].map((part) => part.className).join(' | ');

describe('a scene', () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it('held at a moment, has every cue before that moment applied at once, and no later one', () => {
    const seen: boolean[] = [];
    const { el, picture } = make([
      [100, add('.a')],
      [200, rem('.b')],
      [300, (_q, instant) => seen.push(instant)],
      [400, add('.a', 'late')],
    ]);
    picture.hold(350);
    expect(state(el)).toBe('a on | b');
    expect(seen).toEqual([true]);
    vi.advanceTimersByTime(5000);
    expect(state(el)).toBe('a on | b');
  });

  it('held with no moment given, shows its end', () => {
    const { el, picture } = make([[999, add('.a')]]);
    picture.hold();
    expect(state(el)).toBe('a on | b on');
  });

  it('played from a moment, applies what came before at once and the rest when its time comes', () => {
    const seen: boolean[] = [];
    const { el, picture } = make([
      [100, add('.a')],
      [300, (_q, instant) => seen.push(instant)],
      [400, rem('.b')],
    ]);
    picture.play(200);
    expect(state(el)).toBe('a on | b on');
    vi.advanceTimersByTime(99);
    expect(seen).toEqual([]);
    vi.advanceTimersByTime(1);
    expect(seen).toEqual([false]);
    vi.advanceTimersByTime(100);
    expect(state(el)).toBe('a on | b');
  });

  it('starts from its first markup each time, so nothing has to be undone', () => {
    const { el, picture } = make([[100, add('.a')]]);
    picture.hold(500);
    el.querySelector('.a')?.classList.add('stray');
    picture.hold(50);
    expect(state(el)).toBe('a | b on');
  });

  it('leaves no cue of an earlier play running once it is held or played again', () => {
    const { el, picture } = make([
      [100, add('.a')],
      [500, add('.a', 'late')],
    ]);
    picture.play();
    vi.advanceTimersByTime(200);
    picture.hold(0);
    vi.advanceTimersByTime(5000);
    expect(state(el)).toBe('a | b on');
    picture.play(450);
    picture.play(0);
    vi.advanceTimersByTime(99);
    expect(state(el)).toBe('a | b on');
  });

  it('applies a cue at moment 0 at once, held or played: it is where a part starts', () => {
    const seen: boolean[] = [];
    const { picture } = make([[0, (_q, instant) => seen.push(instant)]]);
    picture.play();
    expect(seen).toEqual([true]);
    vi.advanceTimersByTime(1000);
    expect(seen).toEqual([true]);
    picture.hold(0);
    expect(seen).toEqual([true, true]);
  });

  it('measures the fresh markup before any cue', () => {
    const order: string[] = [];
    const { el, picture } = make([[100, () => order.push('cue')]], () => order.push(`layout:${state(el)}`));
    picture.hold(200);
    expect(order).toEqual(['layout:a | b on', 'cue']);
  });

  it('animates nothing while it is put at a moment', () => {
    let during = '';
    const { el, picture } = make([[100, () => (during = el.className)]]);
    picture.hold(200);
    expect(during).toBe('jump');
    expect(el.className).toBe('');
  });
});
