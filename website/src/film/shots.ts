// The film's eleven shots. Each is built once, in the canvas's units, and its moments count from its start.

import { CHAIN, NET, gateBox, network, paste, placeholder, verdict } from './network';
import type { Reel } from './reel';
import { END, FADE, PRESSED, at, out } from './story';
import { BOUNDARY, C, E, G, MAIN_W, T, box, button, card, diamond, show, textWidth, translate } from './svg';
import { along, clamp, easeInOut, lerp, part, pressAfter, type Leg, type Point } from './time';

function shotFirst(f: Reel) {
  const k = 'first', A = at(k);
  const g = f.scene(k);
  box(g, C.l, C.t, 330, C.h, 'your Mac', { ghost: true });
  const found = ['Claude Code', 'Codex', '3 repositories'];
  const fy = [72, 120, 168];
  const orig = found.map((name, i) => {
    const c = card(g, 52, fy[i], { tok: name });
    f.fade(c.g, A(100 + i * 100));
    return c;
  });
  const mx = C.r - 350;
  f.fade(box(g, mx, C.t, 350, C.h, 'main', { sub: 'macOS' }), A(600));
  f.present.main = [A(600), END];
  f.rec(A(700), 'f', 'main made');
  // The recipe: three chips drop into main.
  let rx = mx + 22;
  const chips = ['Xcode', 'Node', 'Rust'].map((name, i) => {
    const ta = A(900 + i * 140);
    const inner = G(f.fade(G(g), ta, null, { dy: 0 }));
    const c = card(inner, rx, 296, { tok: name, h: 32 });
    rx += c.w + 8;
    f.move(inner, [{ t: ta, d: 420, to: [0, 0] }], [0, -90]);
    return c;
  });
  // One after the other: the sentence, the chips going quiet, then the button.
  const mid = (C.l + 330 + mx) / 2;
  f.fade(T(g, mid, 196, 'nothing shared', 'f-lbl', 'middle'), A(1700), A(2660));
  f.fade(E('path', { d: `M${mid - 104} 190H${mid - 58}M${mid + 58} 190H${mid + 104}`, class: 'f-dot' }, g), A(1700), A(2660), { dy: 0 });
  for (const c of chips) f.recede(c.tok, A(2900));
  const bt = button(g, 52, 296, 'Copy into main');
  const go: Leg = { t: A(3400), d: 480, to: bt.tip };
  const hit = pressAfter(go), done = hit + PRESSED;
  f.fade(bt.g, A(3160), done);
  f.press(bt, hit);
  f.pointer(g, [go], [280, 270], A(3160), done);
  // The copies travel across, one by one; the originals stay on the Mac, quiet.
  let first!: ReturnType<typeof card>;
  orig.forEach((c, i) => {
    const ta = done + i * 440;
    f.fade(c.tok, null, ta, { dy: 0, dout: 1 });
    const b = c.tok.getBBox();
    const bar = E('rect', { x: b.x, y: b.y + b.height / 2 - 2.5, width: b.width, height: 5, rx: 2.5, class: 'f-bar' }, c.g);
    f.fade(bar, ta + FADE, null, { dy: 0 });
    const inner = G(f.fade(G(g), ta - 1, null, { dy: 0, d: 1 }));
    const cc = card(inner, 52, fy[i], { tok: found[i] });
    f.move(inner, [{ t: ta, d: 420, to: [mx + 22 - 52, 0], c: [(mx - 30) / 2, -24] }], [0, 0], i === 0 ? 'the first copy' : undefined);
    if (i === 0) first = cc;
  });
  f.rec(A(5750), 'f', '3 copied');
  // Sign-in: Claude Code in main opens the Mac's browser, and the answer comes back.
  const br = f.fade(G(g), A(5900));
  E('rect', { x: 222, y: 172, width: 116, height: 82, rx: 3, class: 'f-box', style: 'stroke-width:1.25' }, br);
  E('line', { x1: 222, y1: 188, x2: 338, y2: 188, class: 'f-hair' }, br);
  E('rect', { x: 236, y: 202, width: 70, height: 5, rx: 2.5, class: 'f-bar' }, br);
  E('rect', { x: 236, y: 216, width: 48, height: 5, rx: 2.5, class: 'f-bar' }, br);
  E('rect', { x: 236, y: 230, width: 44, height: 14, rx: 2, class: 'f-filled' }, br);
  const from: Point = [mx + 22, 90], to: Point = [338, 210];
  f.fade(E('path', { d: `M${from[0]} ${from[1]}C500 90 450 200 ${to[0]} ${to[1]}`, class: 'f-dot' }, g), A(6100), null, { dy: 0 });
  const d1 = G(f.fade(G(g), A(6100), A(7120), { dy: 0, dout: 120 }));
  E('circle', { r: 5.5, class: 'f-ink' }, d1);
  f.move(d1, [{ t: A(6200), d: 420, to, c: [470, 140] }, { t: A(6700), d: 420, to: from, c: [470, 140] }], from);
  f.fade(T(g, mx + 22 + first.w + 12, 95, 'signed in', 'f-lbl'), A(7240));
  f.rec(A(7240), 'r', 'signed in');
}

