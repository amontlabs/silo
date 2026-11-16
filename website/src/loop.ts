// The picture of the loop on the home page. CSS lays out the boxes; this draws the wires between
// them, redraws them when the layout changes, and plays the drawing once.

declare global {
  interface Window {
    /** The head script's timer that shows the finished picture if this script has not started in time. */
    siloLoopFallback?: number;
  }
}

interface Box {
  l: number;
  t: number;
  r: number;
  b: number;
  cy: number;
}

/** Steps of the drawing, in the order the idea is told, with their start in milliseconds. */
const STEPS: [string, number][] = [
  ['s1', 80],
  ['s2', 620],
  ['s3', 1300],
  ['s4', 2250],
];
const DONE_AT = 3700;
const RADIUS = 12;

const px = (value: number) => Math.round(value) + 0.5;

function boxOf(loop: HTMLElement, selector: string): Box {
  const frame = loop.getBoundingClientRect();
  const rect = (loop.querySelector(selector) as HTMLElement).getBoundingClientRect();
  const l = rect.left - frame.left;
  const t = rect.top - frame.top;
  return { l, t, r: l + rect.width, b: t + rect.height, cy: t + rect.height / 2 };
}

/** Draws the wires, nodes and the drop cross for the current layout, and places the fork label. */
export function drawWires(loop: HTMLElement): void {
  const svg = loop.querySelector<SVGSVGElement>('.wires');
  const verb = loop.querySelector<HTMLElement>('.verb');
  const W = loop.clientWidth;
  const H = loop.clientHeight;
  if (!svg || !verb || !W) return;
  svg.setAttribute('width', String(W));
  svg.setAttribute('height', String(H));
  svg.setAttribute('viewBox', `0 0 ${W} ${H}`);
  const wide = getComputedStyle(loop).getPropertyValue('--mode').trim() === 'wide';
  const [m, c1, d1, e1, c2, d2, e2] = ['.main', '.c1', '.d1', '.e1', '.c2', '.d2', '.e2'].map((selector) => boxOf(loop, selector));
  const R = RADIUS;
  const out: string[] = [];
  const wire = (name: string, d: string) => out.push(`<path class="w ${name}" pathLength="1" d="${d}"/>`);
  const node = (name: string, x: number, y: number) =>
    out.push(`<rect class="n ${name}" x="-3" y="-3" width="6" height="6" transform="translate(${x} ${y}) rotate(45)"/>`);
  const cross = (x: number, y: number) => out.push(`<path class="x" d="M${x - 4} ${y - 4}l8 8M${x + 4} ${y - 4}l-8 8"/>`);
  if (wide) {
    // Main runs along the top; each copy hangs off it and its changes go right, to merge or drop.
    const yM = px(m.cy);
    const tx = px(m.l + 22);
    const y1 = px(c1.cy);
    const y2 = px(c2.cy);
    const rise = px(e1.l + 14);
    wire('w-main', `M${m.r} ${yM}H${W - 1}`);
    wire('w-arrow', `M${W - 8} ${yM - 4}L${W - 1} ${yM}L${W - 8} ${yM + 4}`);
    wire('w-f1', `M${tx} ${m.b}V${y1 - R}Q${tx} ${y1} ${tx + R} ${y1}H${c1.l}`);
    wire('w-f2', `M${tx} ${m.b}V${y2 - R}Q${tx} ${y2} ${tx + R} ${y2}H${c2.l}`);
    wire('w-d1', `M${c1.r} ${y1}H${d1.l}`);
    wire('w-d2', `M${c2.r} ${y2}H${d2.l}`);
    wire('w-merge', `M${d1.r} ${y1}H${rise - R}Q${rise} ${y1} ${rise} ${y1 - R}V${yM + R}Q${rise} ${yM} ${rise + R} ${yM}`);
    wire('w-drop', `M${d2.r} ${y2}H${rise - 9}`);
    node('n-main', px(m.r), yM);
    node('n-fork', tx, px(m.b));
    node('n-fork', px(c1.l), y1);
    node('n-fork', px(c2.l), y2);
    node('n-diff', px(d1.l), y1);
    node('n-diff', px(d2.l), y2);
    node('n-merge', rise + R, yM);
    cross(rise, y2);
  } else {
    // Main runs down the left edge; each copy reads downward to its changes, then merge or drop.
    const tx = px(m.l + 12);
    const y1 = px(c1.cy);
    const y2 = px(c2.cy);
    const sx = px(c1.l + 20);
    const ym = px(e1.cy);
    const yd = px(e2.cy);
    const end = H - 1;
    wire('w-main', `M${tx} ${m.b}V${end}`);
    wire('w-arrow', `M${tx - 4} ${end - 7}L${tx} ${end}L${tx + 4} ${end - 7}`);
    wire('w-f1', `M${tx} ${y1 - R}Q${tx} ${y1} ${tx + R} ${y1}H${c1.l}`);
    wire('w-f2', `M${tx} ${y2 - R}Q${tx} ${y2} ${tx + R} ${y2}H${c2.l}`);
    wire('w-d1', `M${sx} ${c1.b}V${d1.t}`);
    wire('w-d2', `M${sx} ${c2.b}V${d2.t}`);
    wire('w-merge', `M${sx} ${d1.b}V${ym - R}Q${sx} ${ym} ${sx - R} ${ym}H${tx + R}Q${tx} ${ym} ${tx} ${ym + R}`);
    wire('w-drop', `M${sx} ${d2.b}V${yd - 9}`);
    node('n-main', tx, px(m.b));
    node('n-fork', px(c1.l), y1);
    node('n-fork', px(c2.l), y2);
    node('n-diff', sx, px(d1.t));
    node('n-diff', sx, px(d2.t));
    node('n-merge', tx, ym + R);
    cross(sx, yd);
  }
  svg.innerHTML = out.join('');
  verb.style.left = `${Math.round(m.l + (wide ? 34 : 24))}px`;
  verb.style.top = `${Math.round(m.b + (wide ? 12 : 9))}px`;
}

