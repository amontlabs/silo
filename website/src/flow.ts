// The main picture of the home page: one task flow, from two tasks given to main to a teammate's computer
// forked onto your Mac. CSS lays out the boxes; this draws the frames and wires between them, plays the
// flow in a loop, and answers Pause, Play and the step names.

import { add, follow, rem, scene, type Cue, type Run } from './scene';

/** When each step starts, and when the whole flow ends, in milliseconds. */
export const STEPS = [0, 2400, 4900, 7500, 11400, 13800, 17600];
export const END = 21900;
/** The last moment of a step. */
export const endOf = (step: number) => (step < STEPS.length - 1 ? STEPS[step + 1] : END) - 1;
/** The step shown when nothing may move on its own: both forks with what they changed, before you decide. */
export const STILL_STEP = 4;
const stepAt = (t: number) => STEPS.findLastIndex((start) => start <= t);

const RADIUS = 12;

interface Box {
  e: HTMLElement;
  l: number;
  t: number;
  r: number;
  b: number;
  cy: number;
}

export interface Flow {
  /** True while the flow is moving. */
  readonly running: boolean;
  /** Says whether the flow may move on its own. When it may not, it holds at its moment and carries on from there. */
  allow(may: boolean): void;
  /** Draws the flow again at the same moment, after something changed the size of its boxes. */
  redraw(): void;
}

/**
 * Starts the flow in `root`. It moves while the visitor wants it to (Pause and Play; with `still`, not until
 * they press Play) and `allow` says it may. `onChange` is called whenever it starts or stops moving.
 */