function shotNow(f: Reel) {
  const k = 'now', A = at(k);
  f.litShot(k, 'Now');
  const g = f.scene(k);
  box(g, C.l, C.t, C.w, 294, 'main');
  for (const y of [180.5, 244.5]) E('line', { x1: 48, y1: y, x2: 662, y2: y, class: 'f-hair' }, g);
  const lane = (y: number, name: string | null, cmd: string, ta: number, ms: number) => {
    if (name) f.fade(T(g, 54, y, name, 'f-name'), ta);
    f.fade(T(g, 200, y, '$', 'f-tok16 f-muted'), ta + 100);
    return f.typeText(T(g, 216, y, '', 'f-tok16'), cmd, ta + 200, ms);
  };
  const c1 = lane(100, 'Claude Code', 'npm install', A(100), 30);
  const s1 = f.fade(T(g, 660, 100, '$0.42', 'f-tok16', 'end'), A(680));
  f.rec(A(700), 'f', 'npm install');
  f.draw(E('path', { d: 'M62 112V136Q62 144 70 144H188', class: 'f-line' }, g), A(1100), 340, 'the line to the second command');
  lane(149, null, 'cargo test', A(1300), 30);
  f.rec(A(2100), 'f', 'cargo test');
  // What has been read goes to the muted ink, to keep the picture quiet.
  for (const e of [c1, s1]) f.mute(e, A(1900));
  lane(212, 'Codex', 'sudo npm install -g pnpm', A(2200), 28);
  const s3 = f.fade(T(g, 660, 212, '7% of plan', 'f-tok16', 'end'), A(3160));
  f.rec(A(3460), 'f', 'pnpm installed');
  f.mute(s3, A(3860));
  // A second run of Claude Code, stuck: an opinion, not a fact.
  const r3 = f.fade(G(g), A(3500));
  T(r3, 54, 284, 'Claude Code', 'f-name f-muted');
  card(r3, 196, 261, { mark: 'o', tok: 'stuck' });
  // main's screen is inside main: an agent's pointer presses a button there.
  const sc = f.fade(G(g), A(4100));
  box(sc, 688, 66, 252, 206, 'screen');
  [[120, 134], [160, 152], [96, 170]].forEach(([w, y]) => E('rect', { x: 706, y, width: w, height: 6, rx: 3, class: 'f-bar' }, sc));
  const b = { g: G(sc), x: 832, y: 222, w: 90, h: 30 };
  E('rect', { x: b.x, y: b.y, width: b.w, height: b.h, rx: 3, class: 'f-btn' }, b.g);
  const go: Leg = { t: A(4540), d: 520, to: [b.x + b.w - 8, b.y + b.h - 6], c: [830, 200] };
  const hit = pressAfter(go);
  f.press(b, hit, 'f-filled');
  f.pointer(g, [go], [760, 190], A(4340), null);
}

function shotMain(f: Reel) {
  const k = 'main', A = at(k);
  f.litShot(k, 'main');
  const g = f.scene(k);
  const BX = BOUNDARY, GW = 190, py = (i: number) => C.t + 30 + i * 46, gy = py(0);
  box(g, C.l, C.t, MAIN_W, C.h, 'main');
  [100, 76, 112].forEach((w, i) => E('rect', { x: 48, y: 74 + i * 28, width: w, height: 6, rx: 3, class: 'f-bar' }, g));
  E('line', { x1: BX, y1: C.t, x2: BX, y2: C.b, class: 'f-line' }, g);
  const widen = [A(2940), A(4240)], WIDEN = 380;
  // main reaches 14 hosts through the network port: one fan, narrowing with the gate's right edge as it widens.
  // Every line has the same two bends, so no two cross and each meets only its own dot.
  const hx = C.r - 4, o0 = BX + 7, o1 = BX + GW / 2;
  const fan = G(g);
  for (let i = 0; i < 14; i++) {
    const hy = C.t + 5 + (i * (C.h - 10)) / 13;
    f.draw(E('path', { d: `M${o0} ${gy}C${o0 + 190} ${gy} ${hx - 170} ${hy} ${hx} ${hy}`, class: 'f-thin' }, fan), A(700), 600);
    f.fade(E('circle', { cx: hx, cy: hy, r: 3.5, class: 'f-ink' }, g), A(1120), null, { dy: 0 });
  }
  f.during(widen[0], WIDEN);
  f.cue((t) => {
    const s = (hx - lerp(o0, o1, easeInOut(part(t, widen[0], WIDEN)))) / (hx - o0);
    fan.setAttribute('transform', `translate(${hx} 0) scale(${s.toFixed(4)} 1) translate(${-hx} 0)`);
  });
  // Seven channels, all closed; each keeps its name for the whole shot.
  const names = ['network', 'ports', 'files', 'secrets', 'screen', 'clipboard', 'repositories'];
  names.forEach((n, i) => {
    E('rect', { x: BX - 7, y: py(i) - 11, width: 14, height: 22, rx: 2, class: 'f-filled' }, g);
    T(g, BX - GW / 2 - 10, py(i) + 4.5, n, 'f-lbl', 'end');
  });
  // The network channel opens.
  f.fade(E('rect', { x: BX - 7, y: gy - 11, width: 14, height: 22, rx: 2, class: 'f-box' }, g), A(420), null, { dy: 0 });
  const bt = button(g, 600, 368, 'Limit to these 14');
  const go: Leg = { t: A(1700), d: 500, to: bt.tip };
  const hit = pressAfter(go);
  f.fade(bt.g, A(1460), hit + PRESSED);
  f.press(bt, hit);
  f.pointer(g, [go], [820, 310], A(1460), hit + PRESSED);
  // A gate widens on its port, opaque all the while, and says its rule when it is whole.
  const gate = (i: number, ta: number, parts: string[]) => {
    const y = py(i), h = G(g);
    const r = E('rect', { x: -GW / 2, y: -15, width: GW, height: 30, rx: 3, class: 'f-gate' }, h);
    f.during(ta, WIDEN);
    f.cue((t) => {
      const u = easeInOut(part(t, ta, WIDEN));
      r.style.vectorEffect = u < 1 ? 'non-scaling-stroke' : 'none';
      h.setAttribute('transform', `translate(${BX} ${y}) scale(${lerp(14 / GW, 1, u).toFixed(4)} ${lerp(22 / 30, 1, u).toFixed(4)})`);
      show(h, t >= ta);
    });
    const t = E('text', { x: BX, y: y + 5, class: 'f-tok', 'text-anchor': 'middle' }, g);
    parts.forEach((s, j) => E('tspan', { dx: j ? 8 : null }, t, s));
    f.fade(t, ta + WIDEN + 40, null, { dy: 0 });
  };
  gate(0, widen[0], ['used hosts', 'allow']);
  f.rec(A(3500), 'f', '14 hosts');
  // The canary is a fake secret: its rule sits on the secrets port.
  gate(3, widen[1], ['canary', 'deny']);
  f.fade(T(g, BX + GW / 2 + 12, py(3) + 4.5, 'stops main', 'f-lbl'), A(5120), null, { dy: 0 });
  // The promise, under the boundary.
  f.fade(T(g, BX, C.b + 30, 'unchangeable from inside', 'f-name', 'middle'), A(5560));
}