function start(): void {
  const root = document.documentElement;
  const loop = document.getElementById('loop');
  const replay = document.getElementById('replay');
  clearTimeout(window.siloLoopFallback);
  // The head script's fallback already showed the finished picture: it stays finished.
  const shownFinished = !root.classList.contains('loop-pending');
  if (!loop || !replay) {
    root.classList.remove('loop-pending');
    return;
  }

  // Rebuilding the wires mid-drawing would jump each one to its end, so a layout change while
  // the picture draws is applied once the drawing is over.
  let drawing = false;
  let stale = false;
  const draw = () => {
    if (drawing) stale = true;
    else drawWires(loop);
  };
  draw();
  new ResizeObserver(draw).observe(loop);
  void document.fonts?.ready.then(draw);

  const steps = STEPS.map(([name]) => name);
  let timers: number[] = [];
  function finish(): void {
    loop!.classList.remove('anim', ...steps);
    drawing = false;
    if (stale) {
      stale = false;
      drawWires(loop!);
    }
  }
  function play(): void {
    timers.forEach(clearTimeout);
    drawing = false;
    stale = false;
    drawWires(loop!);
    drawing = true;
    loop!.classList.remove(...steps);
    loop!.classList.add('anim', 'reset');
    void loop!.offsetWidth;
    loop!.classList.remove('reset');
    root.classList.remove('loop-pending');
    timers = STEPS.map(([name, delay]) => window.setTimeout(() => loop!.classList.add(name), delay));
    timers.push(window.setTimeout(finish, DONE_AT));
  }

  if (matchMedia('(prefers-reduced-motion: reduce)').matches) {
    root.classList.remove('loop-pending');
    return;
  }
  replay.hidden = false;
  replay.addEventListener('click', play);
  if (shownFinished) return;
  // Draws once, when about a third of the picture is in view (so it is not over before anyone sees
  // it), and only once the fonts are in, since the page stays hidden until then.
  const begin = () => {
    if (!('IntersectionObserver' in window)) {
      play();
      return;
    }
    const seen = new IntersectionObserver(
      (entries) => {
        if (!entries.some((entry) => entry.isIntersecting)) return;
        seen.disconnect();
        play();
      },
      { threshold: 0.33 },
    );
    seen.observe(loop);
  };
  if (document.fonts) void document.fonts.ready.then(begin, begin);
  else begin();
}

start();
