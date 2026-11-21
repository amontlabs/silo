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
  });

  it('gives the key, hidden from screen readers, the page\'s two words', () => {
    const key = draw(buildData()).querySelector('.ws-key');
    expect(key?.getAttribute('aria-hidden')).toBe('true');
    expect([...key!.querySelectorAll('span')].map((line) => line.textContent)).toEqual(['decided', 'not decided']);
    expect([...key!.querySelectorAll('svg')].map((line) => line.classList.contains('undecided'))).toEqual([false, true]);
  });

  it('marks a text that has no folded place, so it leaves the fold', () => {
    const notes = [...draw(buildData()).querySelectorAll('.ws-node')];
    expect(notes.map((note) => note.classList.contains('off-fold'))).toEqual([false, true]);
  });

  it('draws the same wire as decided once the feature it rests on is taken', () => {
    const taken = buildDataFrom(planFiles({ features: FEATURES.map((feature) => (feature.id === 'export' ? { ...feature, verdict: 'take' as const } : feature)) }), { map: EXAMPLE_MAP });
    expect(draw(taken).querySelectorAll('.undecided:not(.ws-key *)')).toHaveLength(0);
  });
});
