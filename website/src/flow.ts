// The main picture of the home page: a film of eight steps, from main and its three agents to a fork dropped after
// one file is brought back. Every part is placed from the moment alone (`draw`), so holding the film at a moment shows
// exactly what playing up to it shows. The parts are in the page's markup and flow.css sizes them; `draw` writes, on
// each part, the numbers of the moment as custom properties: where it is, how big, how visible. The markup and flow.css
// alone show the last moment of Boundary, the still. The flow plays the film in a loop, and answers Pause, Play and the
// step names.

import type { Scene } from './scene';

/** When each step starts, and when the whole film ends, in milliseconds. */
export const STEPS = [0, 3200, 7200, 11200, 15600, 19600, 22600, 26600];
export const END = 31000;
/** The last moment of a step. */
export const endOf = (step: number) => (step < STEPS.length - 1 ? STEPS[step + 1] : END) - 1;
/** The step shown when nothing may move on its own: Boundary, a request passed and one denied at the edge of main. */
export const STILL_STEP = 3;
const stepAt = (t: number) => STEPS.findLastIndex((start) => start <= t);

// ---- Motion ---------------------------------------------------------------------------------------------------------

/** How long a shot takes to settle into its places. */
const SETTLE = 420;
const clamp = (v: number, a = 0, b = 1) => Math.min(b, Math.max(a, v));
const easeOut = (p: number) => 1 - Math.pow(1 - clamp(p), 3);
const smooth = (p: number) => {
  p = clamp(p);
  return p * p * (3 - 2 * p);
};
/** From 0 at `a` to 1 at `b`, eased out. */
const ramp = (t: number, a: number, b: number) => easeOut((t - a) / (b - a));
/** From 0 at `a` to 1 at `b`, at a constant speed. */
const lin = (t: number, a: number, b: number) => clamp((t - a) / (b - a));
const lerp = (a: number, b: number, p: number) => a + (b - a) * p;
/** A moment of step `i`. */
const T = (i: number, ms: number) => STEPS[i] + ms;
/** A press: up and down again over `d` from `t0`. */
const bump = (t: number, t0: number, d = 260) => (t < t0 || t > t0 + d ? 0 : Math.sin((Math.PI * (t - t0)) / d));

// ---- Where every part sits in every shot ----------------------------------------------------------------------------

type Shot = Record<string, number>;
/** One shot per step; `null` where the part is not in that shot. */
type Track = (Shot | null)[];

const S = (x: number, y: number, w: number, h: number, o = 1): Shot => ({ x, y, w, h, o });
const line = (x0: number, x1: number, y: number, size: number): Shot => ({ x0, x1, y, size, o: 1 });

/** A part that is not in a shot waits, invisible, where it was last (or will be first). */
function resolve(track: Track): Shot[] {
  return track.map((shot, i) => {
    if (shot) return shot;
    let j = i - 1;
    while (j >= 0 && !track[j]) j--;
    if (j < 0) {
      j = i + 1;
      while (j < track.length && !track[j]) j++;
    }
    return { ...track[j]!, o: 0 };
  });
}

/** Where a part is at `t`: in its shot, or on its way there from the previous one. */
function sample(track: Shot[], t: number): Shot {
  const s = stepAt(t);
  const to = track[s];
  if (s === 0) return to;
  const p = easeOut((t - STEPS[s]) / SETTLE);
  if (p >= 1) return to;
  const from = track[s - 1];
  const out: Shot = {};
  for (const k of new Set([...Object.keys(from), ...Object.keys(to)])) out[k] = lerp(from[k] ?? 0, to[k] ?? 0, p);
  return out;
}