function shotNetwork(f: Reel) {
  const k = 'network', A = at(k);
  f.litShot(k, 'Network');
  const g = f.scene(k);
  const net = network(f, g, { secret: true, label: A(3300), named: [A(1180), A(4160), A(5960)] });
  const [w1, w2, w3] = NET.W;
  // 1. api.stripe.com, allowed; STRIPE_KEY is a placeholder inside main and the real value past the gate.
  const p0: Point = [NET.src, w1], host = NET.host;
  const m1: Leg[] = [{ t: A(540), d: 640, to: [NET.stop, w1] }, { t: A(1760), d: 800, to: [host, w1] }];
  f.stroke(net.under, p0, [host, w1], 'f-trace', (t) => (along(t, p0, m1)[0] - NET.src) / (host - NET.src));
  const dot = G(f.fade(G(net.under), A(300), null, { dy: 0 }));
  E('circle', { r: 6, class: 'f-ink' }, dot);
  f.move(dot, m1, p0, 'the request to api.stripe.com');
  const secret = G(f.fade(G(net.g), A(300), A(3060), { dy: 0 }));
  const ph = G(secret), real = G(secret);
  placeholder(ph, 0, 24);
  E('rect', { x: -62, y: 24, width: 124, height: 28, rx: 3, class: 'f-card f-solid' }, real);
  T(real, 0, 43, 'sk_live_••••', 'f-tok', 'middle');
  f.cue((t) => {
    const p = along(t, p0, m1);
    translate(secret, p[0], p[1]);
    // One value gives way to the other: their letters never overlap.
    ph.style.opacity = String(1 - clamp((p[0] - (NET.gx - 60)) / 60));
    real.style.opacity = String(clamp((p[0] - NET.gx) / 60));
  });
  f.fade(E('circle', { cx: NET.host, cy: w1, r: 6, class: 'f-ink' }, net.g), A(2560), null, { dy: 0 });
  f.mute(verdict(f, net.g, NET.gx, w1, 'allowed', A(2700)), A(3700));
  f.rec(A(2700), 'f', 'stripe allowed');
  // 2. The package comes in and is stopped outside.
  const out0 = NET.host - 22, m2: Leg[] = [{ t: A(3580), d: 580, to: [NET.stopBack, w2] }];
  const back: Point = [NET.host - 8, w2];
  f.stroke(net.under, back, [NET.stopBack + 11, w2], 'f-trace', (t) => (t < A(3580) ? 0 : clamp((back[0] - along(t, [out0, w2], m2)[0] - 11) / (back[0] - NET.stopBack - 11))));
  const pk = G(f.fade(G(net.g), A(3300), null, { dy: 0 }));
  E('rect', { x: -11, y: -11, width: 22, height: 22, rx: 2, class: 'f-box' }, pk);
  E('path', { d: 'M0 -11V11M-11 0H11', class: 'f-line', style: 'stroke-width:1.2' }, pk);
  f.move(pk, m2, [out0, w2]);
  f.mute(f.fade(T(net.g, NET.stopBack + 20, w2 - 18, 'colorize-ng 2.0.0 · 2 days', 'f-tok'), A(3300)), A(5100));
  verdict(f, net.g, NET.gx, w2, 'denied', A(4700));
  f.rec(A(4700), 'f', 'colorize-ng denied');
  // 3. paste.rs: no rule allows it. It raises an alert.
  f.stroke(net.under, [NET.src, w3], [NET.stop, w3], 'f-trace f-acc-s', f.eased(A(5340), 620));
  verdict(f, net.g, NET.gx, w3, 'denied', A(6500), 'f-tok16 f-acc-t');
  f.rec(A(6500), 'f', 'paste.rs denied', true);
}

function shotNeeds(f: Reel) {
  const k = 'needs', A = at(k);
  f.litShot(k, 'Needs you');
  const g = f.scene(k);
  const h = 72, y2 = C.t + h + 16, r = C.r - 20;
  // First: the new host, denied. Its card waits for the request that raised it.
  E('rect', { x: C.l, y: C.t, width: C.w, height: h, rx: 3, class: 'f-card f-acc' }, g);
  const c1 = f.fade(G(g), A(400), null, { dy: 0 });
  const nh = T(c1, 84, 67, 'new host', 'f-lbl');
  const p = T(c1, 84 + textWidth(nh) + 10, 67.5, 'paste.rs', 'f-tok16');
  T(c1, Number(p.getAttribute('x')) + textWidth(p) + 10, 67.5, 'denied', 'f-tok16 f-acc-t');
  const open = button(c1, r, C.t + 20, 'Open in Timeline', { right: true });
  // Second: a question.
  const c2 = f.fade(G(g), A(760));
  E('rect', { x: C.l, y: y2, width: C.w, height: h, rx: 3, class: 'f-card' }, c2);
  const base = y2 + 41;
  const q1 = T(c2, 84, base, 'Is', 'f-name');
  const q2 = T(c2, 84 + textWidth(q1) + 8, base + 0.5, '~/api/target', 'f-tok16');
  T(c2, Number(q2.getAttribute('x')) + textWidth(q2) + 8, base, 'build output?', 'f-name');
  const yn = G(c2);
  const no = button(yn, r, y2 + 20, 'No', { right: true });
  const yes = button(yn, no.x - 8, y2 + 20, 'Yes', { right: true });
  const toYes: Leg = { t: A(1300), d: 500, to: yes.tip, c: [820, 280] };
  const said = pressAfter(toYes);
  f.press(yes, said);
  f.fade(yn, null, said + PRESSED, { dy: 0 });
  // The pointer steps off the answer before the answer shows.
  const off: Leg = { t: said + PRESSED, d: 360, to: [760, 270] };
  const op = f.fade(G(g), off.t + off.d, null);
  const word = T(op, r, base, 'your opinion', 'f-lbl', 'end');
  diamond(op, r - textWidth(word) - 16, base - 5, 6, 'o');
  const toOpen: Leg = { t: A(3300), d: 560, to: open.tip, c: [720, 120] };
  const opened = pressAfter(toOpen);
  f.press(open, opened);
  f.pointer(g, [toYes, off, toOpen], [700, 300], A(1100), null);
  // The badge is the alert's: it goes when the alert is answered.
  f.badge = [at('network')(6800), opened];
}

