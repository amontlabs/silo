// What a part depends on: computed from the plan, then marked, shown in the key and spoken on the map.
import { act, fireEvent, render, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { buildDataFrom, partDepends } from './data';
import { buildData, FEATURES, planFiles } from './fixtures';
import { EXAMPLE_MAP } from './map-example';
import { MapView } from './MapView';
import { buildPlan } from './plan';
import type { BuildData } from './plan';

/**
 * Starts, in the order of order.json: Part D (`intake`), Part A (`core`), Part B (`create`), Part C (`shells`), Part E (`triggers`).
 * Part A and Part D need each other: `calls` (A) needs `intake` (D), and `fork` (D) needs `core` (A). Only A depends on D, which starts first.
 * Part B needs A (`create` needs `core`); `fork` also names B, but D starts before it, so D depends on none.
 * Part E needs D and A, which start before it. `exec` needs `create`, in its own part, and `audit` names a decision of no part: both say nothing.
 * Part C is alone.
 */
const NEEDS: Record<string, string[]> = { calls: ['intake'], create: ['core'], exec: ['create'], fork: ['core', 'create'], audit: ['fork', 'core-in-zig', 'security-properly'] };
const withNeeds = () => buildDataFrom(planFiles({ features: FEATURES.map((feature) => (NEEDS[feature.id] ? { ...feature, needs: NEEDS[feature.id] } : feature)) }), { map: EXAMPLE_MAP });

const draw = (data: BuildData, folded = false) => render(<MapView layout={data.map} groups={buildPlan(data, null).parts} folded={folded} current={null} onOpen={() => {}} />).container;
const part = (map: HTMLElement, id: string) => map.querySelector<HTMLElement>(`.ws-part[data-part="${id}"]`)!;
const marked = (map: HTMLElement) => [...map.querySelectorAll('.ws-part.marked')].map((node) => node.getAttribute('data-part'));
const faint = (map: HTMLElement) => [...map.querySelectorAll('.ws-part.faint')].map((node) => node.getAttribute('data-part'));
const tip = () => document.querySelector('.ws-tip')?.textContent;
const shown = () => waitFor(() => expect(document.querySelector('.ws-tip')).not.toBeNull());
const keyLines = (map: HTMLElement) => [...map.querySelectorAll('.ws-mapkey span')].map((line) => line.textContent);

afterEach(() => vi.restoreAllMocks());

describe('which parts a part depends on', () => {
  it('is read from the entries of a part that name an entry of a part that starts before it', () => {
    expect(new Map(withNeeds().parts.map((p) => [p.id, p.dependsOn]))).toEqual(new Map([
      ['part-a', ['part-d']],
      ['part-b', ['part-a']],
      ['part-c', []],
      ['part-d', []],
      ['part-e', ['part-a', 'part-d']],
    ]));
  });

  it('never has two parts depend on each other', () => {
    const depends = new Map(withNeeds().parts.map((p) => [p.id, p.dependsOn]));
    for (const [id, on] of depends) for (const other of on) expect(depends.get(other)).not.toContain(id);
  });

  it('is empty on a plan where no entry needs another part', () => {
    expect(buildData().parts.every((p) => p.dependsOn.length === 0)).toBe(true);
  });

  it('starts a part at its first feature, or at its first entry when it has no feature', () => {
    const entry = (id: string, kind: 'decision' | 'feature', part: string | null, position: number, needs: string[] = []) => ({ id, kind, part, position, needs });
    const parts = [{ id: 'one' }, { id: 'two' }];
    // `two` starts at its feature, at 5, though its decision is at 1: it depends on `one`, whose feature is at 3.
    const entries = [entry('d2', 'decision', 'two', 1), entry('f1', 'feature', 'one', 3), entry('f2', 'feature', 'two', 5, ['f1']), entry('f1b', 'feature', 'one', 6, ['d2'])];
    expect(partDepends(parts, entries, []).get('two')).toEqual(['one']);
    expect(partDepends(parts, entries, []).get('one')).toEqual([]);
    // With no feature, `two` starts at its decision, at 1: before `one`, so `one` depends on it.
    const none = [entry('d2', 'decision', 'two', 1), entry('f1', 'feature', 'one', 3, ['d2'])];
    expect(partDepends(parts, none, []).get('one')).toEqual(['two']);
  });

  it('follows the order of the map, and puts the parts the map does not place last, in the order of parts.json', () => {
    const entries = [
      { id: 'w', kind: 'feature' as const, part: 'four', position: 1, needs: [] }, { id: 'y', kind: 'feature' as const, part: 'two', position: 2, needs: [] },
      { id: 'z', kind: 'feature' as const, part: 'three', position: 3, needs: [] }, { id: 'x', kind: 'feature' as const, part: 'one', position: 4, needs: ['z', 'w', 'y'] },
    ];
    const parts = [{ id: 'one' }, { id: 'two' }, { id: 'three' }, { id: 'four' }];
    // `four` is not placed on the map, so it comes last.
    expect(partDepends(parts, entries, ['three', 'two', 'one']).get('one')).toEqual(['three', 'two', 'four']);
    expect(partDepends(parts, entries, ['two', 'three']).get('one')).toEqual(['two', 'three', 'four']);
    // None is placed: parts.json decides, and the order of `needs` in the entry decides nothing.
    expect(partDepends(parts, entries, []).get('one')).toEqual(['two', 'three', 'four']);
  });
});

describe('the map with the pointer on a part', () => {
  it('marks the parts it depends on and fades every other part, those that depend on it too', async () => {
    const map = draw(withNeeds());
    await userEvent.hover(part(map, 'part-e'));
    expect(map.querySelector('.ws-stage')).toHaveClass('relating');
    expect(marked(map)).toEqual(['part-a', 'part-d']);
    expect(faint(map)).toEqual(['part-b', 'part-c']);
    await userEvent.unhover(part(map, 'part-e'));
    await userEvent.hover(part(map, 'part-b'));
    expect(marked(map)).toEqual(['part-a']);
    // Part A and Part D need each other: Part D, hovered, marks nothing of A.
    expect(faint(map)).toEqual(['part-c', 'part-d', 'part-e']);
    await userEvent.unhover(part(map, 'part-b'));
    await userEvent.hover(part(map, 'part-d'));
    expect(marked(map)).toEqual([]);
    expect(faint(map)).toEqual(['part-a', 'part-b', 'part-c', 'part-e']);
    await userEvent.unhover(part(map, 'part-d'));
    await userEvent.hover(part(map, 'part-a'));
    expect(marked(map)).toEqual(['part-d']);
  });

  it('says only the part\'s line in the tip', async () => {
    const map = draw(withNeeds());
    await userEvent.hover(part(map, 'part-e'));
    await shown();
    expect(tip()).toBe('A wide bar under the drawing.');
    expect(document.querySelectorAll('.ws-tip p')).toHaveLength(0);
  });

  it('clears the marks and the fading when the pointer leaves', async () => {
    const map = draw(withNeeds());
    await userEvent.hover(part(map, 'part-e'));
    await userEvent.unhover(part(map, 'part-e'));
    expect(map.querySelector('.ws-stage')).not.toHaveClass('relating');
    expect(marked(map)).toEqual([]);
    expect(faint(map)).toEqual([]);
    expect(document.querySelector('.ws-tip')).toBeNull();
  });

  it('does the same for the keyboard\'s focus, and clears it when the focus leaves', async () => {
    vi.spyOn(HTMLElement.prototype, 'matches').mockImplementation(function (this: HTMLElement, selector: string) { return selector === ':focus-visible'; });
    const map = draw(withNeeds());
    act(() => part(map, 'part-e').focus());
    expect(marked(map)).toEqual(['part-a', 'part-d']);
    fireEvent.blur(part(map, 'part-e'));
    expect(marked(map)).toEqual([]);
    expect(faint(map)).toEqual([]);
  });

  it('keeps the strip as it is: nothing marked or faded, and the tip of a name says the part and its line', async () => {
    const map = draw(withNeeds(), true);
    await userEvent.hover(part(map, 'part-e'));
    expect(map.querySelector('.ws-stage')).not.toHaveClass('relating');
    expect(marked(map)).toEqual([]);
    expect(faint(map)).toEqual([]);
    await shown();
    expect(tip()).toBe('Part E. A wide bar under the drawing.');
  });
});

describe('the key of the map', () => {
  it('gains the line "it depends on", with a sample of the ring, only while a part is marked', async () => {
    const map = draw(withNeeds());
    expect(keyLines(map)).toEqual(['decided', 'not decided']);
    expect(map.querySelector('.ws-ringkey')).toBeNull();
    await userEvent.hover(part(map, 'part-e'));
    expect(keyLines(map)).toEqual(['decided', 'not decided', 'it depends on']);
    expect(map.querySelectorAll('.ws-ringkey')).toHaveLength(1);
    await userEvent.unhover(part(map, 'part-e'));
    expect(keyLines(map)).toEqual(['decided', 'not decided']);
  });

  it('does not gain it when the part depends on none, since nothing is marked', async () => {
    const map = draw(withNeeds());
    await userEvent.hover(part(map, 'part-d'));
    expect(keyLines(map)).toEqual(['decided', 'not decided']);
  });
});

describe('the place of the tip', () => {
  /** Hovers Part A, whose box is at `top`..`bottom` in a window 1200 × 800, and gives where the tip is put. */
  async function placed(top: number, bottom: number, left = 500) {
    const map = draw(withNeeds());
    const box = { left, right: left + 80, top, bottom, width: 80, height: bottom - top, x: left, y: top, toJSON: () => ({}) };
    vi.spyOn(part(map, 'part-a'), 'getBoundingClientRect').mockReturnValue(box);
    vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockReturnValue(270);
    vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockReturnValue(120);
    window.innerWidth = 1200;
    window.innerHeight = 800;
    await userEvent.hover(part(map, 'part-a'));
    await shown();
    return (document.querySelector('.ws-tip') as HTMLElement).style.transform;
  }

  it('goes below its part when it fits there, even for a tall part', async () => {
    // A box 272 high from y = 60: 332 + 8 + 120 fits, so the tip does not cover the box.
    expect(await placed(60, 332)).toBe('translate(405px, 340px)');
  });

  it('goes above its part when it fits only there', async () => {
    // Below: 700 + 8 + 120 is past the window; above: 640 - 128 fits.
    expect(await placed(640, 700)).toBe('translate(405px, 512px)');
  });

  it('is pulled inside the window when it fits in neither place, and 8 px in from the side', async () => {
    // Above: 60 - 128 is out of the window; below: 780 + 8 + 120 is past it. It can only cover its part, at the window's top.
    expect(await placed(60, 780, 1100)).toBe('translate(922px, 8px)');
  });
});

describe('the pointer and the keyboard\'s focus on different parts', () => {
  const focusable = () => vi.spyOn(HTMLElement.prototype, 'matches').mockImplementation(function (this: HTMLElement, selector: string) { return selector === ':focus-visible'; });

  it('keeps a part marked while the keyboard\'s focus is on it after the pointer has been on it and left', async () => {
    focusable();
    const map = draw(withNeeds());
    act(() => part(map, 'part-e').focus());
    await userEvent.hover(part(map, 'part-e'));
    await userEvent.unhover(part(map, 'part-e'));
    expect(marked(map)).toEqual(['part-a', 'part-d']);
    await waitFor(() => expect(tip()).toBe('A wide bar under the drawing.'));
    fireEvent.blur(part(map, 'part-e'));
    expect(marked(map)).toEqual([]);
    expect(faint(map)).toEqual([]);
    expect(document.querySelector('.ws-tip')).toBeNull();
  });

  it('marks the pointer\'s part when the focus is on another, and the focus\'s part again once the pointer leaves', async () => {
    focusable();
    const map = draw(withNeeds());
    act(() => part(map, 'part-e').focus());
    await userEvent.hover(part(map, 'part-b'));
    expect(marked(map)).toEqual(['part-a']);
    await waitFor(() => expect(tip()).toBe('A part that holds another.'));
    await userEvent.unhover(part(map, 'part-b'));
    expect(marked(map)).toEqual(['part-a', 'part-d']);
    await waitFor(() => expect(tip()).toBe('A wide bar under the drawing.'));
  });

  it('keeps the pointer\'s part marked when the focus arrives on another, and clears everything once both have left', async () => {
    focusable();
    const map = draw(withNeeds());
    await userEvent.hover(part(map, 'part-b'));
    act(() => part(map, 'part-e').focus());
    expect(marked(map)).toEqual(['part-a']);
    await userEvent.unhover(part(map, 'part-b'));
    expect(marked(map)).toEqual(['part-a', 'part-d']);
    fireEvent.blur(part(map, 'part-e'));
    expect(marked(map)).toEqual([]);
    expect(map.querySelector('.ws-stage')).not.toHaveClass('relating');
  });
});

describe('what a screen reader hears for a part on the map', () => {
  it('gains "Depends on …" in the order of the map, and nothing for a part that depends on none', () => {
    const map = draw(withNeeds());
    expect(part(map, 'part-e')).toHaveAttribute('aria-label', expect.stringMatching(/ A wide bar under the drawing\. Depends on Part A, Part D\.$/));
    expect(part(map, 'part-b')).toHaveAttribute('aria-label', expect.stringMatching(/ A part that holds another\. Depends on Part A\.$/));
    expect(part(map, 'part-a')).toHaveAttribute('aria-label', expect.stringMatching(/ A tall part with a glyph\. Depends on Part D\.$/));
    for (const id of ['part-c', 'part-d']) expect(part(map, id).getAttribute('aria-label')).not.toMatch(/Depends on|Needed by/);
    expect(part(map, 'part-d').getAttribute('aria-label')).toMatch(/A part with copies behind it\.$/);
  });
});