interface Geo {
  tall: boolean;
  mac: Track;
  main: Track;
  /** The three agents' rows in main: where the row is, where its name and its command sit, and whether the command shows. */
  rows: Track[];
  rec: Track;
  list: Track;
  tl: Track;
  /** paste.rs and crates.io, outside your Mac. */
  h1: Track;
  h2: Track;
  fork: Track;
  /** What is only in the fork, listed before the drop. */
  only: Track;
  bin: Track;
  /** Where the fork waits in the bin. */
  binned: Shot;
  /** How far left of main's edge a request's wire starts. */
  reqFrom: number;
  /** The first row of a list of what needs you, from the list's corner. */
  needRow: { y: number; h: number; x: number };
  /** The first file only in the fork, from the corner of its list. */
  onlyRow: number;
  /** Where a file brought back lands in main, from main's corner. */
  got: { x: number; y: number };
  /** How tall main grows in Drop to hold the file brought back. */
  holds: number;
  /** The widest bar of the list ranked by cost. */
  bar: number;
}

function agentRows(i: number, tall: boolean): Track {
  const row = (y: number, h: number, ny: number, cx: number, cy: number, co: number, o = 1): Shot => ({ y, h, ny, cx, cy, co, o });
  const cx = tall ? 28 : 30;
  const later = row(48 + i * 52, 52, i === 1 ? 6 : 16, cx, 27, i === 1 ? 1 : 0);
  const record = row(48 + i * 52, 52, 16, cx, 27, 0);
  const drop = { ...later, o: 0 };
  const start = tall ? row(48 + i * 56, 56, 18, 0, 18, 0) : row(48 + i * 60, 60, 20, 0, 20, 0);
  const full = tall ? row(48 + i * 66, 66, 8, 28, 34, 1) : row(48 + i * 52, 52, 16, 270, 16, 1);
  return [start, full, record, later, later, later, later, drop];
}

function wide(): Geo {
  const all = S(1, 14, 998, 365);
  const chip = (x: number, y: number) => S(x, y, 150, 40);
  return {
    tall: false,
    mac: [all, all, all, S(1, 14, 700, 365), all, all, all, all],
    main: [S(200, 64, 600, 238), S(90, 84, 820, 214), S(598, 34, 372, 250), S(366, 34, 306, 250), chip(24, 328), chip(24, 328), chip(24, 88), chip(24, 88)],
    rows: [0, 1, 2].map((i) => agentRows(i, false)),
    rec: [null, null, S(24, 34, 544, 254), S(24, 34, 318, 254), S(520, 36, 440, 256), S(520, 36, 440, 256, 0), null, null],
    list: [null, null, null, null, S(40, 36, 440, 256), S(60, 26, 880, 286), null, null],
    tl: [null, null, line(24, 976, 348, 9), line(24, 676, 348, 9), line(196, 976, 348, 9), line(196, 976, 348, 9), line(196, 976, 108, 13), line(196, 976, 108, 13)],
    h1: [null, null, null, S(778, 142, 196, 36), null, null, null, null],
    h2: [null, null, null, S(778, 194, 196, 36), null, null, null, null],
    fork: [null, null, null, null, null, null, S(266, 200, 620, 116), S(460, 210, 250, 104)],
    only: [null, null, null, null, null, null, null, S(24, 210, 420, 128)],
    bin: [null, null, null, null, null, null, null, S(726, 210, 250, 80)],
    binned: S(738, 238, 226, 40),
    reqFrom: 90,
    needRow: { y: 44, h: 64, x: 300 },
    onlyRow: 44,
    got: { x: 12, y: 44 },
    holds: 80,
    bar: 836,
  };
}