function shotTimeline(f: Reel) {
  const k = 'timeline', A = at(k);
  f.litShot(k, 'Timeline');
  const g = f.scene(k);
  const [cx, bottom] = CHAIN, Y = { p: bottom, r: 300, n: 230, t: 160 };
  // Above it all: main's line of time and its checkpoints, one named "before turn 14".
  const tl = rail(f, g, {});
  const right = (y: number, s: string, cls: string | null, a: number) => f.fade(T(g, cx - 24, y + 5.5, s, cls || 'f-tok16', 'end'), a);
  // The alert opens on what triggered it.
  const pd = right(Y.p, 'denied', 'f-tok16 f-acc-t', A(440));
  const pr = f.fade(T(g, cx - 34 - textWidth(pd), Y.p + 5.5, 'paste.rs', 'f-tok16', 'end'), A(440));
  // The chain up to the turn, bottom up.
  f.stroke(g, [cx, Y.p - 9], [cx, Y.r + 9], 'f-line f-dash', f.eased(A(700), 240));
  f.fade(diamond(g, cx, Y.r, 8, 'r'), A(940), null, { dy: 0 });
  const curl = right(Y.r, 'curl', null, A(940));
  const ax = cx - 24 - textWidth(curl) - 8;
  const arrow = f.fade(E('path', { d: `M${ax - 20} ${Y.r}H${ax}M${ax - 5} ${Y.r - 5}L${ax} ${Y.r}L${ax - 5} ${Y.r + 5}`, class: 'f-line', style: 'stroke-width:1.4' }, g), A(940));
  const node = f.fade(T(g, ax - 28, Y.r + 5.5, 'node postinstall.js', 'f-tok16', 'end'), A(940));
  f.stroke(g, [cx, Y.r - 9], [cx, Y.n + 9], 'f-line', f.eased(A(1240), 240));
  f.fade(diamond(g, cx, Y.n, 8, 'f'), A(1480), null, { dy: 0 });
  const npm = right(Y.n, 'npm install', null, A(1480));
  f.stroke(g, [cx, Y.n - 9], [cx, Y.t + 9], 'f-line', f.eased(A(1780), 240));
  f.fade(diamond(g, cx, Y.t, 8, 'f'), A(2020), null, { dy: 0 });
  const t14 = right(Y.t, 'turn 14', null, A(2020));
  const ccw = f.fade(T(g, cx - 34 - textWidth(t14), Y.t + 5.5, 'Claude Code', 'f-name', 'end'), A(2020));
  f.fade(E('path', { d: `M${cx} ${Y.t - 10}V${RAIL.y + 6}`, class: 'f-dot' }, g), A(2020), null, { dy: 0 });
  // What has been read goes to the muted ink, still named; then one branch, then the other.
  for (const e of [pr, pd, curl, node, tl.label, arrow]) f.mute(e, A(2500));
  // On the same tool call: the files it made, and, hung off them, who wrote them (an opinion).
  f.draw(E('path', { d: `M${cx + 9} ${Y.n}H612`, class: 'f-line' }, g), A(2760), 300, 'the branch to the files');
  f.fade(diamond(g, 620, Y.n, 8, 'f'), A(3060), null, { dy: 0 });
  const files = f.fade(T(g, 640, Y.n + 5.5, '1,204 files', 'f-tok16'), A(3060));
  const wy = Y.n + 46;
  f.draw(E('path', { d: `M620 ${Y.n + 9}V${wy - 8}Q620 ${wy} 628 ${wy}H640`, class: 'f-line' }, g), A(3480), 240);
  f.fade(diamond(g, 648, wy, 8, 'o'), A(3720), null, { dy: 0 });
  const o1 = E('text', { x: 668, y: wy + 5.5, class: 'f-tok16' }, g);
  E('tspan', { class: 'f-sans' }, o1, 'written by');
  E('tspan', { dx: 8 }, o1, 'run 2');
  f.fade(o1, A(3720));
  for (const e of [npm, t14, ccw, files, o1]) f.mute(e, A(4580));
  // A second tool call on the turn's line: git push --force, stopped by a rule; the model gets an error.
  const gx = 620, gl = C.r - NET.gw, gm = gl + NET.gw / 2;
  f.draw(E('path', { d: `M${cx + 9} ${Y.t}H${gx - 8}`, class: 'f-line' }, g), A(4840), 240);
  f.fade(diamond(g, gx, Y.t, 8, 'f'), A(5080), null, { dy: 0 });
  f.fade(T(g, gx - 8, Y.t - 20, 'git push --force', 'f-tok16'), A(5080));
  f.draw(E('path', { d: `M${gx + 9} ${Y.t}H${gl}`, class: 'f-line' }, g), A(5340), 240, 'the line to the gate');
  const gate = f.fade(G(g), A(5580), null, { dy: 0 });
  gateBox(gate, gm, Y.t);
  T(gate, gm, Y.t + 5.5, 'no force push', 'f-tok16', 'middle');
  f.fade(T(g, gm, Y.t + 44, 'denied', 'f-tok16', 'middle'), A(5800));
  f.rec(A(5800), 'f', 'git push denied');
  const back = E('path', { d: `M${gm} ${Y.t - 17}C${gm} 88 470 88 447 ${Y.t - 11}`, class: 'f-line' }, g);
  f.draw(back, A(6000), 380, 'the error on its way back');
  f.fade(E('path', { d: `M443 ${Y.t - 22}L447 ${Y.t - 11}L456 ${Y.t - 18}`, class: 'f-line' }, g), A(6380), null, { dy: 0 });
  f.fade(T(g, (gm + 447) / 2, 92, 'error', 'f-lbl', 'middle'), A(6380));
}

