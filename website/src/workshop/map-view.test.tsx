import { render } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { buildData, FEATURES, planFiles } from './fixtures';
import { buildDataFrom } from './data';
import { EXAMPLE_MAP } from './map-example';
import { MapView } from './MapView';
import { buildPlan } from './plan';
import type { BuildData } from './plan';

const draw = (data: BuildData, folded = false) => render(<MapView layout={data.map} groups={buildPlan(data, null).parts} folded={folded} current={null} onOpen={() => {}} />).container;

describe('what the map draws as not decided', () => {
  it('draws a wire that is not decided dashed on the full map and on the fold, its word in the lighter ink, in its part\'s colour', () => {
    const map = draw(buildData());
    const full = [...map.querySelectorAll('.w-full .ws-wire')];
    const fold = [...map.querySelectorAll('.w-fold .ws-wire')];
    expect(full.map((wire) => wire.classList.contains('undecided'))).toEqual([false, false, false, true]);
    // The wire that stops at the cross leaves the fold; the last wire's fold is drawn as it is.
    expect(fold.map((wire) => wire.classList.contains('undecided'))).toEqual([false, false, true]);
    expect(full[3].classList.contains('s-open')).toBe(true);
    expect(fold[2].classList.contains('s-open')).toBe(true);
    // Each word sits on its wire: the last wire's is the one that is not decided, and it carries the mark and the part's colour too.
    const words = [...map.querySelectorAll('.ws-wl')];
    expect(words.map((word) => word.textContent)).toEqual(['uses', 'leads to', 'stops', 'exports']);
    expect(words.map((word) => word.classList.contains('undecided'))).toEqual([false, false, false, true]);
    expect(words[3].classList.contains('s-open')).toBe(true);
  });

  it('gives the key, hidden from screen readers, the page\'s two words', () => {
    const key = draw(buildData()).querySelector('.ws-mapkey');
    expect(key?.getAttribute('aria-hidden')).toBe('true');
    expect([...key!.querySelectorAll('span')].map((line) => line.textContent)).toEqual(['decided', 'not decided']);
    expect([...key!.querySelectorAll('svg')].map((line) => line.classList.contains('undecided'))).toEqual([false, true]);
  });

  it('marks a text that has no folded place, so it leaves the fold', () => {
    const notes = [...draw(buildData()).querySelectorAll('.ws-node')];
    expect(notes.map((note) => note.classList.contains('off-fold'))).toEqual([false, true]);
  });

  it('draws a text with no folded place where it stands on a folded map, marked to fade out, and the other at its folded place', () => {
    const map = draw(buildData(), true);
    expect(map.querySelector('.ws-stage')?.classList.contains('fold')).toBe(true);
    const [first, second] = [...map.querySelectorAll<HTMLElement>('.ws-node')];
    expect(first.classList.contains('off-fold')).toBe(false);
    expect(first.style.getPropertyValue('--cx')).toBe('952');
    expect(second.classList.contains('off-fold')).toBe(true);
    // Its folded place is its full one, so it stays where it is as it fades.
    expect(second.style.getPropertyValue('--cx')).toBe(second.style.getPropertyValue('--x'));
    expect(second.style.getPropertyValue('--cy')).toBe(second.style.getPropertyValue('--y'));
  });

  it('keeps the key of the header apart from the key of the map', () => {
    const map = draw(buildData());
    expect(map.querySelectorAll('.ws-key')).toHaveLength(0);
    expect(map.querySelectorAll('.ws-mapkey')).toHaveLength(1);
  });

  it('draws the same wire as decided once the feature it rests on is taken', () => {
    const taken = buildDataFrom(planFiles({ features: FEATURES.map((feature) => (feature.id === 'export' ? { ...feature, verdict: 'take' as const } : feature)) }), { map: EXAMPLE_MAP });
    expect(draw(taken).querySelectorAll('.undecided:not(.ws-mapkey *)')).toHaveLength(0);
  });
});