function tall(): Geo {
  const all = S(1, 12, 338, 606);
  const chip = (x: number, y: number) => S(x, y, 140, 40);
  return {
    tall: true,
    mac: [all, all, all, S(1, 12, 236, 606), all, all, all, all],
    main: [S(10, 200, 320, 226), S(10, 180, 320, 256), S(10, 296, 320, 214), S(6, 296, 226, 214), chip(10, 532), chip(10, 532), chip(10, 150), chip(10, 150)],
    rows: [0, 1, 2].map((i) => agentRows(i, true)),
    rec: [null, null, S(10, 24, 320, 253), S(6, 24, 226, 253), S(10, 262, 320, 252), S(10, 262, 320, 252, 0), null, null],
    list: [null, null, null, null, S(10, 24, 320, 226), S(10, 24, 320, 334), null, null],
    tl: [null, null, line(10, 330, 580, 8), line(10, 226, 580, 8), line(10, 330, 594, 8), line(10, 330, 594, 8), line(20, 320, 238, 13), line(20, 320, 238, 13)],
    h1: [null, null, null, S(246, 407, 86, 30), null, null, null, null],
    h2: [null, null, null, S(246, 459, 86, 30), null, null, null, null],
    fork: [null, null, null, null, null, null, S(10, 318, 320, 112), S(10, 412, 320, 104)],
    only: [null, null, null, null, null, null, null, S(10, 270, 320, 128)],
    bin: [null, null, null, null, null, null, null, S(10, 530, 320, 80)],
    binned: S(22, 558, 296, 40),
    reqFrom: 60,
    needRow: { y: 44, h: 52, x: 230 },
    onlyRow: 44,
    got: { x: 12, y: 40 },
    holds: 72,
    bar: 282,
  };
}

// ---- What happens, and when ----------------------------------------------------------------------------------------

/** When each agent starts typing its command in Full access. */
const TYPED_AT = [300, 1900, 2800];
const COMMANDS = ['sudo apt install postgresql-17', 'npm install', 'cargo build'];
const CURL = 'curl paste.rs';
const TYPE_MS = 38;
/** When each line of the record is written. */
const RECORD_AT = [T(2, 700), T(2, 1200), T(2, 1700), T(2, 2200), T(2, 2700), T(3, 1550), T(3, 3700)];
/** How long each line of the list ranked by cost is, against the longest. */
const COSTS = [38, 12, 3];
/** When each checkpoint is taken. The last one is taken after the fork starts: main keeps running. */
const CHECKPOINTS = [8200, 9800, 11900, 13700, 16000, 17400, 19200, 20400, 21800, T(6, 3300)];
/** The checkpoint before turn 14, which you go back to. */
const BACK = 2;

const cpAt = (tl: Shot, k: number) => ({ x: lerp(tl.x0, tl.x1, (k + 1) / 11), y: tl.y });

// ---- The picture ----------------------------------------------------------------------------------------------------

type Part = HTMLElement | SVGElement;

/**
 * The film in `stage`, as a scene: held at a moment, or played on from one. While it is held nothing runs; while it
 * plays one frame loop draws it. Which composition it uses, across or in one column, CSS says (`--mode`), and it is
 * read again each time the film is held or played.
 */
