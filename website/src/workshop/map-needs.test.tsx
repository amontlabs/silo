// What a part needs and what needs it: computed from the plan, then marked, said in the tip and spoken on the map.
import { act, fireEvent, render, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { buildDataFrom, partNeeds } from './data';
import { buildData, FEATURES, planFiles } from './fixtures';
import type { Part } from './plan';
import { EXAMPLE_MAP } from './map-example';
import { MapView } from './MapView';
import { buildPlan } from './plan';
import type { BuildData } from './plan';

/**
 * Part B needs Part A (`create` needs `core`, and `exec` needs `create`, a need inside its own part that says nothing).
 * Part D needs A and B. Part E needs D, and a decision of A. Part C is neither needed nor needs; a decision of
 * no part is named too, and says nothing.
 */
const NEEDS: Record<string, string[]> = { create: ['core'], exec: ['create'], fork: ['core', 'create'], audit: ['fork', 'core-in-zig', 'security-properly'] };
const withNeeds = () => buildDataFrom(planFiles({ features: FEATURES.map((feature) => (NEEDS[feature.id] ? { ...feature, needs: NEEDS[feature.id] } : feature)) }), { map: EXAMPLE_MAP });

const draw = (data: BuildData, folded = false) => render(<MapView layout={data.map} groups={buildPlan(data, null).parts} folded={folded} current={null} onOpen={() => {}} />).container;
const part = (map: HTMLElement, id: string) => map.querySelector<HTMLElement>(`.ws-part[data-part="${id}"]`)!;
const marked = (map: HTMLElement) => [...map.querySelectorAll('.ws-part.needed')].map((node) => node.getAttribute('data-part'));
const faint = (map: HTMLElement) => [...map.querySelectorAll('.ws-part.faint')].map((node) => node.getAttribute('data-part'));
const tip = () => [...document.querySelectorAll('.ws-tip p')].map((line) => line.textContent);
const shown = () => waitFor(() => expect(document.querySelector('.ws-tip')).not.toBeNull());

afterEach(() => vi.restoreAllMocks());

describe('which parts a part needs', () => {
  it('is read from the entries of the part that name an entry of another part, each list in the order of the map', () => {
    const parts = new Map(withNeeds().parts.map((p) => [p.id, [p.needs, p.neededBy]]));
    expect(parts).toEqual(new Map([
      ['part-a', [[], ['part-b', 'part-d', 'part-e']]],
      ['part-b', [['part-a'], ['part-d']]],
      ['part-c', [[], []]],
      ['part-d', [['part-a', 'part-b'], ['part-e']]],
      ['part-e', [['part-a', 'part-d'], []]],
    ]));
  });

  it('is empty on a plan where no entry needs another part', () => {
    expect(buildData().parts.every((p: Part) => p.needs.length === 0 && p.neededBy.length === 0)).toBe(true);
  });

  it('follows the order it is given, and puts a part the map does not place last, in the order of parts.json', () => {
    const entries = [{ id: 'x', part: 'one', needs: ['y', 'z'] }, { id: 'y', part: 'two', needs: [] }, { id: 'z', part: 'three', needs: [] }];
    const parts = [{ id: 'one' }, { id: 'two' }, { id: 'three' }];
    expect(partNeeds(parts, entries, ['three', 'two', 'one']).get('one')?.needs).toEqual(['three', 'two']);
    expect(partNeeds(parts, entries, ['two']).get('one')?.needs).toEqual(['two', 'three']);
  });
});

describe('the map with the pointer on a part', () => {
  it('marks the parts it needs, fades those that neither it needs nor need it, and says both in the tip', async () => {
    const map = draw(withNeeds());
    await userEvent.hover(part(map, 'part-d'));
    expect(map.querySelector('.ws-stage')).toHaveClass('relating');
    expect(marked(map)).toEqual(['part-a', 'part-b']);
    // Part E needs it, so it is not faded; Part C has nothing to do with it.
    expect(faint(map)).toEqual(['part-c']);
    await shown();
    expect(tip()).toEqual(['A part with copies behind it.', 'Needs Part A, Part B', 'Needed by Part E']);
  });

  it('leaves out the line of a kind that has no part', async () => {
    const map = draw(withNeeds());
    await userEvent.hover(part(map, 'part-a'));
    await shown();
    expect(tip()).toEqual(['A tall part with a glyph.', 'Needed by Part B, Part D, Part E']);
    expect(marked(map)).toEqual([]);
    expect(faint(map)).toEqual(['part-c']);
    await userEvent.unhover(part(map, 'part-a'));
    await userEvent.hover(part(map, 'part-e'));
    await waitFor(() => expect(tip()).toEqual(['A wide bar under the drawing.', 'Needs Part A, Part D']));
  });

  it('gives a part with neither no line in the tip, marks nothing and fades the others', async () => {
    const map = draw(withNeeds());
    await userEvent.hover(part(map, 'part-c'));
    await shown();
    expect(tip()).toEqual(['A part inside Part B.']);
    expect(marked(map)).toEqual([]);
    expect(faint(map)).toEqual(['part-a', 'part-b', 'part-d', 'part-e']);
  });

  it('clears the marks and the fading when the pointer leaves', async () => {
    const map = draw(withNeeds());
    await userEvent.hover(part(map, 'part-d'));
    await userEvent.unhover(part(map, 'part-d'));
    expect(map.querySelector('.ws-stage')).not.toHaveClass('relating');
    expect(marked(map)).toEqual([]);
    expect(faint(map)).toEqual([]);
    expect(document.querySelector('.ws-tip')).toBeNull();
  });

  it('does the same for the keyboard\'s focus, and clears it when the focus leaves', async () => {
    vi.spyOn(HTMLElement.prototype, 'matches').mockImplementation(function (this: HTMLElement, selector: string) { return selector === ':focus-visible'; });
    const map = draw(withNeeds());
    act(() => part(map, 'part-d').focus());
    expect(marked(map)).toEqual(['part-a', 'part-b']);
    await shown();
    expect(tip()).toEqual(['A part with copies behind it.', 'Needs Part A, Part B', 'Needed by Part E']);
    fireEvent.blur(part(map, 'part-d'));
    expect(marked(map)).toEqual([]);
    expect(faint(map)).toEqual([]);
  });

  it('keeps the strip as it is: nothing marked or faded, the tip of a name with the same lines', async () => {
    const map = draw(withNeeds(), true);
    await userEvent.hover(part(map, 'part-d'));
    expect(map.querySelector('.ws-stage')).not.toHaveClass('relating');
    expect(marked(map)).toEqual([]);
    expect(faint(map)).toEqual([]);
    await shown();
    expect(tip()).toEqual(['Part D. A part with copies behind it.', 'Needs Part A, Part B', 'Needed by Part E']);
  });

  it('keeps the tip inside the window, whatever its height', async () => {
    const map = draw(withNeeds());
    const box = { left: 1100, right: 1180, top: 700, bottom: 760, width: 80, height: 60, x: 1100, y: 700, toJSON: () => ({}) };
    vi.spyOn(part(map, 'part-a'), 'getBoundingClientRect').mockReturnValue(box);
    vi.spyOn(HTMLElement.prototype, 'offsetWidth', 'get').mockReturnValue(270);
    vi.spyOn(HTMLElement.prototype, 'offsetHeight', 'get').mockReturnValue(120);
    window.innerWidth = 1200;
    window.innerHeight = 800;
    await userEvent.hover(part(map, 'part-a'));
    await shown();
    // Too low to fit under its part, so above it; too near the right edge, so pulled in to 8 px.
    expect((document.querySelector('.ws-tip') as HTMLElement).style.transform).toBe('translate(922px, 572px)');
  });
});

describe('what a screen reader hears for a part on the map', () => {
  it('gains the two sentences, and each is left out when its kind has no part', () => {
    const map = draw(withNeeds());
    expect(part(map, 'part-d')).toHaveAttribute('aria-label', expect.stringMatching(/ A part with copies behind it\. Needs Part A, Part B\. Needed by Part E\.$/));
    expect(part(map, 'part-a')).toHaveAttribute('aria-label', expect.stringMatching(/ A tall part with a glyph\. Needed by Part B, Part D, Part E\.$/));
    expect(part(map, 'part-e')).toHaveAttribute('aria-label', expect.stringMatching(/ A wide bar under the drawing\. Needs Part A, Part D\.$/));
    expect(part(map, 'part-c')).toHaveAttribute('aria-label', expect.stringMatching(/ A part inside Part B\.$/));
  });
});