// main's line of time: the same drawing in the Timeline and in the fork.
const RAIL = { y: C.t + 36, x0: 90, x1: C.r - 4.5, cps: [170, 280, 390] };
// a: the line draws itself from a, then come the checkpoints and the name of one. Without a, it is there whole.
function rail(f: Reel, parent: Node, o: { a?: number; tag?: boolean; x1?: number }) {
  const g = G(parent);
  const x1 = o.x1 || RAIL.x1, x0 = o.tag ? RAIL.x0 : C.l;
  const { a } = o, whole = a === undefined;
  f.stroke(g, [x0, RAIL.y], [x1, RAIL.y], 'f-line', whole ? () => 1 : f.eased(a, 500));
  // main's name on its line, where the fork needs telling apart from it.
  if (o.tag) {
    E('rect', { x: C.l, y: RAIL.y - 12, width: 60, height: 24, rx: 3, class: 'f-box' }, g);
    T(g, C.l + 30, RAIL.y + 5, 'main', 'f-name', 'middle');
  }
  const then = whole ? null : a + 500;
  const cps = f.fade(G(g), then, null, { dy: 0 });
  RAIL.cps.forEach((x) => E('circle', { cx: x, cy: RAIL.y, r: 6, class: 'f-box' }, cps));
  if (!o.x1) E('circle', { cx: x1, cy: RAIL.y, r: 4.5, class: 'f-ink' }, cps);
  const label = f.fade(T(g, RAIL.cps[2], RAIL.y - 18, 'before turn 14', 'f-tok16', 'middle'), then, null, { dy: 0 });
  return { g, label };
}

function shotDisk(f: Reel) {
  const k = 'disk', A = at(k);
  f.litShot(k, 'Disk');
  const g = f.scene(k);
  // One bar, four parts in proportion; the two smallest get a few pixels so they can be seen.
  const y = C.t, h = 140, x0 = C.l;
  const parts = [[0, 586, 'f-part'], [588, 138, 'f-wiref'], [728, 6, 'f-few'], [736, 4, 'f-bar']] as const;
  const end = x0 + 740;
  const left = E('rect', { x: x0 + 0.75, y: y + 0.75, width: parts[0][1] - 1.5, height: h - 1.5, rx: 2, class: 'f-box f-dash f-emptied' }, g);
  const grow = G(g);
  f.during(A(0), 700);
  f.cue((t) => grow.setAttribute('transform', `translate(${x0} 0) scale(${Math.max(easeInOut(part(t, A(0), 700)), 0.001).toFixed(4)} 1)`));
  const rects = parts.map(([x, w, c], i) => E('rect', { x, y, width: w, height: h, rx: i < 2 ? 2 : 0, class: c }, grow));
  const purge = [A(2760), 760] as const;
  f.during(...purge);
  f.cue((t) => { const u = easeInOut(part(t, ...purge)); rects[0].style.opacity = String(1 - u); left.style.opacity = String(u); });
  // Each part's name and size on one baseline under the bar, at the part's left edge.
  const label = (x: number, yy: number, name: string, size: string, a: number) => {
    const n = f.fade(T(g, x, yy, name, 'f-lbl'), a);
    return f.fade(T(g, x + textWidth(n) + 8, yy, size, 'f-tok16'), a);
  };
  const s1 = label(x0, y + h + 30, 'rebuild', '41.2 GB', A(700));
  f.cue((t) => { const v = 41.2 * (1 - easeInOut(part(t, ...purge))); s1.textContent = (v < 0.05 ? '0' : v.toFixed(1)) + ' GB'; });
  label(x0 + 588, y + h + 30, 'pull', '9.8 GB', A(700));
  // The two small parts take their labels to the right of the bar, on one grid, each by one straight hairline.
  const sm = f.fade(G(g), A(700));
  const r1 = y + 50, r2 = y + 90;
  const oh = T(sm, 0, r1 + 4.5, 'only here', 'f-lbl'), os = T(sm, 0, r1 + 5.5, '212 MB', 'f-tok16');
  const ua = T(sm, 0, r2 + 4.5, 'unattributed', 'f-lbl'), us = T(sm, 0, r2 + 5.5, '18 MB', 'f-tok16');
  const lx = C.r - 28 - Math.max(textWidth(oh) + textWidth(os), textWidth(ua) + textWidth(us));
  oh.setAttribute('x', String(lx + 20)); os.setAttribute('x', String(lx + 28 + textWidth(oh)));
  ua.setAttribute('x', String(lx + 20)); us.setAttribute('x', String(lx + 28 + textWidth(ua)));
  E('line', { x1: x0 + 734, y1: r1, x2: lx - 6, y2: r1, class: 'f-thin' }, sm);
  E('line', { x1: end + 0.5, y1: r2, x2: lx - 6, y2: r2, class: 'f-thin' }, sm);
  diamond(sm, lx + 6, r1, 6, 'o');
  // Purge, under the part it empties, on that part's label line.
  const bt = button(g, x0 + parts[0][1] - 12, y + h + 9, 'Purge', { right: true });
  const go: Leg = { t: A(1480), d: 500, to: bt.tip };
  const hit = pressAfter(go);
  f.fade(bt.g, A(1240), hit + PRESSED);
  f.press(bt, hit);
  f.pointer(g, [go], [760, 300], A(1240), hit + PRESSED);
  f.rec(A(3520), 'f', 'purged');
  // The one pattern, under the bar, on its left edge.
  const pc = card(g, x0, y + h + 84, { mark: 'f', tok: 'npm install', detail: '41× in 12 runs', cls: 'f-tok16', detCls: 'f-tok', h: 56, w: C.w });
  f.fade(pc.g, A(3840));
}