export function film(stage: HTMLElement): Scene {
  const q = (selector: string) => stage.querySelector<Part>(selector)!;
  const qa = (selector: string) => [...stage.querySelectorAll<Part>(selector)];
  const written = new Map<Part, Map<string, string>>();
  /** Writes the numbers of the moment on a part, only where they changed. */
  const put = (part: Part, values: Record<string, number>) => {
    let had = written.get(part);
    if (!had) written.set(part, (had = new Map()));
    for (const name in values) {
      const value = String(Math.round(values[name] * 100) / 100);
      if (had.get(name) === value) continue;
      had.set(name, value);
      part.style.setProperty(`--${name}`, value);
    }
  };
  const box = (part: Part, s: Shot, more: Record<string, number> = {}) => put(part, { x: s.x, y: s.y, w: s.w, h: s.h, o: clamp(s.o), ...more });
  const toggle = (part: Part, name: string, on: boolean) => {
    if (part.classList.contains(name) !== on) part.classList.toggle(name, on);
  };

  const P = {
    scene: q('.scene'),
    mac: q('.mac'),
    macName: q('.mac-name'),
    tl: q('.tl'),
    tlLine: q('.tl-line'),
    cps: qa('.cp'),
    cpName: q('.cp-name'),
    main: q('.main'),
    rows: qa('.main .ag').map((row) => ({
      row,
      who: row.querySelector<Part>('.who')!,
      cmds: [...row.querySelectorAll<Part>('.cmd')],
      ok: row.querySelector<Part>('.ok')!,
    })),
    rec: q('.rec'),
    recNames: qa('.rec .hd b'),
    lines: qa('.rr'),
    legend: q('.legend'),
    chain: qa('.cc'),
    chainLine: q('.chain-line'),
    list: q('.list'),
    listNames: qa('.list .hd b'),
    needs: qa('.nr'),
    costs: qa('.cr').map((row) => ({ row, bar: row.querySelector<Part>('.bar')! })),
    h1: q('.h1'),
    h2: q('.h2'),
    reached: q('.h2 .hdot'),
    denied: q('.denied'),
    edges: qa('.edge'),
    wires: qa('.req'),
    dots: qa('.pk'),
    cross: q('.cross'),
    fork: q('.fork'),
    forkRun: q('.fork .run'),
    drop: q('.fork .drop'),
    forkRow: q('.fork .ag'),
    only: q('.only'),
    files: qa('.fn').map((row) => ({ row, button: row.querySelector<Part>('.bb')!, ok: row.querySelector<Part>('.ok') })),
    bin: q('.bin'),
    wire: q('.back-wire'),
    file: q('.file'),
    pointer: q('.ptr'),
  };

  let G = wide();
  let R: Record<string, Shot[]> = {};
  let rows: Shot[][] = [];
  const measure = () => {
    G = getComputedStyle(stage).getPropertyValue('--mode').trim() === 'tall' ? tall() : wide();
    R = {};
    for (const k of ['mac', 'main', 'rec', 'list', 'tl', 'h1', 'h2', 'fork', 'only', 'bin'] as const) R[k] = resolve(G[k]);
    rows = G.rows.map(resolve);
  };

  /** Where your pointer goes: moments, each with where it is and how visible. */
  function pointerPath(): [number, number, number, number][] {
    const list = R.list[4];
    const need = { x: list.x + G.needRow.x, y: list.y + G.needRow.y + G.needRow.h / 2 };
    const tl = R.tl[6];
    const cp = cpAt(tl, BACK);
    const fork = R.fork[7];
    const drop = { x: fork.x + fork.w - 118, y: fork.y + 21 };
    const only = R.only[7];
    const bring = { x: only.x + only.w - 52, y: only.y + G.onlyRow + 20 };
    const away = { x: need.x + 70, y: need.y + 90 };
    return [
      [T(4, 1200), away.x, away.y, 0],
      [T(4, 1500), away.x, away.y, 1],
      [T(4, 2150), need.x, need.y, 1],
      [T(5, 300), need.x, need.y, 1],
      [T(5, 600), need.x, need.y, 0],
      [T(6, 500), tl.x1 - 40, tl.y + 70, 0],
      [T(6, 750), tl.x1 - 40, tl.y + 70, 1],
      [T(6, 1550), cp.x + 2, cp.y + 3, 1],
      [T(7, 300), cp.x + 2, cp.y + 3, 1],
      [T(7, 900), drop.x, drop.y, 1],
      [T(7, 1500), drop.x, drop.y, 1],
      [T(7, 2250), bring.x, bring.y, 1],
      [T(7, 3000), bring.x, bring.y, 1],
      [T(7, 3300), bring.x + 30, bring.y + 40, 0],
    ];
  }

  /** Draws every part as it is at `t`. */
  function draw(t: number) {
    const s = stepAt(t);
    put(P.scene, { o: ramp(t, 0, 350) });

    // your Mac
    const mac = sample(R.mac, t);
    box(P.mac, mac);
    put(P.macName, { x: mac.x + 16, y: mac.y - 9, o: 1 });

    // main, and its three agents
    // In Drop, main grows downwards to take the file brought back; its corner and its line of time stay.
    const m = { ...sample(R.main, t) };
    if (s === 7) m.h = lerp(m.h, G.holds, ramp(t, T(7, 2300), T(7, 2600)));
    const intro = s === 0 ? ramp(t, 100, 520) : 1;
    box(P.main, { ...m, o: m.o * intro }, { s: lerp(0.95, 1, intro), glow: s === 7 ? bump(t, T(7, 3000), 500) : 0 });
    toggle(P.main, 'chip', m.w < 200);
    P.rows.forEach((r, i) => {
      const at = sample(rows[i], t);
      const a = ramp(t, T(0, 900 + i * 600), T(0, 1250 + i * 600));
      put(r.row, { y: at.y, h: at.h, o: a * at.o, dx: (1 - a) * -40 });
      put(r.who, { y: at.ny });
      // The command: typed in Full access. Codex types a second one in Boundary.
      const second = i === 1 && t >= T(3, 1900);
      r.cmds.forEach((cmd, k) => {
        const text = k ? CURL : COMMANDS[i];
        const from = k ? T(3, 2000) : T(1, TYPED_AT[i]);
        const shown = k ? second : !second;
        let o = shown ? at.co : 0;
        if (k) o *= ramp(t, T(3, 1850), T(3, 2000));
        if (t < from) o = 0;
        const n = Math.floor((t - from) / TYPE_MS);
        put(cmd, { x: at.cx, y: at.cy, o, n: t < from ? 0 : clamp(n, 0, text.length), caret: t >= from && n < text.length + 4 ? 1 : 0 });
      });
      const done = T(1, TYPED_AT[i]) + COMMANDS[i].length * TYPE_MS;
      put(r.ok, { y: (G.tall ? at.ny : at.cy) + 1, o: second ? 0 : at.co, d: ramp(t, done + 150, done + 420) });
    });

    // the record
    const rec = sample(R.rec, t);
    box(P.rec, rec);
    const chained = ramp(t, T(4, 2300), T(4, 2600));
    put(P.recNames[0], { o: 1 - chained });
    put(P.recNames[1], { o: chained });
    P.lines.forEach((part, k) => {
      const a = ramp(t, RECORD_AT[k], RECORD_AT[k] + 300);
      put(part, { a, o: a * (1 - chained), wash: k < 5 ? (1 - lin(t, RECORD_AT[k], RECORD_AT[k] + 1300)) * 0.9 : 0 });
    });
    put(P.legend, { o: ramp(t, T(2, 3000), T(2, 3300)) * (1 - ramp(t, T(3, 0), T(3, 300))) });
    P.chain.forEach((part, j) => {
      const a = ramp(t, T(4, 2700 + j * 330), T(4, 2980 + j * 330));
      put(part, { a, o: a * chained });
    });
    put(P.chainLine, { a: lin(t, T(4, 2850), T(4, 3900)), o: chained });

    // what needs you, then what costs the most: one list that changes its rows
    const list = sample(R.list, t);
    box(P.list, list);
    const needs = 1 - ramp(t, T(5, 0), T(5, 250));
    const cost = ramp(t, T(5, 350), T(5, 600));
    put(P.listNames[0], { o: needs });
    put(P.listNames[1], { o: cost });
    P.needs.forEach((part, i) => {
      const a = ramp(t, T(4, 350 + i * 300), T(4, 650 + i * 300));
      put(part, {
        dy: (1 - a) * 8,
        o: a * needs,
        wash: i === 0 ? ramp(t, T(4, 2200), T(4, 2350)) * needs : 0,
        s: i === 0 ? 1 - 0.012 * bump(t, T(4, 2200)) : 1,
      });
    });
    P.costs.forEach((c, i) => {
      const a = ramp(t, T(5, 450 + i * 300), T(5, 750 + i * 300));
      put(c.row, { dy: (1 - a) * 8, o: a * cost });
      const grow = ramp(t, T(5, 700 + i * 150), T(5, 1700 + i * 150));
      put(c.bar, { w: Math.max(3, (G.bar * COSTS[i]) / COSTS[0]) * grow });
    });

    // main's line of time and its checkpoints
    const tl = sample(R.tl, t);
    put(P.tl, { o: tl.o });
    put(P.tlLine, { x: tl.x0, y: tl.y, w: tl.x1 - tl.x0 });
    const picked = ramp(t, T(6, 1600), T(6, 1800));
    CHECKPOINTS.forEach((at, k) => {
      const a = ramp(t, at, at + 300);
      const p = cpAt(tl, k);
      let size = tl.size * (0.5 + 0.5 * a);
      let hot = 0;
      if (k === BACK && s >= 6) {
        hot = picked;
        size *= 1 + 0.35 * picked + 0.25 * bump(t, T(6, 1600));
      }
      put(P.cps[k], { x: p.x, y: p.y, size, o: a, hot });
    });
    const back = cpAt(tl, BACK);
    put(P.cpName, { x: back.x + 14, y: back.y - 24, o: ramp(t, T(6, 350), T(6, 650)) * (1 - ramp(t, T(7, 0), T(7, 300))) });

    // outside your Mac: crates.io and paste.rs
    const h1 = sample(R.h1, t);
    const h2 = sample(R.h2, t);
    box(P.h1, h1);
    box(P.h2, h2);
    put(P.reached, { got: ramp(t, T(3, 1500), T(3, 1800)) });
    // Denied: by paste.rs, inside its box across, over it in one column, on the side away from crates.io.
    const by = G.tall ? { x: h1.x + h1.w, y: h1.y - 10 } : { x: h1.x + h1.w - 10, y: h1.y + h1.h / 2 };
    put(P.denied, { ...by, o: h1.o * ramp(t, T(3, 3600), T(3, 3900)) });

    // The edge of main is lit through Boundary. A request leaves the row of the agent that sent it: the one for
    // crates.io crosses the edge, which opens for it, and arrives; the one for paste.rs stops at the edge.
    const lit = s === 3 ? ramp(t, T(3, 0), T(3, 400)) : s === 4 ? 1 - ramp(t, T(4, 0), T(4, 300)) : 0;
    const edge = m.x + m.w;
    const yPass = h2.y + h2.h / 2;
    const yStop = h1.y + h1.h / 2;
    const open = 17 * (lin(t, T(3, 950), T(3, 1050)) - lin(t, T(3, 1250), T(3, 1350)));
    put(P.edges[0], { x: edge, y: m.y, h: yPass - open - m.y, o: lit });
    put(P.edges[1], { x: edge, y: yPass + open, h: m.y + m.h - yPass - open, o: lit });
    const from = edge - G.reqFrom;
    const pass = lerp(from, h2.x - 6, smooth(lin(t, T(3, 700), T(3, 1500))));
    const stop = lerp(from, edge - 7, easeOut(lin(t, T(3, 2900), T(3, 3500))));
    put(P.wires[0], { x: from, y: yStop, w: stop - from, o: lit * lin(t, T(3, 2900), T(3, 2980)) });
    put(P.wires[1], { x: from, y: yPass, w: pass - from, o: lit * lin(t, T(3, 700), T(3, 780)) });
    const stopped = lin(t, T(3, 3500), T(3, 3800));
    put(P.dots[0], { x: stop, y: yStop, o: lin(t, T(3, 2900), T(3, 2980)) * lit * (1 - stopped) });
    put(P.dots[1], { x: pass, y: yPass, o: lin(t, T(3, 700), T(3, 780)) * (1 - lin(t, T(3, 1500), T(3, 1600))) });
    put(P.cross, { x: edge, y: yStop, o: stopped * lit });

    // the fork: it starts from the checkpoint you pressed, shows what is only there, and goes to the bin
    const start = s < 6 ? 0 : ramp(t, T(6, 1900), T(6, 2300));
    let f = sample(R.fork, t);
    let fo = f.o * start;
    let scale = lerp(0.94, 1, start);
    const binned = ramp(t, T(7, 3300), T(7, 3900));
    if (binned > 0) {
      const b = G.binned;
      f = { x: lerp(f.x, b.x, binned), y: lerp(f.y, b.y, binned), w: lerp(f.w, b.w, binned), h: lerp(f.h, b.h, binned) };
      fo = 1;
      scale = 1;
    }
    box(P.fork, { ...f, o: fo }, { s: scale });
    toggle(P.fork, 'chip', f.h < 80);
    toggle(P.fork, 'gone', binned >= 1);
    put(P.forkRow, { o: s >= 6 ? (s === 6 ? ramp(t, T(6, 2500), T(6, 2800)) : 1 - binned) : 0 });
    put(P.drop, {
      o: ramp(t, T(7, 200), T(7, 450)) * (1 - ramp(t, T(7, 1450), T(7, 1700))),
      s: 1 - 0.07 * bump(t, T(7, 1000)),
    });
    put(P.forkRun, { o: 1 - binned });
    const only = sample(R.only, t);
    box(P.only, { ...only, o: only.o * ramp(t, T(7, 1100), T(7, 1400)) });
    P.files.forEach((file, i) => {
      const a = ramp(t, T(7, 1300 + i * 220), T(7, 1600 + i * 220));
      put(file.row, { dy: (1 - a) * 6, o: a });
      const brought = i === 0 ? ramp(t, T(7, 2350), T(7, 2600)) : 0;
      put(file.button, { o: 1 - brought, s: 1 - 0.07 * (i === 0 ? bump(t, T(7, 2300)) : 0) });
      if (file.ok) put(file.ok, { d: brought, o: brought });
    });
    // The file brought back goes to main and stays there.
    const fly = smooth(lin(t, T(7, 2450), T(7, 3050)));
    const fromOnly = R.only[7];
    const toMain = R.main[7];
    put(P.file, {
      x: lerp(fromOnly.x + 12, toMain.x + G.got.x, fly),
      y: lerp(fromOnly.y + G.onlyRow + 6, toMain.y + G.got.y, fly),
      o: s === 7 ? lin(t, T(7, 2450), T(7, 2550)) : 0,
    });
    const bin = sample(R.bin, t);
    box(P.bin, { ...bin, o: bin.o * ramp(t, T(7, 2900), T(7, 3250)) });

    // the wire from the checkpoint you pressed to the fork
    const grow = (s === 7 ? 1 - ramp(t, T(7, 0), T(7, 250)) : 1) * ramp(t, T(6, 1700), T(6, 2200));
    const below = f.y > back.y;
    const y1 = back.y + (below ? 8 : -8);
    const y2 = below ? f.y : f.y + f.h;
    const reach = lerp(y1, y2, grow);
    put(P.wire, { x: back.x, y: Math.min(y1, reach), h: Math.abs(reach - y1), o: s >= 6 && grow > 0 ? 1 : 0, hot: s === 6 ? 1 : 0 });

    // your pointer
    const path = pointerPath();
    let px = path[0][1];
    let py = path[0][2];
    let po = 0;
    for (let i = 0; i < path.length - 1; i++) {
      const a = path[i];
      const b = path[i + 1];
      if (t >= a[0] && t <= b[0]) {
        const p = smooth((t - a[0]) / (b[0] - a[0]));
        px = lerp(a[1], b[1], p);
        py = lerp(a[2], b[2], p);
        po = lerp(a[3], b[3], p);
      }
    }
    const press = Math.max(bump(t, T(4, 2200)), bump(t, T(6, 1600)), bump(t, T(7, 1000)), bump(t, T(7, 2300)));
    put(P.pointer, { x: px, y: py, o: po, s: 1 - 0.12 * press });
  }

  let loop = 0;
  let startedAt = 0;
  const stop = () => {
    if (loop) cancelAnimationFrame(loop);
    loop = 0;
  };
  const frame = () => {
    const t = performance.now() - startedAt;
    draw(Math.min(t, END - 1));
    loop = t < END - 1 ? requestAnimationFrame(frame) : 0;
  };
  return {
    end: END,
    hold(t = END - 1) {
      stop();
      measure();
      draw(clamp(t, 0, END - 1));
    },
    play(from = 0) {
      stop();
      measure();
      draw(from);
      startedAt = performance.now() - from;
      loop = requestAnimationFrame(frame);
    },
  };
}

