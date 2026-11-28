import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { loadDir, parsePlan } from '../../../tools/plan/read/index.js';
import { VIEW, viewAt } from './camera';
import { Shown } from './chrome';
import { drawFilm, type Layout } from './drawing';
import { mountFilm } from './film';
import { ROWS, type Motion } from './reel';
import { END, FADE, LENGTH, REACT, SHOTS, STILL, TIMED, at, endOf, shotAt, shotOf, type Key, type Pan, type Span } from './story';
import { CANVAS, offsetOf } from './svg';
import { homePage, measurableSvg, seen } from './test-support';
import { along } from './time';

const page = new DOMParser().parseFromString(homePage(), 'text/html').body.innerHTML;
const $ = <El extends Element>(selector: string) => document.querySelector<El>(selector)!;

beforeEach(() => {
  measurableSvg();
  document.body.innerHTML = page;
});
afterEach(() => vi.unstubAllGlobals());

/** The film drawn in `layout`, and what renders a moment of it. */
function drawn(layout: Layout = 'window') {
  const figure = $('.film figure');
  const drawing = drawFilm(figure, layout, () => {});
  const shown = new Shown();
  return { figure, shown, reel: drawing.reel, render: (t: number) => drawing.render(t, shown) };
}
const stageOf = (figure: Element) => figure.querySelector('.film-window')!;
const onScreen = (el: Element | null): boolean => {
  if (!el || el.matches('svg')) return true;
  const { style } = el as SVGElement;
  return style.visibility !== 'hidden' && !(style.opacity && Number(style.opacity) < 0.003) && onScreen(el.parentElement);
};

describe('the shots', () => {
  it('follow one another, for 73.7 seconds', () => {
    expect(TIMED.map((shot) => shot.name)).toEqual([
      'First run', 'Now', 'main', 'Network', 'Needs you', 'Timeline', 'Disk', 'fork', 'Tests', 'An extension', 'As text',
    ]);
    TIMED.forEach((shot, i) => expect(shot.start).toBe(i ? TIMED[i - 1].start + TIMED[i - 1].dur : 0));
    expect(LENGTH).toBe(73720);
  });

  it('each show entries of the plan', () => {
    const { plan } = parsePlan(loadDir(join(process.cwd(), '../plan')));
    const entries = Object.keys(plan.entries);
    expect(entries.length).toBeGreaterThan(0);
    for (const shot of SHOTS) {
      expect(shot.plan.length, shot.key).toBeGreaterThan(0);
      expect(shot.plan.filter((id) => !entries.includes(id)), `${shot.key} names what is not an entry of the plan`).toEqual([]);
    }
  });

  it('agree with the sidebar', () => {
    const { reel, render, figure } = drawn();
    const names = ROWS.map(([name]) => name as string);
    for (const [, row] of reel.lit) if (row) expect(names).toContain(row);
    for (const [name, , key] of ROWS) {
      const shot = shotOf(key);
      // A row named like a shot goes to that shot.
      const named = TIMED.find((other) => other.name === name);
      if (named) expect(key).toBe(named.key);
      // It is there during the shot it plays.
      const there = reel.present[name];
      if (there) expect(there[0] < shot.start + shot.dur && there[1] > shot.start, name).toBe(true);
      // And its mark is on it at some moment of that shot, when it is the shot's own place or computer.
      if (named || name === 'macOS') {
        const marked = reel.lit.some(([t, row]) => row === name && t >= shot.start - FADE && t < shot.start + shot.dur);
        expect(marked, name).toBe(true);
      }
    }
    // The mark is on the row of the shot as it ends.
    const ends: [Key, string | null][] = [
      ['first', null], ['now', 'Now'], ['main', 'main'], ['network', 'Network'], ['needs', 'Needs you'], ['timeline', 'Timeline'],
      ['disk', 'Disk'], ['fork', 'main'], ['tests', 'macOS'], ['ext', 'Network'], ['text', 'main'],
    ];
    for (const [key, row] of ends) {
      render(endOf(shotOf(key)));
      const on = [...figure.querySelectorAll('.f-side [data-go]')].filter((g) => Number(g.querySelector<SVGElement>('.f-on')!.style.opacity) > 0.5);
      expect(on.map((g) => g.getAttribute('aria-label')), key).toEqual(row ? [row] : []);
    }
  });

  it('never put two computers on one row of the sidebar', () => {
    const { render, figure } = drawn();
    for (let t = 0; t < LENGTH; t += 200) {
      render(t);
      const heights = [...figure.querySelectorAll('.f-side [data-go]')].filter(onScreen).map((g) => g.querySelector('.f-hit')!.getAttribute('y'));
      expect(new Set(heights).size, `at ${t}`).toBe(heights.length);
    }
  });
});