function shotFork(f: Reel) {
  const k = 'fork', A = at(k);
  f.litShot(k, 'Timeline');
  const g = f.scene(k);
  // The same line of time; main keeps running while the fork works.
  const x1 = 820, cp = RAIL.cps[2];
  const r = rail(f, g, { a: A(0), tag: true, x1 });
  f.mute(r.label, A(2380));
  // On a phone it runs on out of view, right of all the camera frames meanwhile.
  const run = f.eased(A(2400), 1900, 'main running on', [x1, RAIL.x1]);
  f.stroke(g, [x1, RAIL.y], [RAIL.x1, RAIL.y], 'f-line', run);
  const now = G(f.fade(G(g), A(500), null, { dy: 0 }));
  E('circle', { r: 4.5, class: 'f-ink' }, now);
  f.cue((t) => translate(now, lerp(x1, RAIL.x1, run(t)), RAIL.y));
  f.fade(E('circle', { cx: 880, cy: RAIL.y, r: 6, class: 'f-box' }, g), A(3400), null, { dy: 0 });
  const go: Leg = { t: A(940), d: 480, to: [cp + 4, RAIL.y + 4], c: [520, 200] };
  const chosen = pressAfter(go);
  f.pressed(chosen, { x: cp - 6, y: RAIL.y - 6, w: 12, h: 12 });
  f.fade(E('circle', { cx: cp, cy: RAIL.y, r: 6, class: 'f-ink' }, g), chosen, null, { dy: 0, d: 100 });
  f.pointer(g, [go], [620, 300], A(740), chosen + PRESSED);
  // The fork springs from the checkpoint: a computer of its own. Dropped, it stays as a ghost.
  const made = A(2140), dropped = A(6980);
  const F = { x: C.l, y: 140, w: 400, h: 160 };
  const branch = `M${cp} ${RAIL.y + 6}V${F.y}`;
  f.fade(E('path', { d: branch, class: 'f-wire f-ghosted f-dash' }, g), dropped, null, { dy: 0 });
  f.draw(E('path', { d: branch, class: 'f-line' }, f.fade(G(g), null, dropped, { dy: 0 })), chosen + 240, 320);
  const fb = f.fade(G(g), made, null);
  f.fade(E('rect', { x: F.x, y: F.y, width: F.w, height: F.h, rx: 4, class: 'f-box' }, fb), null, dropped, { dy: 0 });
  f.fade(E('rect', { x: F.x, y: F.y, width: F.w, height: F.h, rx: 4, class: 'f-box f-ghost' }, fb), dropped, null, { dy: 0 });
  const lifted = A(6300);
  const fname = T(fb, F.x + 18, F.y + 30, 'fork', 'f-name');
  const nw = textWidth(fname);
  f.fade(fname, null, lifted, { dy: 0, dout: 1 });
  f.fade(T(fb, F.x + 18, F.y + 30, 'fork', 'f-name f-muted'), dropped, null, { dy: 0 });
  const props = T(fb, F.x + 26 + nw, F.y + 30, 'no secrets · repos read-only', 'f-lbl');
  props.style.fill = 'var(--ink)';
  f.mute(props, A(3150));
  f.present.fork = [made - FADE, dropped];
  f.litFor(made, 'fork');
  f.rec(made, 'f', 'fork made');
  // A task runs in it.
  const ly = F.y + 86;
  const p = f.fade(T(g, F.x + 18, ly, '$', 'f-tok f-muted'), A(2380));
  const cmd = f.typeText(T(g, F.x + 37, ly, '', 'f-tok'), 'npm install pkg-stats && npm test', A(2420), 22);
  const ex = f.fade(T(g, F.x + 18, ly + 30, 'exit 1', 'f-tok'), A(3200));
  // Only in the fork: notes.md.
  const ni = G(f.fade(G(g), A(3200), null, { dy: 0 }));
  E('path', { d: 'M-9 -12H4L10 -6V12H-9Z', class: 'f-box', style: 'stroke-width:1.3' }, ni);
  E('path', { d: 'M4 -12V-6H10', class: 'f-line', style: 'stroke-width:1.3' }, ni);
  T(ni, 18, 5, 'notes.md', 'f-tok');
  f.move(ni, [{ t: A(5200), d: 500, to: [770, RAIL.y + 34] }], [F.x + F.w - 106, F.y + 25], 'notes.md');
  // It tried paste.rs too: no rule here either.
  const gx = 590, gy = ly - 5, stop = gx - NET.gw / 2 - 6, edge = F.x + F.w;
  const nt = f.fade(G(g), A(3480), null, { dy: 0 });
  E('line', { x1: gx, y1: F.y, x2: gx, y2: F.y + F.h, class: 'f-line' }, nt);
  E('line', { x1: edge + 1, y1: gy, x2: NET.host, y2: gy, class: 'f-wire' }, nt);
  f.stroke(nt, [edge + 1, gy], [stop, gy], 'f-trace f-acc-s', f.eased(A(3780), 400, 'the request to paste.rs'));
  const d4 = G(f.fade(G(nt), A(3700), null, { dy: 0 }));
  E('circle', { r: 6, class: 'f-acc-f' }, d4);
  f.move(d4, [{ t: A(3780), d: 400, to: [stop, gy] }], [edge + 1, gy], 'the request to paste.rs');
  gateBox(nt, gx, gy, true);
  T(nt, gx, gy + 5.5, 'no rule', 'f-tok16 f-muted', 'middle');
  E('circle', { cx: NET.host, cy: gy, r: 6, class: 'f-box' }, nt);
  const pt = T(nt, NET.host + 18, gy + 5.5, 'paste.rs', 'f-tok16');
  const dn = verdict(f, nt, gx, gy, 'denied', A(4220), 'f-tok16 f-acc-t');
  for (const e of [p, cmd, ex, pt, dn]) f.mute(e, A(4640));
  // Its receipt, made from its record, not by the agent: three chips, each a fact.
  let rx = F.x;
  ['2 commands', '316 files', '1 host denied'].forEach((s, i) => {
    const c = card(g, rx, F.y + F.h + 16, { mark: 'f', tok: s });
    rx += c.w + 8;
    f.fade(c.g, A(4760 + i * 90));
  });
  f.rec(A(5700), 'f', 'notes.md back');
  // You drop the fork: its name lifts off as a box of its own, and fades beside the Bin's word as it gets there.
  const fly: Leg = { t: lifted, d: 680, to: [-58, 426], c: [-30, 300] };
  const tag = f.fade(G(g), lifted, fly.t + fly.d + 460, { dy: 0, d: 1 });
  const ti = G(tag);
  E('rect', { x: -nw / 2 - 7, y: -12, width: nw + 14, height: 24, rx: 3, class: 'f-box' }, ti);
  T(ti, 0, 5, 'fork', 'f-name', 'middle');
  f.move(ti, [fly], [F.x + 18 + nw / 2, F.y + 25]);
  f.bin = dropped + FADE;
  f.litFor(f.bin, 'main');
  f.rec(f.bin, 'f', 'dropped');
}