export function mountFlow(root: HTMLElement, options: { still: boolean; onChange?: () => void }): Flow {
  const stage = root.querySelector<HTMLElement>('.stage')!;
  const steps = [...root.querySelectorAll<HTMLElement>('.steps li')];
  const ctl = root.querySelector<HTMLButtonElement>('.ctl')!;

  /** What `layout` measured: the routes a request takes, and where the checkpoint from the server lands. */
  let routes: Record<string, number[][]> = {};
  let slot = [0, 0];

  const once =
    (selector: string, name: string): Run =>
    (q, instant) => {
      if (!instant) q(selector).forEach((part) => part.classList.add(name));
    };
  const view =
    (selector: string, name: string): Run =>
    (q) =>
      q(`${selector} .body`).forEach((part) => (part.dataset.view = name));
  const say =
    (selector: string, text: string, hot = false): Run =>
    (q) =>
      q(selector).forEach((part) => {
        part.textContent = text;
        part.classList.toggle('hot', hot);
      });
  /** Moves your pointer onto a button. */
  const point =
    (selector: string): Run =>
    (q) => {
      const frame = stage.getBoundingClientRect();
      const target = q(selector)[0].getBoundingClientRect();
      const x = Math.round(target.left - frame.left + target.width / 2 + 5);
      const y = Math.round(target.top - frame.top + target.height / 2 + 4);
      q('.cursor')[0].style.transform = `translate(${x}px, ${y}px)`;
    };
  const press =
    (selector: string): Run =>
    (q, instant) => {
      if (instant) return;
      const button = q(selector)[0];
      const cursor = q('.cursor')[0];
      button.classList.add('press');
      cursor.classList.add('click');
      window.setTimeout(() => {
        button.classList.remove('press');
        cursor.classList.remove('click');
      }, 400);
    };
  /** The checkpoint goes from the server to its place on your Mac. */
  const carry =
    (selector: string): Run =>
    (q) =>
      q(selector).forEach((part) => (part.style.transform = `translate(${slot[0]}px, ${slot[1]}px) rotate(45deg)`));
  /** A request leaves the fork. With `real`, it picks the secret up at the edge of your Mac. */
  const send =
    (selector: string, route: string, ms: number, real: boolean): Run =>
    (q, instant) => {
      if (!instant) follow(q(selector)[0], routes[route], ms, real);
    };

  // The flow, in the order it is told.
  const CUES: Cue[] = [
    // Fork: two tasks, two forks of main
    [200, add('.l1')],
    [600, add('.l2')],
    [1100, add('.wa, .nm, .na, .fa, .tf')],
    [1400, add('.wb, .nb, .fb')],
    [1900, add('.fa, .fb', 'busy')],
    // Work: both at once, each in its own virtual machine
    [2500, add('.fa .t1')],
    [2650, add('.fb .t1')],
    [3200, add('.fb .t2')],
    [3250, add('.fa .t2')],
    [3750, add('.fb .t3')],
    [3800, add('.fa .t3')],
    [4150, add('.fa .t4')],
    [4400, add('.port')],
    // Screen: the agent opens the app on the fork's own screen and clicks through it
    [5000, view('.fa', 'scrv')],
    [5300, add('.scrn', 'open')],
    [6200, add('.scrn', 'done')],
    // Secrets: a placeholder inside, the real value added outside, and only on the way to the allowed host
    [7600, view('.fb', 'term2')],
    [7700, add('.fb .u1')],
    [8100, add('.fb .u2')],
    [8200, add('.net, .gate, .wn')],
    [8600, add('.fb .u3')],
    [9000, send('.p1', 'toStripe', 1000, true)],
    [9230, add('.gate', 'lit')],
    [10000, add('.ws', 'real')],
    [10000, add('.hs', 'got')],
    [10000, add('.fb .u4')],
    [10150, send('.p2', 'toElse', 900, false)],
    [11050, add('.he', 'empty')],
    // Diff: everything each fork changed, past where a Git diff ends
    [11500, view('.fa', 'diff')],
    [11500, rem('.fa', 'busy')],
    [11600, add('.fa .d1')],
    [11800, add('.fa .cut')],
    [11950, add('.fa .d2')],
    [12100, add('.fa .d3')],
    [12250, add('.fa .d4')],
    [12400, view('.fb', 'diff')],
    [12400, rem('.fb', 'busy')],
    [12500, add('.fb .d1')],
    [12650, add('.fb .cut')],
    [12800, add('.fb .d2')],
    [13100, add('.fa, .fb', 'decide')],
    // Merge or drop
    [13900, add('.cursor')],
    [13950, point('.fa .merge')],
    [14800, press('.fa .merge')],
    [14950, rem('.fa', 'decide')],
    [14950, add('.wm')],
    [15500, once('.main', 'hit')],
    [15500, say('.pg', 'postgresql 17', true)],
    [15500, add('.undo')],
    [15500, add('.nm', 'hot')],
    [15500, say('.fa .pc-state', 'merged')],
    [15500, add('.fa', 'merged')],
    [15900, point('.fb .drop')],
    [16700, press('.fb .drop')],
    [16850, rem('.fb', 'decide')],
    [16850, add('.fb', 'dropped')],
    [16850, add('.wb, .nb, .net, .gate, .wn', 'gone')],
    [16850, add('.xb')],
    // Other devices: a teammate's task on the server is done, and you fork its computer onto your Mac to check it
    [17700, rem('.job', 'busy')],
    [17700, add('.job', 'decide')],
    [17900, point('.job .forkb')],
    [18700, press('.job .forkb')],
    [18850, add('.nj, .wsv, .cpk')],
    [18850, carry('.cpk')],
    [19300, rem('.cursor')],
    [19500, rem('.cpk, .xb')],
    [19500, add('.wb', 'away')],
    [19500, add('.fc')],
    [19500, add('.fc', 'busy')],
    [19900, add('.fc .c1')],
    [20400, add('.fc .c2')],
    [20700, add('.port2')],
  ];

  // Where everything is: the frames, the wires, the routes a request takes, and how fast each line is typed.
  function layout() {
    const svg = stage.querySelector<SVGSVGElement>('.wires')!;
    const width = stage.clientWidth;
    const height = stage.clientHeight;
    svg.setAttribute('width', `${width}`);
    svg.setAttribute('height', `${height}`);
    svg.setAttribute('viewBox', `0 0 ${width} ${height}`);
    // CSS says which way the picture reads at this width: across, or down in one column.
    const stack = getComputedStyle(stage).getPropertyValue('--mode').trim() === 'stack';
    const find = (selector: string) => stage.querySelector<HTMLElement>(selector)!;
    const box = (selector: string): Box => {
      const e = find(selector);
      let l = 0;
      let t = 0;
      for (let node: HTMLElement | null = e; node && node !== stage; node = node.offsetParent as HTMLElement | null) {
        l += node.offsetLeft;
        t += node.offsetTop;
      }
      return { e, l, t, r: l + e.offsetWidth, b: t + e.offsetHeight, cy: t + e.offsetHeight / 2 };
    };
    const px = (value: number) => Math.round(value) + 0.5;
    const place = (selector: string, x: number, y: number) => {
      const e = find(selector);
      e.style.left = `${Math.round(x)}px`;
      e.style.top = `${Math.round(y)}px`;
      return e;
    };
    const mac = box('.mac');
    const srv = box('.srv');
    const m = box('.main');
    const a = box('.fa');
    const b = box('.fb');
    const R = RADIUS;

    // main to its forks, and back
    let out: (fork: Box) => string;
    let back: (fork: Box) => string;
    let mainNode: number[];
    if (stack) {
      const tx = px(m.l + 14);
      out = (f) => `M${tx} ${px(m.b)}V${px(f.cy) - R}Q${tx} ${px(f.cy)} ${tx + R} ${px(f.cy)}H${px(f.l)}`;
      back = (f) => `M${px(f.l)} ${px(f.cy)}H${tx + R}Q${tx} ${px(f.cy)} ${tx} ${px(f.cy) - R}V${px(m.b)}`;
      mainNode = [tx, px(m.b)];
      find('.tf').hidden = true;
    } else {
      const x1 = px(m.r);
      const y1 = px(m.cy);
      const mx = px((m.r + a.l) / 2);
      const turn = (f: Box) => (px(f.cy) > y1 ? 1 : -1);
      out = (f) =>
        `M${x1} ${y1}H${mx - R}Q${mx} ${y1} ${mx} ${y1 + turn(f) * R}V${px(f.cy) - turn(f) * R}Q${mx} ${px(f.cy)} ${mx + R} ${px(f.cy)}H${px(f.l)}`;
      back = (f) =>
        `M${px(f.l)} ${px(f.cy)}H${mx + R}Q${mx} ${px(f.cy)} ${mx} ${px(f.cy) - turn(f) * R}V${y1 + turn(f) * R}Q${mx} ${y1} ${mx - R} ${y1}H${x1}`;
      mainNode = [x1, y1];
      // The word sits on the wire, when the wire is long enough to carry it.
      place('.tf', m.r + 9, m.cy - 22).hidden = mx - m.r < 48;
    }

    // out of the fork: the gate on the edge of your Mac, then each host
    const net = find('.net');
    let gx: number;
    let gy: number;
    let wireIn: string;
    let wireTo: (host: Box) => string;
    let route: (host: Box) => number[][];
    const r = 7;
    if (stack) {
      gx = px(b.l + 34);
      gy = px(mac.b);
      place('.net', gx + 22, mac.b + 16);
      wireIn = `M${gx} ${px(b.b)}V${gy - 10}`;
      wireTo = (h) => `M${gx} ${gy + 10}V${px(h.cy) - r}Q${gx} ${px(h.cy)} ${gx + r} ${px(h.cy)}H${h.l - 5}`;
      route = (h) => [
        [gx, b.b],
        [gx, gy],
        [gx, px(h.cy)],
        [h.l + 5, px(h.cy)],
      ];
    } else {
      gx = px(mac.r);
      gy = px(b.cy);
      place('.net', mac.r + 30, b.cy - net.offsetHeight / 2);
      const sx = px(gx + 18);
      const turn = (h: Box) => (px(h.cy) > gy ? 1 : -1);
      wireIn = `M${px(b.r)} ${gy}H${gx - 10}`;
      wireTo = (h) =>
        `M${gx + 10} ${gy}H${sx - r}Q${sx} ${gy} ${sx} ${gy + turn(h) * r}V${px(h.cy) - turn(h) * r}Q${sx} ${px(h.cy)} ${sx + r} ${px(h.cy)}H${h.l - 5}`;
      route = (h) => [
        [b.r, gy],
        [gx, gy],
        [sx, gy],
        [sx, px(h.cy)],
        [h.l + 5, px(h.cy)],
      ];
    }
    const hs = box('.hs');
    const he = box('.he');

    // the teammate's computer sits under the forks, and its fork lands where the dropped one was
    const jobEl = find('.job');
    jobEl.style.marginLeft = `${Math.max(0, b.l - srv.l - parseFloat(getComputedStyle(srv.e).paddingLeft))}px`;
    const job = box('.job');
    const jx = px(b.l + (stack ? 120 : 36));
    const fc = find('.fc');
    fc.style.left = `${b.l}px`;
    fc.style.top = `${b.t}px`;
    fc.style.width = `${b.r - b.l}px`;
    fc.style.setProperty('--dy', '46px');
    routes = { toStripe: route(hs), toElse: route(he) };
    slot = [jx, px(b.b)];
    find('.cpk').style.transform = `translate(${jx}px, ${px(job.t)}px) rotate(45deg)`;

    const node = (name: string, x: number, y: number) =>
      `<rect class="nd ${name}" x="-3" y="-3" width="6" height="6" transform="translate(${x} ${y}) rotate(45)"/>`;
    const frame = (f: Box) => `<rect class="frame" x="${px(f.l)}" y="${px(f.t)}" width="${f.r - f.l - 1}" height="${f.b - f.t - 1}"/>`;
    const bx = px(b.l);
    const by = px(b.cy);
    svg.innerHTML =
      frame(mac) +
      frame(srv) +
      `<path class="w wa" pathLength="1" d="${out(a)}"/><path class="w wb" pathLength="1" d="${out(b)}"/><path class="w wm" pathLength="1" d="${back(a)}"/>` +
      `<path class="wn" d="${wireIn}"/><path class="wn ws" d="${wireTo(hs)}"/><path class="wn" d="${wireTo(he)}"/>` +
      `<path class="w wsv" pathLength="1" d="M${jx} ${px(job.t)}V${px(b.b)}"/>` +
      node('nm', mainNode[0], mainNode[1]) +
      node('na', px(a.l), px(a.cy)) +
      node('nb', bx, by) +
      node('nj hot', jx, px(job.t)) +
      `<path class="x xb" d="M${bx - 4} ${by - 4}l8 8M${bx + 4} ${by - 4}l-8 8"/>` +
      `<g class="gate" transform="translate(${gx} ${gy})"><rect x="-10" y="-10" width="20" height="20" rx="5"/><circle cx="-3" cy="0" r="2.4"/><path d="M-0.6 0H5.5M3.2 0v2.2M5.5 0v2.2"/></g>`;

    place('.dev-mac', mac.l + 10, mac.t - 8);
    place('.dev-srv', srv.l + 10, srv.t - 8);
    place('.team', srv.r - find('.team').offsetWidth - 10, srv.t - 9);
    // A fork comes out of main: it starts from where main is.
    for (const f of [a, b]) {
      f.e.style.setProperty('--dx', `${m.l - f.l}px`);
      f.e.style.setProperty('--dy', `${m.cy - f.cy}px`);
    }
    stage.querySelectorAll<HTMLElement>('.ln:not(.reply)').forEach((line) => {
      const letters = line.textContent?.length ?? 0;
      line.style.transition = `clip-path ${letters * 18}ms steps(${letters}, end)`;
    });
    find('.cursor').style.transform = `translate(${Math.round(m.r + 30)}px, ${Math.round(mac.b - 30)}px)`;
  }

  const picture = scene(stage, CUES, END, layout);
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

  // A change of width moves the boxes, so everything is drawn again at the same moment of the flow.
  let width = stage.clientWidth;
  new ResizeObserver(() => {
    if (stage.clientWidth === width) return;
    width = stage.clientWidth;
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