describe('the pointer', () => {
  it('has landed on what it presses before every press, and stays on it while it is down', () => {
    const { reel } = drawn();
    expect(reel.presses.length).toBeGreaterThanOrEqual(8);
    for (const { t, x, y, w, h } of reel.presses) {
      const pointer = reel.pointers.find(({ moves, start, a, b }) => {
        if (a > t || (b !== null && b < t)) return false;
        const [px, py] = along(t, start, moves);
        return px >= x && px <= x + w && py >= y && py <= y + h;
      });
      expect(pointer, `no pointer is on what is pressed at ${t}`).toBeDefined();
      for (const leg of pointer!.moves) {
        const landed = leg.t + leg.d <= t - REACT;
        expect(landed || leg.t >= t + 300, `a press at ${t}, while the pointer moves from ${leg.t}`).toBe(true);
      }
    }
  });
});

describe('the record', () => {
  /** How long a text has to rest to be read. */
  const readable = (words: string) => 250 + 200 * words.split(/\s+/).length;

  it('rests every line at full ink long enough to be read, 250 ms and 200 ms a word, one line at a time', { timeout: 30_000 }, () => {
    const { reel, render, figure } = drawn();
    const labels = [...figure.querySelectorAll<SVGElement>('[data-line]')];
    expect(labels).toHaveLength(reel.record.length);
    reel.record.forEach((line, i) => {
      let from: number | null = null;
      let to = 0;
      for (let t = line.t; t < Math.min(line.t + 2200, LENGTH); t += 10) {
        render(t);
        if (Number(labels[i].style.opacity) < 1) {
          if (from !== null) break;
          continue;
        }
        from ??= t;
        to = t;
        expect(labels.filter(onScreen), `${line.words} at ${t}`).toEqual([labels[i]]);
      }
      expect(to - (from ?? to), line.words).toBeGreaterThanOrEqual(readable(line.words));
    });
  });
});