export interface Flow {
  /** True while the flow is moving. */
  readonly running: boolean;
  /** Says whether the flow may move on its own. When it may not, it holds at its moment and carries on from there. */
  allow(may: boolean): void;
  /** Draws the flow again at the same moment, after something changed the size of its stage. */
  redraw(): void;
}

/**
 * Starts the flow in `root`. It moves while the visitor wants it to (Pause and Play; with `still`, not until they
 * press Play) and `allow` says it may. `onChange` is called whenever it starts or stops moving.
 */
export function mountFlow(root: HTMLElement, options: { still: boolean; onChange?: () => void }): Flow {
  const stage = root.querySelector<HTMLElement>('.stage')!;
  const steps = [...root.querySelectorAll<HTMLElement>('.steps li')];
  const ctl = root.querySelector<HTMLButtonElement>('.ctl')!;
  const picture = film(stage);
  let timers: number[] = [];
  /** The visitor wants the flow to move: true until they press Pause. */
  let wanted = !options.still;
  /** The page says the flow may move. */
  let allowed = false;
  let running = false;
  let step = 0;
  /** The moment shown while the flow is held, and what `performance.now()` was at moment 0 while it moves. */
  let at = 0;
  let startedAt = 0;
  const later = (run: () => void, ms: number) => timers.push(window.setTimeout(run, ms));
  const now = () => (running ? Math.min(performance.now() - startedAt, END - 1) : at);

  function setStep(i: number, t: number) {
    step = i;
    steps.forEach((li, k) => {
      li.classList.toggle('done', k < i);
      li.classList.remove('on');
      li.querySelector('button')?.removeAttribute('aria-current');
    });
    // The bar of a step is a CSS animation: laying the page out here starts it again from `t`.
    void root.offsetWidth;
    steps[i].style.setProperty('--dur', `${endOf(i) + 1 - STEPS[i]}ms`);
    steps[i].style.setProperty('--skip', `${STEPS[i] - t}ms`);
    steps[i].classList.add('on');
    steps[i].querySelector('button')?.setAttribute('aria-current', 'step');
  }

  /** Shows the flow as it is at `t`. It plays on from there if the visitor wants it to and the page allows it. */
  function show(t: number) {
    timers.forEach(clearTimeout);
    timers = [];
    const was = running;
    running = wanted && allowed;
    at = t;
    startedAt = performance.now() - t;
    if (running) picture.play(t);
    else picture.hold(t);
    stage.classList.remove('fade');
    setStep(stepAt(t), t);
    // Paused by the visitor, the step's bar is full. Held by the page, it stops where it is.
    root.classList.toggle('paused', !wanted);
    root.classList.toggle('held', wanted && !allowed);
    ctl.textContent = wanted ? 'Pause' : 'Play';
    if (running) {
      STEPS.forEach((start, i) => {
        if (start > t) later(() => setStep(i, start), start - t);
      });
      later(() => stage.classList.add('fade'), END - 450 - t);
      later(() => show(0), END - t);
    }
    if (was !== running) options.onChange?.();
  }

  // Pause shows the end of the step that was playing. Play goes on with the next step.
  ctl.addEventListener('click', () => {
    wanted = !wanted;
    show(wanted ? (step < STEPS.length - 1 ? STEPS[step + 1] : 0) : endOf(step));
  });
  steps.forEach((li, i) => li.querySelector('button')?.addEventListener('click', () => show(wanted ? STEPS[i] : endOf(i))));

  // A change of size can change the composition (across, or in one column), so the flow is drawn again at the same
  // moment. The stage keeps one size through every step, so playing never causes this.
  let size = `${stage.clientWidth} ${stage.clientHeight}`;
  new ResizeObserver(() => {
    const found = `${stage.clientWidth} ${stage.clientHeight}`;
    if (found === size) return;
    size = found;
    show(now());
  }).observe(stage);

  root.classList.add('live');
  show(options.still ? endOf(STILL_STEP) : 0);

  return {
    get running() {
      return running;
    },
    allow(may) {
      if (may === allowed) return;
      const t = now();
      allowed = may;
      show(t);
    },
    redraw: () => show(now()),
  };
}
