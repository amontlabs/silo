// What is around the canvas: the record's dock, and the open place or computer, as the sidebar's highlighted row
// or, on a phone, as a label over the canvas.

import { ROWS, type Recorded, type Reel, type Row } from './reel';
import { END, FADE, TIMED, out, type Key } from './story';
import { E, G, T, diamond, show, textWidth, translate } from './svg';
import { clamp, easeInOut, lerp, part, visible } from './time';

/**
 * While a jump lasts, what is around the canvas goes from the value it had to the value it gets. This is where each
 * value is written down as it is shown, and blended.
 */
export class Shown {
  private values = new Map<string, number>();
  /** The jump under way: what was shown as it started, and how much of it has passed. */
  jump: { from: ReadonlyMap<string, number>; u: number } | null = null;

  blend(key: string, value: number) {
    const from = this.jump?.from.get(key);
    const result = from === undefined ? value : lerp(from, value, easeInOut(this.jump!.u));
    this.values.set(key, result);
    return result;
  }
  set(key: string, value: number) {
    this.values.set(key, value);
  }
  start() {
    this.jump = { from: new Map(this.values), u: 0 };
  }
}

/** The row marked at `t`, the one before it, and how far the mark has come from one to the other. */
function markedAt(reel: Reel, t: number) {
  let before: Row | null = null;
  let now: Row | null = null;
  let since = -1e9;
  for (const [when, row] of reel.lit) {
    if (when > t) break;
    before = now;
    now = row;
    since = when;
  }
  return { before, now, u: easeInOut(part(t, since, FADE)) };
}

const rowY = (row: Row | null) => ROWS.find(([name]) => name === row)?.[1] ?? null;

/** The sidebar's rows, each a way to its shot. */
export function buildSidebar(side: SVGGElement, reel: Reel, go: (key: Key) => void, reduced: boolean) {
  const highlight = E('rect', { x: 8, y: -13, width: 116, height: 26, rx: 3, class: 'f-hl' }, side);
  const rows = ROWS.map(([name, y, key]) => {
    const g = G(side, { 'data-go': key, role: 'button', tabindex: 0, 'aria-label': name });
    let x = 18;
    if (y > 180 && name !== 'Bin') {
      E('rect', { x: 18, y: y - 4.5, width: 9, height: 9, rx: 1.5, class: 'f-box', style: 'stroke-width:1.2' }, g);
      x = 34;
    }
    T(g, x, y + 4.5, name, 'f-chrome');
    const on = T(g, x, y + 4.5, name, 'f-chrome f-on');
    E('rect', { x: 8, y: y - 13, width: 116, height: 26, rx: 3, class: 'f-hit' }, g);
    g.addEventListener('click', () => go(key));
    g.addEventListener('keydown', (event) => {
      if (event.key !== 'Enter' && event.key !== ' ') return;
      event.preventDefault();
      go(key);
    });
    return { name, g, on };
  });
  const badge = E('circle', { cx: 112, cy: 56, r: 4, class: 'f-acc-f' }, side);
  const bin = T(side, 112, 460.5, '1', 'f-chrome', 'end');

  return (t: number, shown: Shown) => {
    const { before, now, u } = markedAt(reel, t);
    const to = rowY(now);
    const from = rowY(before);
    let y: number | null = null;
    let opacity = 0;
    if (to === null && from !== null) [y, opacity] = [from, 1 - u];
    else if (to !== null && from === null) [y, opacity] = [to, u];
    else if (to !== null && from !== null) [y, opacity] = [lerp(from, to, u), 1];
    // The mark slides from one row to the next, on a jump too. It only fades to or from no row, or when motion is reduced.
    if (shown.jump) {
      const was = shown.jump.from.get('y') ?? null;
      const lit = (shown.jump.from.get('lit') ?? 0) > 0.01;
      if (y === null) y = was;
      else if (lit && was !== null && !reduced) y = lerp(was, y, easeInOut(shown.jump.u));
    }
    if (y !== null) {
      translate(highlight, 0, y);
      shown.set('y', y);
    }
    highlight.style.opacity = String(shown.blend('lit', opacity));
    for (const row of rows) {
      row.on.style.opacity = String(shown.blend(`on${row.name}`, row.name === now ? u : row.name === before ? 1 - u : 0));
      const there = reel.present[row.name];
      if (!there) continue;
      const o = shown.blend(`row${row.name}`, visible(t, there[0], there[1])[0]);
      row.g.style.opacity = String(o);
      show(row.g, o >= 0.003);
    }
    badge.style.opacity = String(shown.blend('badge', visible(t, ...reel.badge)[0]));
    bin.style.opacity = String(shown.blend('bin', visible(t, reel.bin, END)[0]));
  };
}