describe('orange', () => {
  it('is only on the parts of the request to paste.rs', () => {
    const { figure } = drawn();
    const parts = [...stageOf(figure).querySelectorAll('[class*="f-acc"]')].map((el) => {
      const classes = [...el.classList].filter((name) => name.startsWith('f-acc')).join();
      const shot = el.closest('[data-shot]')?.getAttribute('data-shot');
      const where = shot ?? (el.closest('.f-side') ? 'sidebar' : el.closest('.f-canvas') ? 'request' : 'record');
      return `${where} ${el.tagName} ${classes} ${el.textContent}`.trim();
    });
    expect(parts.sort()).toEqual(
      [
        // Network: its line and its verdict.
        'network line f-acc-s', 'network text f-acc-t denied',
        // The request itself, which the film follows: a dot, then an alert's diamond.
        'request circle f-acc-f', 'request path f-acc-f',
        // Needs you: its badge and its alert.
        'sidebar circle f-acc-f', 'needs rect f-acc', 'needs text f-acc-t denied',
        // Timeline: where its chain starts.
        'timeline text f-acc-t denied',
        // The same request from the fork.
        'fork line f-acc-s', 'fork circle f-acc-f', 'fork text f-acc-t denied',
        // Its line in the record, in the dock and as text.
        'record path f-acc-f', 'record text f-acc-t paste.rs denied',
        'text path f-acc-f', 'text text f-acc-t paste.rs · denied',
      ].sort(),
    );
  });

  it('is not seen before that request sets off', () => {
    const { figure, render } = drawn();
    const parts = [...stageOf(figure).querySelectorAll('[class*="f-acc"]')];
    for (let t = 0; t <= at('network')(5100); t += 100) {
      render(t);
      expect(parts.filter(onScreen), `at ${t}`).toEqual([]);
    }
  });

  it('comes from no other class and no style', () => {
    const css = readFileSync(join(process.cwd(), 'src/film/film.css'), 'utf8').replace(/\/\*[\s\S]*?\*\//g, '');
    for (const [, selector, body] of css.matchAll(/([^{}]+)\{([^{}]*)\}/g)) {
      // The ring of a focused control is the page's.
      if (/var\(--accent/.test(body) && !selector.includes(':focus-visible')) expect(selector).toMatch(/\.f-acc/);
    }
    const { figure, render } = drawn();
    render(STILL);
    expect(stageOf(figure).querySelectorAll('[style*="accent"]')).toHaveLength(0);
  });
});

describe('the drawing', () => {
  it.each(['window', 'phone'] as const)('ends the loop on the frame it starts from (%s)', (layout) => {
    const { figure, render } = drawn(layout);
    const picture = () => {
      const parts = layout === 'window' ? [stageOf(figure)] : [...figure.querySelectorAll('.film-view, .film-dock, .film-place')];
      return parts.map(seen).join('');
    };
    render(0);
    const first = picture();
    render(LENGTH - 1);
    expect(picture()).toBe(first);
  });

  it('uses no id, and no class outside its own names', () => {
    const { figure } = drawn();
    const stage = stageOf(figure);
    expect(stage.querySelectorAll('[id]')).toHaveLength(0);
    expect([...stage.querySelectorAll('[class]')].flatMap((el) => [...el.classList]).filter((name) => !name.startsWith('f-'))).toEqual([]);
  });

  it('says nothing the writing guide rules out', () => {
    const { figure } = drawn();
    expect(stageOf(figure).textContent).not.toMatch(/sandbox|\bsafe\b|secure|proven|dashboard|monitor|observab/i);
  });

  it('has for its still, without script, the words of the end of Network', () => {
    const served = new DOMParser().parseFromString(homePage(), 'text/html').querySelector('.film-window')!;
    const { figure, render } = drawn();
    render(STILL);
    const words = (root: Element) => [...root.querySelectorAll('text')].filter(onScreen).flatMap((el) => el.textContent || []).sort();
    expect(words(served)).toEqual(words(stageOf(figure)));
  });
});

describe("the phone's camera", () => {
  const pans = TIMED.flatMap((shot) => shot.camera.pans.map((pan) => ({ ...pan, shot: shot.key, t: shot.start + pan.t })));
  /** What moves in the camera's view during `pan` and is not the one thing it follows, moving with it. */
  const besides = (pan: Pan, motions: readonly Motion[]) =>
    motions.filter(({ t, d, of, across }) => {
      if (t >= pan.t + pan.d || pan.t >= t + d) return false;
      if (pan.follows && of === pan.follows && t === pan.t && d === pan.d) return false;
      if (!across) return true;
      // What says where it keeps to is out of it all the while.
      for (let u = Math.max(t, pan.t); u < Math.min(t + d, pan.t + pan.d); u += 10) {
        if (viewAt(u) < across[1] && viewAt(u) + VIEW > across[0]) return true;
      }
      return false;
    });

  it('frames parts of the canvas no wider than its view', () => {
    for (const span of TIMED.flatMap(({ camera }) => [...(camera.from ? [camera.from] : []), ...camera.pans.map((pan) => pan.to)])) {
      expect(span[1] - span[0]).toBeLessThanOrEqual(VIEW);
      expect(span[0]).toBeGreaterThanOrEqual(0);
      expect(span[1]).toBeLessThanOrEqual(CANVAS.w);
    }
  });

  it('moves once at a time, only while nothing else moves or with the one thing it follows', () => {
    const { reel } = drawn('phone');
    pans.forEach((pan, i) => {
      const said = `${pan.shot}, the pan at ${pan.t}`;
      if (i) expect(pan.t, said).toBeGreaterThanOrEqual(pans[i - 1].t + pans[i - 1].d);
      expect(besides(pan, reel.motions), said).toEqual([]);
      if (pan.follows) {
        const followed = reel.motions.some(({ t, d, of }) => of === pan.follows && t === pan.t && d === pan.d);
        expect(followed, `${said} moves with ${pan.follows}`).toBe(true);
      }
      // A second thing that moves with it is found, whatever its timing.
      expect(besides(pan, [...reel.motions, { t: pan.t, d: pan.d }]), said).toHaveLength(1);
    });
  });

  it('has finished its last move by the moment a paused jump shows, and ends on what the shot is about', () => {
    const { render, figure } = drawn('phone');
    const canvas = figure.querySelector('.film-view .f-canvas')!;
    /** Where `el` is drawn on the canvas, from its left to its right, as it has been moved. */
    const where = (el: Element): Span => {
      let x = 0;
      for (let up: Element | null = el; up && up !== canvas; up = up.parentElement) x += offsetOf(up);
      const number = (name: string) => Number(el.getAttribute(name) ?? 0);
      if (el.tagName === 'rect') return [x + number('x'), x + number('x') + number('width')];
      if (el.tagName === 'text') {
        const box = (el as SVGTextElement).getBBox();
        return [x + box.x, x + box.x + box.width];
      }
      // A mark's path has only moves and lines: its numbers are pairs.
      const xs = el.getAttribute('d')!.match(/-?[\d.]+/g)!.map(Number).filter((_, i) => i % 2 === 0);
      return [x + Math.min(...xs), x + Math.max(...xs)];
    };
    const find = (shot: Key, selector: string, words: readonly string[] = []) => {
      const all = [...canvas.querySelectorAll(`[data-shot="${shot}"] ${selector}`)].filter(onScreen);
      const found = words.length ? words.flatMap((word) => {
        const said = all.filter((el) => el.textContent === word);
        expect(said.length, `${shot} says ${word} at its end`).toBeGreaterThan(0);
        return said;
      }) : all;
      expect(found.length, `${shot} has ${selector} at its end`).toBeGreaterThan(0);
      return found.map(where);
    };
    // What each shot is about, found in its drawing as it ends. One view cannot hold all of Now, Needs you or Disk:
    // each ends on the part that matters most, and has been to the others as they changed.
    const ABOUT: Partial<Record<Key, () => Span[]>> = {
      // main, the box that has its name
      first: () => {
        const name = [...canvas.querySelectorAll('[data-shot="first"] text')].find((el) => el.textContent === 'main')!;
        return [where(name.parentElement!.querySelector('rect')!)];
      },
      // the runs' names and commands, and the stuck run's opinion
      now: () => [
        ...find('now', 'text', ['Claude Code', 'Codex', 'npm install', 'cargo test', 'sudo npm install -g pnpm', 'stuck']),
        where([...canvas.querySelectorAll('[data-shot="now"] text')].find((el) => el.textContent === 'stuck')!.parentElement!.querySelector('rect')!),
      ],
      // the alert's words and the question
      needs: () => find('needs', 'text', ['new host', 'paste.rs', 'denied', 'Is', '~/api/target', 'build output?']),
      // the chain's labels, the muted ones among them, and the mark it starts from
      timeline: () => [
        ...find('timeline', 'text.f-muted', ['paste.rs', 'denied', 'curl', 'node postinstall.js', 'npm install', 'turn 14', 'Claude Code']),
        where([...canvas.querySelectorAll('path.f-acc-f')].find((el) => onScreen(el) && !el.closest('[data-shot]'))!),
      ],
      // main's rebuildable size after the purge, and the pattern
      disk: () => find('disk', 'text', ['rebuild', '0 GB', 'npm install', '41× in 12 runs']),
      // the fork's edge, its denied request and the no rule gate it stopped at
      fork: () => [
        ...find('fork', 'rect.f-ghost').map(([, right]): Span => [right, right]),
        ...find('fork', 'rect.f-gate'),
        ...find('fork', 'text', ['denied', 'no rule']),
      ],
      // the three gates, their rules and their verdicts
      network: () => [...find('network', 'rect.f-gate'), ...find('network', 'text', ['stripe v2', 'registries v3', 'no rule', 'allowed', 'denied'])],
    };
    for (const shot of TIMED) {
      const last = shot.camera.pans.at(-1);
      if (last) expect(last.t + last.d, `${shot.key}'s last move`).toBeLessThanOrEqual(endOf(shot) - shot.start);
      const about = ABOUT[shot.key];
      if (!about) continue;
      render(endOf(shot));
      const left = viewAt(endOf(shot));
      for (const [from, to] of about()) {
        expect(left, `${shot.key} at its end, from ${from}`).toBeLessThanOrEqual(from);
        expect(left + VIEW, `${shot.key} at its end, to ${to}`).toBeGreaterThanOrEqual(to);
      }
    }
  });

  it('cuts only while the canvas is empty', () => {
    const { render, figure } = drawn('phone');
    for (const shot of TIMED.filter(({ camera }) => camera.from)) {
      render(shot.start);
      expect(seen(figure.querySelector('.film-view')!), shot.key).toBe('');
    }
  });

  it('stands still outside its pans', () => {
    for (let t = 0; t < LENGTH - 20; t += 20) {
      if (viewAt(t) === viewAt(t + 20)) continue;
      const cut = TIMED.some(({ start, camera }) => camera.from && start >= t && start <= t + 20);
      const panning = pans.some((pan) => pan.t < t + 20 && t < pan.t + pan.d);
      expect(cut || panning, `at ${t}`).toBe(true);
    }
  });
});

describe('the player', () => {
  // 300 seeded steps that each draw the film twice: a few seconds of work on a fast computer, so the default 5 s is too
  // near on a busy one.
  it.each(['window', 'phone'] as const)('shows, whatever was pressed, the frame that a hold of the same moment shows (%s)', { timeout: 30_000 }, (layout) => {
    let now = 1000;
    let waitingFrames: FrameRequestCallback[] = [];
    vi.stubGlobal('requestAnimationFrame', (run: FrameRequestCallback) => waitingFrames.push(run));
    vi.stubGlobal('cancelAnimationFrame', () => (waitingFrames = []));
    vi.stubGlobal('performance', { now: () => now });
    vi.stubGlobal('matchMedia', (query: string) => ({ matches: layout === 'phone' && query.includes('999') }));

    const root = $<HTMLElement>('.film');
    const copy = root.cloneNode(true) as HTMLElement;
    document.body.append(copy);
    const film = mountFilm(root, { reduced: false, inView: true });
    const reference = mountFilm(copy, { reduced: false, inView: true, hold: 0 });

    // The model: what the moment has to be.
    const model = { clock: 0, paused: false, reduced: false, allowed: false, last: null as number | null, landsAt: 0 };
    const running = () => model.allowed && !model.paused;
    const picture = (el: HTMLElement) => {
      const parts = layout === 'window' ? ['.film-window'] : ['.film-view', '.film-dock', '.film-place'];
      return parts.map((part) => seen(el.querySelector(part)!)).join('');
    };
    const bars = (el: HTMLElement) => [...el.querySelectorAll<HTMLElement>('.film-progress i')].map((bar) => bar.style.transform).join();
    const segments = [...root.querySelectorAll<HTMLButtonElement>('.film-progress button')];

    // A seeded generator (Park-Miller), so every run presses the same.
    let seed = 11;
    const pick = (below: number) => Math.floor(((seed = (seed * 48271) % 2147483647) / 2147483647) * below);
    let compared = 0;
    for (let step = 0; step < 300; step++) {
      const what = pick(11);
      if (what === 0) {
        const i = pick(segments.length);
        segments[i].click();
        model.clock = model.paused ? endOf(TIMED[i]) : TIMED[i].start;
        model.last = null;
        model.landsAt = model.reduced ? 0 : now + 150;
      } else if (what === 1) {
        root.querySelector<HTMLButtonElement>('.film-pause')!.click();
        model.paused = !model.paused;
        model.last = null;
      } else if (what === 2) {
        model.allowed = !model.allowed || pick(3) === 0;
        film.allow(model.allowed);
        if (!running()) model.last = null;
      } else if (what === 3) {
        model.reduced = !model.reduced;
        if (model.reduced) {
          model.paused = true;
          model.clock = endOf(shotAt(model.clock));
          model.last = null;
          model.landsAt = 0;
        }
        film.reduce(model.reduced);
        reference.reduce(model.reduced);
      } else {
        now += 16 + pick(900);
        const frames = waitingFrames;
        waitingFrames = [];
        for (const run of frames) run(now);
        if (frames.length) {
          if (running()) {
            if (model.last !== null) model.clock = (model.clock + (now - model.last)) % LENGTH;
            model.last = now;
          } else {
            model.last = null;
          }
          if (now >= model.landsAt) model.landsAt = 0;
        }
      }
      expect(film.running, `step ${step}`).toBe(running());
      expect(film.moving, `step ${step}`).toBe(running() || model.landsAt > 0);
      reference.hold(model.clock);
      expect(bars(root), `step ${step}`).toBe(bars(copy));
      // While a jump crosses over, two frames are on screen.
      if (model.landsAt) continue;
      expect(picture(root), `step ${step}, at ${model.clock}`).toBe(picture(copy));
      compared++;
    }
    expect(compared).toBeGreaterThan(100);
  });

  it('slides the mark between two rows, and with reduced motion never does, played or held', () => {
    const { render, shown, reel, figure } = drawn();
    const mark = () => figure.querySelector('.f-hl')!.getAttribute('transform');
    let slid = 0;
    for (const [i, [t, row]] of reel.lit.entries()) {
      if (!row || !i || !reel.lit[i - 1][1]) continue;
      const y = ROWS.find(([name]) => name === row)![1].toFixed(2);
      shown.reduced = false;
      render(t + FADE / 4);
      if (mark() !== `translate(0.00 ${y})`) slid++;
      shown.reduced = true;
      render(t + FADE / 4);
      expect(mark(), `${row} at ${t}`).toBe(`translate(0.00 ${y})`);
    }
    expect(slid).toBeGreaterThan(5);
  });
});