function shotTests(f: Reel) {
  const k = 'tests', A = at(k);
  f.litShot(k, 'main');
  const g = f.scene(k);
  const mid = (C.t + C.b) / 2, w = 430, h = 140, x = C.r - w, low = C.b - h;
  box(g, C.l, C.t, 236, C.h, 'main');
  const c = card(g, 50, mid - 18, { tok: '~/api', detail: 'uncommitted', cls: 'f-tok16' });
  f.fade(c.g, A(200));
  const sx = 50 + c.w;
  f.draw(E('path', { d: `M${sx} ${mid}H360`, class: 'f-line' }, g), A(560), 340);
  f.fade(T(g, 310, mid - 10, 'commit', 'f-lbl', 'middle'), A(700));
  f.fade(E('circle', { cx: 360, cy: mid, r: 5, class: 'f-ink' }, g), A(900), null, { dy: 0 });
  f.rec(A(900), 'f', 'commit sent');
  for (const y of [C.t + h / 2, low + h / 2]) f.draw(E('path', { d: `M360 ${mid}C440 ${mid} 445 ${y} ${x} ${y}`, class: 'f-line' }, g), A(1000), 420, 'the commit');
  f.fade(box(g, x, C.t, w, h, 'macOS'), A(1420));
  f.fade(box(g, x, low, w, h, 'Linux'), A(1420));
  f.present.macOS = [A(1420) - FADE, out(k)];
  f.present.Linux = [A(1420) - FADE, out(k)];
  f.litFor(A(1420), 'macOS');
  [[A(1780), C.t], [A(2280), low]].forEach(([ta, y]) => {
    f.fade(T(g, x + 24, y + 72, '$', 'f-tok16 f-muted'), ta);
    f.typeText(T(g, x + 40, y + 72, '', 'f-tok16'), 'cargo test', ta + 60, 34);
  });
  const ok = f.fade(G(g), A(2900));
  E('path', { d: `M${x + 25} ${C.t + 106}l5.5 5.5 10-11`, class: 'f-line', style: 'stroke-width:2' }, ok);
  T(ok, x + 52, C.t + 112, '412 passed', 'f-tok16');
  const ko = f.fade(G(g), A(3400));
  E('path', { d: `M${x + 26} ${low + 101}l11 11m0 -11l-11 11`, class: 'f-line', style: 'stroke-width:2' }, ko);
  T(ko, x + 52, low + 112, '2 failed', 'f-tok16');
}

