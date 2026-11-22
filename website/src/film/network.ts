// The network as the film draws it, in Network and again in An extension, and the request the film follows.

import type { Reel } from './reel';
import { FADE, at, out } from './story';
import { BOUNDARY, C, E, G, MAIN_W, T, box, textWidth, translate } from './svg';
import { along, easeOut, part, type Leg, type Point } from './time';

const GW = 150;
export const NET = {
  W: [104, 200, 296],
  gx: BOUNDARY,
  src: C.l + MAIN_W / 2,
  host: 764,
  gw: GW,
  // Where a request, or what comes back, stands when it waits at a gate.
  stop: BOUNDARY - GW / 2 - 6,
  stopBack: BOUNDARY + GW / 2 + 17,
};
export function gateBox(parent: Node, x: number, y: number, open = false) {
  return E('rect', { x: x - NET.gw / 2, y: y - 17, width: NET.gw, height: 34, rx: 3, class: open ? 'f-gate f-open' : 'f-gate' }, parent);
}
// A verdict under its gate, on the boundary.
export function verdict(f: Reel, parent: Node, x: number, y: number, word: string, a: number, cls = 'f-tok16') {
  const s = f.fade(G(parent), a, null);
  const t = T(s, x, y + 44, word, cls, 'middle');
  const w = textWidth(t) + 12;
  s.insertBefore(E('rect', { x: x - w / 2, y: y + 27, width: w, height: 24, class: 'f-knock' }), t);
  return t;
}
export function placeholder(parent: Node, x: number, y: number) {
  E('rect', { x: x - 62, y, width: 124, height: 28, rx: 3, class: 'f-card f-placeholder' }, parent);
  T(parent, x, y + 19, 'STRIPE_KEY', 'f-tok f-muted', 'middle');
}
export function network(f: Reel, parent: Node, o: { secret?: boolean; label?: number; named?: readonly number[] } = {}) {
  const g = G(parent);
  E('line', { x1: NET.gx, y1: C.t, x2: NET.gx, y2: C.b, class: 'f-line' }, g);
  box(g, C.l, C.t, MAIN_W, C.h, 'main');
  NET.W.forEach((y) => {
    E('line', { x1: NET.src, y1: y, x2: NET.host, y2: y, class: 'f-wire' }, g);
    E('circle', { cx: NET.src, cy: y, r: 3.5, class: 'f-wiref' }, g);
  });
  // Inside main the secret is only a placeholder.
  if (o.secret) placeholder(g, NET.src, 232);
  // What travels goes under the gates: in at one side, out at the other.
  const under = G(g);
  const hosts = ['api.stripe.com', 'registry.npmjs.org', 'paste.rs'];
  const rules = ['stripe v2', 'registries v3', 'no rule'];
  NET.W.forEach((y, i) => {
    gateBox(g, NET.gx, y, i === 2);
    E('circle', { cx: NET.host, cy: y, r: 6, class: 'f-box' }, g);
    T(g, NET.host + 18, y + 5.5, hosts[i], 'f-tok16');
    const r = T(g, NET.gx, y + 5.5, rules[i], i === 2 ? 'f-tok16 f-muted' : 'f-tok16', 'middle');
    // A gate says its rule once a request has reached it; until then a bar, gone before the words come.
    if (o.named) {
      const w = [70, 100, 60][i];
      f.fade(E('rect', { x: NET.gx - w / 2, y: y - 2.5, width: w, height: 5, rx: 2.5, class: 'f-bar' }, g), null, o.named[i], { dy: 0 });
      f.fade(r, o.named[i] + FADE, null, { dy: 0 });
    }
  });
  if (o.label) f.mute(T(g, NET.gx + 12, C.t + 10, 'boundary', 'f-lbl'), o.label);
  return { g, under };
}

// Where the request the story follows waits, as an alert and as the start of a chain.
const ALERT: Point = [60, 62];
export const CHAIN: Point = [440, 370];

// The request the story follows: a dot at the boundary, the first card of Needs you, the first node of the chain.
// It sets off as the canvas it stands on starts to leave.
export function paste(f: Reel) {
  const N = at('network'), from: Point = [NET.src, NET.W[2]];
  const g = f.fade(G(f.canvas), N(5100), out('timeline'), { dy: 0 });
  const inner = G(g), dot = G(inner), dmd = G(inner);
  E('circle', { r: 6, class: 'f-acc-f' }, dot);
  E('path', { d: 'M0 -8L8 0L0 8L-8 0Z', class: 'f-acc-f' }, dmd);
  const moves: Leg[] = [
    { t: N(5340), d: 620, to: [NET.stop, NET.W[2]] },
    { t: out('network'), d: FADE + 400, to: ALERT, c: [150, 250] },
    { t: out('needs'), d: FADE + 400, to: CHAIN, c: [180, 300] },
  ];
  const toNeeds = at('needs')(400);
  for (const leg of moves) f.during(leg.t, leg.d);
  f.cue((t) => {
    const p = along(t, from, moves);
    translate(inner, p[0], p[1]);
    const u = easeOut(part(t, toNeeds, FADE));
    dot.style.opacity = String(1 - u); dmd.style.opacity = String(u);
  });
  return g;
}