/** The phone's label: one name at a time, so two are never read over each other as they change. */
export function buildPlace(place: HTMLElement, reel: Reel) {
  const names = [...new Set(reel.lit.flatMap(([, row]) => row ?? []))].map((name) => {
    const el = document.createElement('span');
    el.textContent = name;
    return { name, el };
  });
  place.replaceChildren(...names.map(({ el }) => el));

  return (t: number, shown: Shown) => {
    const { before, now, u } = markedAt(reel, t);
    for (const { name, el } of names) {
      const o = shown.blend(`place${name}`, name === now ? clamp(2 * u - 1) : name === before ? clamp(1 - 2 * u) : 0);
      el.style.opacity = String(o);
      el.style.visibility = o >= 0.003 ? '' : 'hidden';
    }
  };
}

/** Where the dock draws: the marks' row, and where the words of the newest mark go. */
interface Dock {
  y: number;
  step: number;
  first: number;
  right: number;
  words(markX: number): readonly [x: number, y: number];
}
export const WINDOW_DOCK: Dock = { y: 462.5, step: 13, first: 152, right: 1104, words: (x) => [x + 14, 467] };
/** On a phone the marks share their row with the key, and the newest words have the row under them. */
export const PHONE_DOCK: Dock = { y: 15, step: 12, first: 16, right: 464, words: () => [11, 42] };

/** When the words of the record's line `i` have come and when they leave. */
function wordsRest(record: readonly Recorded[], i: number) {
  const { t } = record[i];
  const said = t + 120;
  const words = record[i].words.split(/\s+/).length;
  const shot = TIMED.find((s) => t >= s.start && t < s.start + s.dur)!;
  // Its words leave with its shot, or after the cut when they need that to be read, and before the next mark lands.
  // To be read they need 250 ms and 200 ms a word, and 900 ms at least.
  const next = record[i + 1] ? record[i + 1].t - FADE : END;
  return { said, until: Math.min(next, Math.max(out(shot.key), said + FADE + Math.max(900, 250 + 200 * words))) };
}

export function buildRecord(parent: SVGGElement, reel: Reel, dock: Dock) {
  const rise = reel.phone ? 0 : 6;
  // The key to the marks, at the right end. It steps aside in the last shot, where each line names its kind.
  const key = G(parent);
  let x = dock.right;
  for (const [kind, word] of [['o', 'opinion'], ['r', 'report'], ['f', 'fact']] as const) {
    const text = T(key, x, dock.y + 4.5, word, 'f-chrome', 'end');
    x -= textWidth(text) + 12;
    diamond(key, x, dock.y, 4.5, kind);
    x -= reel.phone ? 16 : 22;
  }
  const lines = reel.record.map((line, i) => {
    const markX = dock.first + i * dock.step;
    const mark = G(parent);
    diamond(mark, markX, dock.y, 4.5, line.kind, line.accent ? 'f-acc-f' : null);
    const [wordsX, wordsY] = dock.words(markX);
    const label = G(parent, { 'data-line': i });
    T(label, 0, wordsY, line.words, line.accent ? 'f-chrome f-on f-acc-t' : 'f-chrome f-on');
    return { t: line.t, mark, label, wordsX, ...wordsRest(reel.record, i) };
  });

  return (t: number, shown: Shown) => {
    key.style.opacity = shown.blend('key', visible(t, 300, out('ext'))[0]).toFixed(3);
    lines.forEach((line, i) => {
      const [o, k] = visible(t, line.t, END);
      line.mark.style.opacity = String(shown.blend(`mark${i}`, o));
      translate(line.mark, -k * rise, 0);
      // The newest mark has its words beside it or under it.
      const [target, later] = visible(t, line.said, line.until);
      const opacity = shown.blend(`words${i}`, target);
      line.label.style.opacity = String(opacity);
      show(line.label, opacity >= 0.003);
      translate(line.label, line.wordsX + later * rise, 0);
    });
  };
}