function shotExt(f: Reel) {
  const k = 'ext', A = at(k);
  f.litShot(k, 'main');
  const g = f.scene(k);
  // The import card: what it adds, before anything changes.
  const ic = G(g);
  const x = C.l + 26;
  E('rect', { x: C.l, y: C.t, width: C.w, height: 260, rx: 4, class: 'f-card f-solid' }, ic);
  T(ic, x, 66, 'request check', 'f-tok16');
  E('line', { x1: C.l, y1: 86.5, x2: C.r, y2: 86.5, class: 'f-hair' }, ic);
  E('rect', { x: x + 2, y: 116, width: 16, height: 14, rx: 2, class: 'f-box', style: 'stroke-width:1.3' }, ic);
  T(ic, x + 34, 128, 'own computer', 'f-name');
  E('line', { x1: x + 10, y1: 154, x2: x + 10, y2: 178, class: 'f-line' }, ic);
  E('circle', { cx: x + 3, cy: 166, r: 4, class: 'f-ink' }, ic);
  T(ic, x + 34, 171, 'wakes on waiting requests', 'f-name');
  E('path', { d: `M${x + 3} 206l11 11m0 -11l-11 11`, class: 'f-line', style: 'stroke-width:1.6' }, ic);
  T(ic, x + 34, 216, 'deny or hold · never allow', 'f-name');
  const inst = button(ic, C.r - 20, 234, 'Install', { right: true });
  const go: Leg = { t: A(1000), d: 520, to: inst.tip, c: [820, 340] };
  const hit = pressAfter(go), gone = hit + PRESSED;
  f.press(inst, hit);
  f.fade(ic, null, gone, { dy: 0 });
  f.pointer(g, [go], [760, 340], A(760), gone);
  // The network returns, with one more computer under the boundary, wired to the first gate.
  const back0 = gone + FADE;
  f.litFor(back0, 'Network');
  const body = f.fade(G(g), back0, null, { dy: 0 });
  const net = network(f, body, {});
  f.present['request check'] = [back0 - FADE, END];
  f.rec(back0, 'f', 'installed');
  const rc = f.fade(G(body), back0 + 200, null, { dy: 0 });
  const bx = { r: NET.gx + 90, y: 336, h: 46 }, wx = 576, bm = bx.y + bx.h / 2;
  E('rect', { x: NET.gx - 90, y: bx.y, width: 180, height: bx.h, rx: 4, class: 'f-box' }, rc);
  T(rc, NET.gx, bm + 5, 'request check', 'f-name', 'middle');
  const [w1] = NET.W, gr = NET.gx + NET.gw / 2, wy = w1 + 8;
  const wire: Point[] = [[gr, wy], [wx, wy], [wx, bm], [bx.r, bm]];
  E('path', { d: `M${gr} ${wy}H${wx - 12}Q${wx} ${wy} ${wx} ${wy + 12}V${bm - 12}Q${wx} ${bm} ${wx - 12} ${bm}H${bx.r}`, class: 'f-thin' }, rc);
  // A request of main waits at the boundary.
  f.stroke(net.under, [NET.src, w1], [NET.stop, w1], 'f-trace', f.eased(A(2900), 520, 'the request'));
  const d = G(f.fade(G(net.under), A(2780), null, { dy: 0 }));
  E('circle', { r: 6, class: 'f-ink' }, d);
  f.move(d, [{ t: A(2900), d: 520, to: [NET.stop, w1] }], [NET.src, w1], 'the request');
  // The gate asks: a dot leaves along the wire to request check.
  const legs = (pts: readonly Point[], t0: number, total: number) => {
    const len = pts.slice(1).map((p, i) => Math.hypot(p[0] - pts[i][0], p[1] - pts[i][1]));
    const sum = len.reduce((a, b) => a + b, 0);
    let tt = t0;
    return pts.slice(1).map((p, i) => { const leg: Leg = { t: tt, d: total * len[i] / sum, to: p }; tt += leg.d; return leg; });
  };
  const ask = G(f.fade(G(body), A(3460), A(4100), { dy: 0, d: 100, dout: 100 }));
  E('circle', { r: 5, class: 'f-ink' }, ask);
  f.move(ask, legs(wire, A(3560), 540), wire[0]);
  // It answers deny, an opinion: the cause, which comes back along the same wire, clear of the box it left.
  const ai = G(f.fade(G(body), A(4200), null, { dy: 0 }));
  const centred = G(ai);
  const c = card(centred, 0, 0, { mark: 'o', tok: 'deny', h: 30 });
  translate(centred, -c.w / 2, -15);
  f.move(ai, [{ t: A(4460), d: 560, to: [wx + 4, w1 + 68] }], [wx + 4, bm]);
  // The verdict is request check's, not the rule's: this rule allowed the same request in Network.
  verdict(f, body, NET.gx, w1, 'denied by request check', A(5100), 'f-tok');
  f.rec(A(5100), 'f', 'denied by request check');
}

function shotText(f: Reel) {
  const k = 'text', A = at(k);
  f.litShot(k, 'main');
  const g = f.scene(k);
  // An agent inside main reads the record as text: the terminal is its own.
  E('rect', { x: C.l, y: C.t, width: C.w, height: C.h, rx: 4, class: 'f-card f-solid' }, g);
  for (let i = 0; i < 3; i++) E('circle', { cx: C.l + 20 + i * 13, cy: C.t + 19, r: 3.5, class: 'f-hair' }, g);
  E('line', { x1: C.l, y1: C.t + 38.5, x2: C.r, y2: C.t + 38.5, class: 'f-hair' }, g);
  // On a phone it stands in the middle of what the camera frames, which is not the middle of the canvas.
  T(g, f.phone ? 240 : (C.l + C.r) / 2, C.t + 24, 'Claude Code · main', 'f-chrome', 'middle');
  const x = C.l + 32;
  f.fade(T(g, x, 112, '$', 'f-tok16 f-muted'), A(100));
  f.typeText(T(g, x + 18, 112, '', 'f-tok16'), 'silo …', A(300), 40);
  const lines = [['f', 'fact', 'npm install'], ['r', 'report', 'curl'], ['o', 'opinion', 'written by run 2'], ['f', 'fact', 'paste.rs · denied']] as const;
  lines.forEach(([m, kind, rest], i) => {
    const y = 172 + i * 38;
    const ln = f.fade(G(g), A(720 + i * 340));
    diamond(ln, x + 6, y - 5, 5.5, m, i === 3 ? 'f-acc-f' : null);
    T(ln, x + 26, y, kind, 'f-tok16 f-muted');
    T(ln, x + 134, y, rest, i === 3 ? 'f-tok16 f-acc-t' : 'f-tok16');
  });
}

/** Draws every shot on the reel's canvas, with the request the story follows over all of them. */
export function drawShots(f: Reel) {
  shotFirst(f);
  shotNow(f);
  shotMain(f);
  shotNetwork(f);
  const request = paste(f);
  shotNeeds(f);
  shotTimeline(f);
  shotDisk(f);
  shotFork(f);
  shotTests(f);
  shotExt(f);
  shotText(f);
  f.canvas.appendChild(request);
  f.finish();
}
