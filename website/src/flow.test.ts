import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { END, STEPS, STILL_STEP, endOf, film } from './flow';

// The page as it is served: the film's stage with its parts, before any script has drawn it.
const page = new DOMParser().parseFromString(readFileSync(join(process.cwd(), 'index.html'), 'utf8'), 'text/html');
const css = readFileSync(join(process.cwd(), 'src/flow.css'), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '');

/** A fresh stage, across or in one column (the composition CSS would choose at that width). */
function stage(mode: 'wide' | 'tall') {
  const root = document.createElement('div');
  root.innerHTML = page.querySelector('.stage')!.outerHTML;
  const el = root.firstElementChild as HTMLElement;
  if (mode === 'tall') el.style.setProperty('--mode', 'tall');
  document.body.append(root);
  return el;
}

/** What the film wrote on each part, as one rule per part, keyed by a selector that finds only that part. */
function written(el: HTMLElement) {
  const rules: string[] = [];
  const selector = (part: Element): string => {
    const own = part.classList.length ? [...part.classList].map((name) => `.${name}`).join('') : part.tagName.toLowerCase();
    const parent = part.parentElement!;
    const same = [...parent.children].filter((sibling) => sibling.matches(own));
    const here = same.length > 1 ? `${own}:nth-child(${[...parent.children].indexOf(part) + 1})` : own;
    return parent === el ? here : `${selector(parent)} ${here}`;
  };
  for (const part of el.querySelectorAll<HTMLElement | SVGElement>('*')) {
    const values = [...part.style]
      .filter((name) => name.startsWith('--'))
      .map((name) => [name, part.style.getPropertyValue(name)] as const)
      // The values every part has without a rule: hidden, in place, at its own size.
      .filter(([name, value]) => value !== (name === '--s' ? '1' : '0'));
    if (values.length) rules.push(`${selector(part)} { ${values.map(([name, value]) => `${name}: ${value};`).join(' ')} }`);
  }
  return rules;
}

/** The rules of the still in flow.css: across, or in one column. */
function still(mode: 'wide' | 'tall') {
  const tall = css.lastIndexOf('@media (max-width: 700px)');
  const block = mode === 'wide' ? css.slice(0, tall) : css.slice(tall);
  return (block.match(/^\s*\.scene[^{}]*\{[^{}]*\}/gm) ?? []).map((rule) => rule.trim().replace(/\s+/g, ' '));
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['requestAnimationFrame', 'cancelAnimationFrame', 'performance'] });
  document.body.innerHTML = '';
});
afterEach(() => {
  vi.useRealTimers();
});

describe('the film', () => {
  for (const mode of ['wide', 'tall'] as const) {
    it(`shows the last moment of Boundary from its markup and flow.css alone (${mode})`, () => {
      const el = stage(mode);
      const classes = [...el.querySelectorAll('*')].map((part) => part.getAttribute('class'));
      const text = el.textContent;
      film(el).hold(endOf(STILL_STEP));
      // Drawing the still changes no class and no text: only the numbers, which flow.css holds.
      expect([...el.querySelectorAll('*')].map((part) => part.getAttribute('class'))).toEqual(classes);
      expect(el.textContent).toBe(text);
      const rules = written(el);
      for (const rule of rules) expect(el.querySelectorAll(rule.slice(0, rule.indexOf(' {'))), rule).toHaveLength(1);
      expect(still(mode).join('\n')).toBe(rules.join('\n'));
    });
  }

  for (const mode of ['wide', 'tall'] as const) {
    it(`shows the same stage when held at a moment as when played up to it, in every step (${mode})`, () => {
      const played = stage(mode);
      const held = stage(mode);
      const playing = film(played);
      const holding = film(held);
      playing.play(0);
      let passed = 0;
      for (let i = 0; i < STEPS.length; i++) {
        // A frame comes every 16 ms: these moments are frames, the middle of a step and its last frame.
        for (const at of [STEPS[i] + (endOf(i) - STEPS[i]) / 2, endOf(i)].map((t) => Math.floor(t / 16) * 16)) {
          vi.advanceTimersByTime(at - passed);
          passed = at;
          holding.hold(at);
          expect(played.innerHTML, `${mode} at ${at}`).toBe(held.innerHTML);
        }
      }
      // At its end the film stops drawing until it is played again.
      vi.advanceTimersByTime(END);
      expect(vi.getTimerCount()).toBe(0);
    });
  }

  it('runs nothing while it is held', () => {
    const el = stage('wide');
    const picture = film(el);
    picture.play(0);
    expect(vi.getTimerCount()).toBe(1);
    picture.hold(STEPS[2]);
    expect(vi.getTimerCount()).toBe(0);
  });
});
