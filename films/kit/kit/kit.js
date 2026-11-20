/* Fjord explainer kit: a stage with a camera, crafted pieces, and the player around them.
   The creator writes one call: explainer({ title, answer, scenes, build(K, tl, scene) }).
   Everything on stage is plain SVG driven by plain GSAP tweens on one paused timeline: no callbacks, no clocks,
   so any frame can be scrubbed to or rendered offscreen, and a theme change can rebuild the film where it stands. */
(() => {
  gsap.registerPlugin(MotionPathPlugin, DrawSVGPlugin, MorphSVGPlugin, CustomEase);
  const NS = "http://www.w3.org/2000/svg";
  const W = 1280, H = 720, HOLD = 1.4, END_HOLD = 2;
  const root = document.documentElement, stage = document.getElementById("stage");
  const ICONS = window.FJORD_ICONS || {};
  const SANS = "Geist, -apple-system, system-ui, sans-serif", MONO = "'Geist Mono', ui-monospace, Menlo, monospace";
  const EASE = {
    out: CustomEase.create("fjOut", "0.16,1,0.3,1"),        // entrances: fast, long settle, no overshoot
    inOut: CustomEase.create("fjInOut", "0.65,0,0.2,1"),    // moves between two rests
    cam: CustomEase.create("fjCam", "0.6,0,0.12,1"),        // the camera: eases off the mark, lands softly
  };

  // ---------- colour ----------
  // Fjord's cool ink-and-paper neutrals and the Router's status colours (Brand.swift, Theme.swift), both looks.
  // bgHi is the light falling on the stage from above, bgLo its floor and corners.
  const PALETTES = {
    dark: { bg: "#0d1114", bgHi: "#1a232a", bgLo: "#06080a", surface: "#1a2025", surfaceHi: "#252d34", ink: "#eef0ee", muted: "#9aa3a9", faint: "#5f6a72",
            line: "#323b42", accent: "#6da7ec", ok: "#55bf50", bad: "#ec7e7e", warn: "#f0a92a", you: "#a096eb", shadow: "#000000" },
    light: { bg: "#e4e8eb", bgHi: "#fdfefe", bgLo: "#c5ced6", surface: "#f6f8f9", surfaceHi: "#ffffff", ink: "#101418", muted: "#5d6871", faint: "#98a2aa",
             line: "#cdd3d8", accent: "#2b63c6", ok: "#2f8a3b", bad: "#cf3a3a", warn: "#b07400", you: "#6a58d8", shadow: "#14273a" },
  };
  const D = PALETTES.dark; // a terminal is dark in both looks
  let P = PALETTES.light, dark = false;
  const tone = t => P[t] || t;
  const hex = c => /^#[0-9a-f]{6}$/i.test(c) ? [1, 3, 5].map(i => parseInt(c.slice(i, i + 2), 16)) : null;
  const mix = (a, b, t) => {
    const x = hex(a), y = hex(b);
    return x && y ? "#" + x.map((v, i) => Math.round(v + (y[i] - v) * t).toString(16).padStart(2, "0")).join("") : b;
  };
  const wash = (t, k = 1) => mix(P.surfaceHi, tone(t), (dark ? 0.17 : 0.11) * k);  // a tone's pale fill, opaque
  const edge = (t, k = 0.5) => mix(P.surfaceHi, tone(t), k);                         // a tone's hairline
  const neutral = t => !t || ["ink", "muted", "faint", "line"].includes(t);
  const clamp01 = v => Math.max(0, Math.min(1, v));

  // ---------- drawing ----------
  let uidN = 0, L = {}, pieces = [], links = [], notes = [];
  // The film is built twice. The first pass only measures (what each camera framing will have on stage, the widest
  // words a card will ever hold); the second is the film, built knowing it.
  let rec = null, plan = null;
  const uid = () => "fj" + (++uidN);
  const note = m => { if (!notes.includes(m)) notes.push(m); };
  const make = (tag, attrs = {}, parent) => {
    const e = document.createElementNS(NS, tag);
    for (const k in attrs) if (attrs[k] != null) e.setAttribute(k, attrs[k]);
    (parent || L.pieces).appendChild(e);
    return e;
  };
  const set = (e, attrs) => { for (const k in attrs) e.setAttribute(k, attrs[k]); };
  const textEl = (parent, x, y, str, { size = 20, color = "ink", weight = 400, anchor = "middle", mono = false } = {}) => {
    const t = make("text", {
      x, y, "font-size": size, "font-weight": mono ? 400 : weight, "text-anchor": anchor, fill: tone(color), "dominant-baseline": "central",
      "font-family": mono ? MONO : SANS, "letter-spacing": mono ? null : size >= 40 ? "-0.035em" : "-0.011em",
    }, parent);
    t.textContent = str;
    return t;
  };
  const len = t => t.getComputedTextLength();
  // Words that must fit a width are cut short with an ellipsis, and the creator is told.
  function clip(t, maxW, where) {
    if (len(t) <= maxW) return;
    const str = t.textContent;
    let n = str.length;
    do { t.textContent = str.slice(0, --n).trimEnd() + "…"; } while (n > 1 && len(t) > maxW);
    note(`${where}: "${str}" is too long and is cut short. Use fewer words${/line|row|title/.test(where) ? ", or a wider w" : ""}.`);
  }
  const rect = (parent, w, h, rx, attrs = {}) => make("rect", { x: -w / 2, y: -h / 2, width: w, height: h, rx, ...attrs }, parent);
  // Depth without filters. A soft shape (a shadow, a glow, the ground under big type) is nine gradient tiles: four
  // corners, four edges and a middle. It is as smooth as a blur and costs almost nothing to redraw, so a camera move
  // over twenty pieces stays smooth. `fall` px of falloff around the shape shrunk by `inset`, dropped by `dy`.
  const FALL = [1, 0.94, 0.78, 0.55, 0.32, 0.14, 0.04, 0];
  function soft(parent, w, h, rx, { color, alpha, fall, inset = 0, dy = 0 }) {
    const g = make("g", { "shape-rendering": "crispEdges" }, parent);
    const rc0 = Math.max(rx - inset, 2);
    const tiles = w2 => {
      const l = -w2 / 2 + inset, r = w2 / 2 - inset, t = -h / 2 + inset + dy, b = h / 2 - inset + dy;
      const rc = Math.max(0.5, Math.min(rc0, (r - l) / 2, (b - t) / 2)), R = rc + fall, xl = l + rc, xr = r - rc, yt = t + rc, yb = b - rc;
      return { rc, R, at: [[xl - R, yt - R, R, R], [xr, yt - R, R, R], [xl - R, yb, R, R], [xr, yb, R, R],
                            [xl, yt - R, xr - xl, R], [xl, yb, xr - xl, R], [xl - R, yt, R, yb - yt], [xr, yt, R, yb - yt], [xl, yt, xr - xl, yb - yt]] };
    };
    const first = tiles(w), q = first.rc / first.R, id = `fj-s${color.replace(/\W/g, "")}-${Math.round(alpha * 1000)}-${Math.round(first.rc * 10)}-${Math.round(fall)}`;
    if (!stage.querySelector(`#${id}-0`)) {
      const stops = FALL.map((f, i) => `<stop offset="${(q + (1 - q) * i / (FALL.length - 1)).toFixed(4)}" stop-color="${color}" stop-opacity="${(alpha * f).toFixed(4)}"/>`).join("");
      const defs = stage.querySelector("defs");
      [[1, 1], [0, 1], [1, 0], [0, 0]].forEach(([x, y], i) => defs.insertAdjacentHTML("beforeend", `<radialGradient id="${id}-${i}" cx="${x}" cy="${y}" fx="${x}" fy="${y}" r="1">${stops}</radialGradient>`));
      [[0, 1, 0, 0], [0, 0, 0, 1], [1, 0, 0, 0], [0, 0, 1, 0]].forEach(([x1, y1, x2, y2], i) => defs.insertAdjacentHTML("beforeend", `<linearGradient id="${id}-${i + 4}" x1="${x1}" y1="${y1}" x2="${x2}" y2="${y2}">${stops}</linearGradient>`));
    }
    const box = ([x, y, w2, h2]) => ({ x, y, width: Math.max(0, w2), height: Math.max(0, h2) });
    const els = first.at.map((r, i) => make("rect", { ...box(r), fill: i < 8 ? `url(#${id}-${i})` : color, "fill-opacity": i < 8 ? null : alpha }, g));
    g.fit = (w2, t, dur) => tiles(w2).at.forEach((r, i) => { if (t) t.to(els[i], { attr: box(r), duration: dur, ease: EASE.inOut }, 0); else set(els[i], box(r)); });
    return g;
  }
  // A shadow is two soft shapes: where the piece touches the ground, and the light from above. 0 a chip, 1 a card, 2 a window.
  const DEPTH = [{ fall: 14, inset: 3, dy: 5 }, { fall: 26, inset: 5, dy: 9 }, { fall: 58, inset: 10, dy: 24 }];
  function shade(parent, w, h, rx, lvl = 1) {
    const g = make("g", {}, parent), k = dark ? [0.4, 0.42, 0.5, 0.58] : [0.1, 0.13, 0.17, 0.25];
    const parts = [soft(g, w, h, rx, { color: P.shadow, alpha: k[0], fall: 5 + lvl, inset: 0.5, dy: 1.5 }), soft(g, w, h, rx, { color: P.shadow, alpha: k[lvl + 1], ...DEPTH[lvl] })];
    g.fit = (w2, t, dur) => parts.forEach(s => s.fit(w2, t, dur));   // follow a chip that changes width
    parent.insertBefore(g, parent.firstChild);
    return g;
  }
  // A light behind a piece, in a tone.
  function halo(parent, w, h, rx, c, after) {
    const g = make("g", { opacity: 0 }, parent);
    soft(g, w, h, rx, { color: c, alpha: dark ? 0.5 : 0.36, fall: 34, inset: 3 });
    parent.insertBefore(g, after ? after.nextSibling : parent.firstChild);
    return g;
  }
  function iconEl(parent, name, x, y, size, color) {
    if (!ICONS[name]) { note(`No icon named "${name}". Icons: ${Object.keys(ICONS).join(" ")}`); name = "alert"; }
    const g = make("g", { fill: "none", stroke: tone(color), "stroke-width": (Math.max(1.5, size * 0.07) * 24 / size).toFixed(2), "stroke-linecap": "round", "stroke-linejoin": "round" }, parent);
    make("g", { transform: "translate(-12 -12)" }, g).innerHTML = ICONS[name];
    gsap.set(g, { x, y, scale: size / 24, svgOrigin: "0 0" });
    g._s = size / 24;
    return g;
  }
  // Words wrapped to a width: a <text> of <tspan> lines, its first line centred on y.
  function wrapped(parent, x, y, str, maxW, opts) {
    const lh = Math.round((opts.size || 16) * 1.38), probe = textEl(parent, 0, 0, "", opts), lines = [];
    let line = "";
    for (const word of String(str).split(/\s+/)) {
      probe.textContent = line ? line + " " + word : word;
      if (line && len(probe) > maxW) { lines.push(line); line = word; } else line = probe.textContent;
    }
    lines.push(line);
    let w = 0;
    for (const l of lines) { probe.textContent = l; w = Math.max(w, len(probe)); }
    probe.textContent = "";
    lines.forEach((l, i) => { const s = make("tspan", { x, dy: i ? lh : 0 }, probe); s.textContent = l; });
    probe.setAttribute("x", x); probe.setAttribute("y", y);
    return { el: probe, w, h: lines.length * lh, lh };
  }

  // Every piece is a group centred on its own origin. It starts invisible; a motion brings it in.
  // _x/_y are where it rests once the motions built so far have run, so later pieces and links follow a move.
  // _op is the opacity its creator last gave it; the spotlight and the camera fade it from there and give it back.
  function piece(kind, x, y, parent, owner) {
    const g = make("g", {}, parent);
    gsap.set(g, { x, y, opacity: 0, svgOrigin: "0 0" });
    Object.assign(g, { kind, _x: x, _y: y, _owner: owner || null, _shown: false, _op: 0, _i: pieces.length, w: 0, h: 0 });
    g.at = side => anchor(g, side);
    pieces.push(g);
    return g;
  }
  // A row's origin is its window's top edge (_cy), so a window can grow without moving what it holds.
  const wx = p => p._x + (p._owner ? wx(p._owner) : 0), wy = p => p._y + (p._owner ? wy(p._owner) + (p._owner._cy || 0) : 0);
  const cy = p => wy(p) + (p._oy || 0);
  const isPiece = p => p && typeof p.at === "function";
  const pin = p => { if (isPiece(p)) (p._owner || p)._pin = true; return p; };    // measured or shown: from now on it grows downwards only
  const tracked = p => isPiece(p) && !p._owner && !p.lens && p.kind !== "region";
  const label = p => { const t = p.label || p.text || p.title; return `${p.kind} "${(t ? t.textContent : "").slice(0, 28)}"`; };
  function bounds(p) { // the piece's whole footprint in the world, labels included
    if (p.kind === "link") { const b = p.getBBox(); return { l: b.x, t: b.y, r: b.x + b.width, b: b.y + b.height }; }
    if (!isPiece(p)) return { l: p.x - (p.w || 0) / 2, r: p.x + (p.w || 0) / 2, t: p.y - (p.h || 0) / 2, b: p.y + (p.h || 0) / 2 };
    pin(p);
    const x = wx(p), y = cy(p), bb = p.bb || { l: -p.w / 2, r: p.w / 2, t: -p.h / 2, b: p.h / 2 };
    return { l: x + bb.l, r: x + bb.r, t: y + bb.t, b: y + bb.b };
  }
  const join = (a, b) => ({ l: Math.min(a.l, b.l), t: Math.min(a.t, b.t), r: Math.max(a.r, b.r), b: Math.max(a.b, b.b) });
  const union = list => list.map(bounds).reduce(join);
  const NORMAL = { left: [-1, 0], right: [1, 0], top: [0, -1], bottom: [0, 1], center: [0, 0] };
  function anchor(p, side = "center", gap = 0) {
    const n = NORMAL[side];
    if (!n) throw new Error(`at("${side}"): the sides are "left", "right", "top", "bottom" and "center".`);
    const b = abox(p);
    if (p._owner && n[0]) { const o = abox(p._owner); b.l = o.l; b.r = o.r; } // a row is entered at its window's edge
    return { x: n[0] ? (n[0] < 0 ? b.l - gap : b.r + gap) : (b.l + b.r) / 2, y: n[1] ? (n[1] < 0 ? b.t - gap : b.b + gap) : p.round ? cy(p) : (b.t + b.b) / 2 };
  }
  function abox(p) { // where links attach: the piece's body, without the labels around it
    if (!isPiece(p)) return { l: p.x, r: p.x, t: p.y, b: p.y };
    pin(p);
    const x = wx(p), y = cy(p), a = p.ab || { l: -p.w / 2, r: p.w / 2, t: -p.h / 2, b: p.h / 2 };
    return { l: x + a.l, r: x + a.r, t: y + a.t, b: y + a.b };
  }

  // ---------- the stage ----------
  function setStage() {
    stage.replaceChildren();
    uidN = 0; pieces = []; links = []; notes = [];
    make("defs", {}, stage).innerHTML = `
      <radialGradient id="fj-bg" cx="50%" cy="-12%" r="110%"><stop offset="0" stop-color="${P.bgHi}"/><stop offset="${dark ? 1 : 0.7}" stop-color="${P.bg}"/></radialGradient>
      <radialGradient id="fj-light" cx="50%" cy="0%" r="72%"><stop offset="0" stop-color="${P.accent}" stop-opacity="0.1"/><stop offset="1" stop-color="${P.accent}" stop-opacity="0"/></radialGradient>
      <radialGradient id="fj-vig" cx="50%" cy="42%" r="78%"><stop offset="0.45" stop-color="${P.bgLo}" stop-opacity="0"/><stop offset="1" stop-color="${P.bgLo}" stop-opacity="${dark ? 0.9 : 0.62}"/></radialGradient>
      <pattern id="fj-dots" width="32" height="32" patternUnits="userSpaceOnUse"><circle cx="16" cy="16" r="1.1" fill="${P.ink}" opacity="${dark ? 0.1 : 0.15}"/></pattern>
      <linearGradient id="fj-surf" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${P.surfaceHi}"/><stop offset="1" stop-color="${P.surface}"/></linearGradient>
      <linearGradient id="fj-rim" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="${dark ? mix(P.line, P.ink, 0.22) : mix(P.line, "#ffffff", 0.3)}"/><stop offset="${dark ? 0.5 : 1}" stop-color="${dark ? P.line : mix(P.line, P.ink, 0.16)}"/></linearGradient>
      <linearGradient id="fj-type" x1="0" y1="0" x2="0" y2="1"><stop offset="0.2" stop-color="${P.ink}"/><stop offset="1" stop-color="${mix(P.ink, P.bg, dark ? 0.42 : 0.3)}"/></linearGradient>`;
    const back = make("g", { id: "fj-back" }, stage);
    make("rect", { width: W, height: H, fill: "url(#fj-bg)" }, back);
    L.grid = make("g", {}, back);                         // drifts with the camera at a third of its travel: depth
    make("rect", { x: -W, y: -H, width: W * 3, height: H * 3, fill: "url(#fj-dots)" }, L.grid);
    make("rect", { width: W, height: H, fill: "url(#fj-light)" }, back);
    make("rect", { width: W, height: H, fill: "url(#fj-vig)" }, back);
    L.world = make("g", {}, stage);                       // what the camera looks at
    for (const n of ["regions", "links", "pieces", "top", "fx"]) L[n] = make("g", {}, L.world);
    L.ground = make("g", {}, stage);                      // the backdrop again, where big type needs clear ground
    L.lens = make("g", {}, stage);                        // on the glass: big type, unmoved by the camera
    gsap.set([L.world, L.grid], { x: 0, y: 0, scale: 1, svgOrigin: "0 0" });
  }

  // ---------- pieces ----------
  function card(x, y, title, { sub, icon, tone: tn, w, dashed = false } = {}) {
    const g = piece("card", x, y, L.pieces), c = neutral(tn) ? null : tone(tn);
    const h = sub ? 78 : 60, tile = icon ? 42 : 0, lead = icon ? 14 + tile + 13 : 24;
    g.label = textEl(g, 0, sub ? -11 : 0, title, { size: 20, weight: 500, anchor: icon ? "start" : "middle" });
    if (sub) g.sub = textEl(g, 0, 15, sub, { size: 15, color: "muted", anchor: icon ? "start" : "middle" });
    const tw = Math.max(len(g.label), sub ? len(g.sub) : 0);
    w = Math.max(w || 0, lead + tw + 24, 140, plan && plan.reserve[g._i] || 0);   // room for the words a relabel will bring
    g.box = rect(g, w, h, 16, { fill: dashed ? P.surface : "url(#fj-surf)", "fill-opacity": dashed ? 0.55 : null, stroke: c ? edge(tn, 0.62) : dashed ? P.faint : "url(#fj-rim)", "stroke-width": 1.25,
                                "stroke-dasharray": dashed ? "5 5" : null });
    g.insertBefore(g.box, g.label);
    if (!dashed) g.shade = shade(g, w, h, 16, 1);
    if (icon) {
      g.tile = rect(g, tile, tile, 12, { fill: c ? wash(tn) : mix(P.surfaceHi, P.ink, 0.07) });
      gsap.set(g.tile, { x: -w / 2 + 14 + tile / 2 });
      g.insertBefore(g.tile, g.label);
      g.icon = iconEl(g, icon, -w / 2 + 14 + tile / 2, 0, 22, c || P.ink);
      [g.label, g.sub].forEach(t => t && t.setAttribute("x", -w / 2 + lead));
    }
    Object.assign(g, { w, h, rx: 16, _lead: lead, extras: g.icon ? [g.icon] : [] });
    g.paint = t => [[g.box, { stroke: edge(t, 0.62) }], [g.tile, { fill: wash(t) }], [g.icon, { stroke: tone(t) }]];
    return g;
  }
  function actor(x, y, icon, label, { sub, tone: tn = "ink", r = 36 } = {}) {
    const g = piece("actor", x, y, L.pieces), c = tone(tn), R = r + 8;
    g.ring = make("circle", { r: R, fill: "none", stroke: c, "stroke-opacity": neutral(tn) ? 0.16 : 0.3, "stroke-width": 1.25 }, g);
    g.box = make("circle", { r, fill: "url(#fj-surf)", stroke: neutral(tn) ? "url(#fj-rim)" : edge(tn, 0.6), "stroke-width": 1.25 }, g);
    g.shade = shade(g, r * 2, r * 2, r, 1);
    g.icon = iconEl(g, icon, 0, 0, Math.round(r * 0.86), c);
    let half = R, bottom = R;
    if (label) { g.label = textEl(g, 0, R + 20, label, { size: 18, weight: 500 }); half = Math.max(half, len(g.label) / 2); bottom = R + 32; }
    if (sub) { g.sub = textEl(g, 0, R + 42, sub, { size: 14, color: "muted" }); half = Math.max(half, len(g.sub) / 2); bottom = R + 53; }
    Object.assign(g, { w: R * 2, h: R * 2, round: true, bb: { l: -half, r: half, t: -R, b: bottom }, ab: { l: -R, r: R, t: -R, b: bottom + 2 }, extras: [g.icon, g.ring] });
    g.paint = t => [[g.box, { stroke: edge(t, 0.6) }], [g.ring, { stroke: tone(t) }], [g.icon, { stroke: tone(t) }]];
    return g;
  }
  function chip(x, y, label, { tone: tn = "accent", solid = false, size = 17, mono = false, icon } = {}) {
    const g = piece("chip", x, y, L.top), plain = neutral(tn), h = size + 18;
    const colors = t => solid ? { fill: tone(t), stroke: tone(t), text: P.bg } : neutral(t) ? { fill: P.surfaceHi, stroke: mix(P.line, P.ink, dark ? 0 : 0.08), text: tone(t || "ink") } : { fill: wash(t), stroke: edge(t, 0.5), text: tone(t) };
    const k = colors(tn);
    g.box = rect(g, 10, h, h / 2, { fill: k.fill, stroke: k.stroke, "stroke-width": 1.25 });
    g.shade = shade(g, 10, h, h / 2, 0);
    g.label = textEl(g, 0, 0, label, { size, weight: 500, color: k.text, mono });
    if (icon) g.icon = iconEl(g, icon, 0, 0, size + 1, k.text);
    Object.assign(g, { h, rx: h / 2, _tone: tn, _lead: icon ? size + 8 : 0, _pad: Math.round(size * 0.82) });
    g.fit = tw => { // the pill's width and where its icon and words sit, for words `tw` wide
      const w = Math.max(tw + g._lead + g._pad * 2, h + 8);
      return { w, labelX: g._lead / 2, iconX: -(tw + g._lead) / 2 + (size + 1) / 2 };
    };
    g.resize = () => { // to the words it holds now
      const f = g.fit(len(g.label));
      g.w = f.w; set(g.box, { x: -f.w / 2, width: f.w }); g.shade.fit(f.w); g.label.setAttribute("x", f.labelX);
      if (g.icon) gsap.set(g.icon, { x: f.iconX });
    };
    g.resize();
    g.paint = t => { const k = colors(t); return [[g.box, { fill: k.fill, stroke: k.stroke }], [g.label, { fill: k.text }], [g.icon, { stroke: k.text }]]; };
    return g;
  }
  function icon(x, y, name, { size = 30, tone: tn = "ink" } = {}) {
    const g = piece("icon", x, y, L.pieces);
    g.icon = iconEl(g, name, 0, 0, size, tn);
    Object.assign(g, { w: size + 10, h: size + 10 });
    g.paint = t => [[g.icon, { stroke: tone(t) }]];
    return g;
  }
  function text(x, y, str, { size = 20, color = "ink", weight = 400, anchor: an = "middle", mono = false } = {}) {
    const g = piece("text", x, y, L.pieces);
    g.text = textEl(g, 0, 0, str, { size, color, weight, anchor: an, mono });
    const w = len(g.text), l = an === "start" ? 0 : an === "end" ? -w : -w / 2;
    Object.assign(g, { w, h: size * 1.3, bb: { l, r: l + w, t: -size * 0.65, b: size * 0.65 } });
    g.paint = t => [[g.text, { fill: tone(t) }]];
    return g;
  }
  // Type as image: a word or number that fills the frame. It sits on the lens, so the camera doesn't move it, and on
  // its own ground: the backdrop itself, drawn again through a soft-edged mask, clears whatever lies behind the words.
  function big(x, y, str, { size = 132, tone: tn = "ink", sub, anchor: an = "middle" } = {}) {
    const g = piece("big", x, y, L.lens), subSize = Math.max(20, Math.round(size * 0.19)), id = uid();
    g.ground = make("g", { opacity: 0 }, L.ground);
    const hole = make("mask", { id, maskUnits: "userSpaceOnUse", x: 0, y: 0, width: W, height: H }, g.ground);
    make("use", { href: "#fj-back", mask: `url(#${id})` }, g.ground);
    g.text = textEl(g, 0, 0, str, { size, weight: 500, anchor: an, color: tn === "ink" ? "url(#fj-type)" : tn });
    g.text.style.fontVariantNumeric = "tabular-nums";
    if (sub) g.sub = textEl(g, an === "start" ? size * 0.04 : 0, size * 0.62 + 12, sub, { size: subSize, color: "muted", anchor: an });
    g.refit = () => { // the ground follows the words (a count changes them)
      const w = Math.max(len(g.text), sub ? len(g.sub) : 0), l = an === "start" ? 0 : an === "end" ? -w : -w / 2;
      const t = -size * 0.5, b = sub ? size * 0.62 + 12 + subSize * 0.75 : size * 0.5;
      hole.replaceChildren();
      gsap.set(soft(hole, w, b - t, 26, { color: "#ffffff", alpha: 1, fall: 40, inset: -14 }), { x: x + l + w / 2, y: y + (t + b) / 2 });
      Object.assign(g, { w, h: b - t, bb: { l, r: l + w, t, b } });
    };
    g.refit();
    g.lens = true;
    g.enter = dur => {
      const t = gsap.timeline();
      t.fromTo(g, { opacity: 0, y: g._y + size * 0.22, scale: 0.94 }, { opacity: 1, y: g._y, scale: 1, duration: dur * 1.3, ease: EASE.out }, 0);
      t.fromTo(g.ground, { opacity: 0 }, { opacity: 1, duration: dur * 0.8, ease: "power2.out" }, 0);
      if (g.sub) t.fromTo(g.sub, { opacity: 0 }, { opacity: 1, duration: 0.5, ease: "power2.out" }, dur * 0.45);
      return t;
    };
    g.paint = t => [[g.text, { fill: tone(t) }]];
    return g;
  }

  // A window, thread, terminal or document is as tall as what it holds: it fits its rows as they are added, around
  // its centre. Once it has been shown, linked or wrapped in a region it stays where it is and grows downwards.
  // Give h only to keep room for rows that arrive later. What it holds hangs from its top edge (g.top).
  function container(g, w, h, min, redraw) {
    g.in = make("g", {}, g); g.top = make("g", {}, g.in);
    Object.assign(g, { w, h: 0, _oy: 0, _cy: 0 });
    g.size = need => {
      const H = Math.max(h ?? min, need, g.h);
      if (H === g.h) return;
      if (g.h && g._pin) g._oy += (H - g.h) / 2;
      g.h = H; g._cy = g._oy - H / 2;
      gsap.set(g.in, { y: g._oy }); gsap.set(g.top, { y: -H / 2 });
      if (g.shade) g.shade.remove();
      redraw(H);
    };
  }
  // A window's chrome: surface, title bar, three quiet lights. Shared by window and thread.
  function frame(g, w, h, title, { glyph, min } = {}) {
    container(g, w, h, min, H => { set(g.box, { y: -H / 2, height: H }); g.shade = shade(g.in, w, H, 14, 2); });
    g.box = rect(g.in, w, 10, 14, { fill: "url(#fj-surf)", stroke: "url(#fj-rim)", "stroke-width": 1.25 });
    g.in.insertBefore(g.box, g.top);
    make("path", { d: `M${-w / 2 + 14} 0.6 h${w - 28}`, stroke: "#ffffff", "stroke-opacity": dark ? 0.09 : 0.9, "stroke-width": 1 }, g.top);
    make("path", { d: `M${-w / 2} 40 h${w}`, stroke: P.line, "stroke-width": 1 }, g.top);
    for (let i = 0; i < 3; i++) make("circle", { cx: -w / 2 + 20 + i * 15, cy: 20, r: 4.5, fill: mix(P.surface, P.ink, dark ? 0.2 : 0.16) }, g.top);
    // The title sits in the middle when it fits there, and clear of the lights always.
    const lead = glyph ? 22 : 0, left = -w / 2 + 66;
    g.title = textEl(g.top, 0, 20, title, { size: 15, weight: 500, color: "muted", anchor: "start" });
    g._max = w - 66 - 16 - lead;
    clip(g.title, g._max, `The title of ${g.kind} "${title}"`);
    g._titleX = tw => Math.max(left, -(tw + lead) / 2) + lead;
    g.title.setAttribute("x", g._titleX(len(g.title)));
    if (glyph) g.glyph = iconEl(g.top, glyph, g._titleX(len(g.title)) - lead + 7.5, 20, 15, "muted");
    Object.assign(g, { rx: 14, _title: title, _lead: lead });
    g.paint = t => [[g.box, { stroke: edge(t, 0.62) }]];
    g.size(0);
  }
  function win(x, y, title, { w = 340, h, icon: glyph } = {}) {
    const g = piece("window", x, y, L.pieces);
    frame(g, w, h, title, { glyph, min: 98 });
    g.rows = [];
    g.row = (label, { icon: ic, tone: tn, mono = false, muted = false, meta } = {}) => {
      const i = g.rows.length, rw = w - 20, r = piece("row", 0, 69 + i * 40, g.top, g), c = neutral(tn) ? null : tn;
      r.band = rect(r, rw, 36, 9, { fill: c ? wash(c) : "none" });
      let lead = -rw / 2 + 14;
      if (ic) { r.icon = iconEl(r, ic, lead + 8, 0, 16, c || (muted ? "faint" : "muted")); lead += 28; }
      else if (c) { r.dot = make("circle", { cx: lead + 4, r: 4, fill: tone(c) }, r); lead += 20; }
      r.label = textEl(r, lead, 0, label, { size: mono ? 15 : 16, anchor: "start", mono, color: muted ? "muted" : "ink" });
      if (meta) r.meta = textEl(r, rw / 2 - 14, 0, meta, { size: 13, anchor: "end", mono: true, color: "faint" });
      r._max = rw / 2 - 14 - (meta ? len(r.meta) + 14 : 0) - lead;
      clip(r.label, r._max, `A row of window "${title}"`);
      Object.assign(r, { w: rw, h: 36, rx: 9 });
      r.paint = t => [[r.band, { fill: wash(t) }], [r.icon, { stroke: tone(t) }], [r.dot, { fill: tone(t) }]];
      g.rows.push(r);
      g.size(58 + 40 * g.rows.length);
      return r;
    };
    return g;
  }
  function thread(x, y, title, { w = 360, h } = {}) {
    const g = piece("thread", x, y, L.pieces);
    frame(g, w, h, title, { glyph: "thread", min: 110 });
    let at = 54, last = null;
    g.says = [];
    g.say = (who, words, { tone: tn } = {}) => {
      const mine = who === "you", t = tn || (mine ? "you" : null);
      const named = !mine && who !== last ? 20 : 0;
      const b = piece("bubble", 0, 0, g.top, g), body = wrapped(b, 0, 0, words, w * 0.74 - 28, { size: 15, anchor: "start" });
      const bw = body.w + 26, bh = body.h + 15, bx = mine ? w / 2 - 14 - bw / 2 : -w / 2 + 14 + bw / 2, by = at + named + bh / 2;
      b.box = rect(b, bw, bh, 15, { fill: t ? wash(t, 1.15) : mix(P.surfaceHi, P.ink, dark ? 0.07 : 0.05), stroke: t ? edge(t, 0.4) : P.line, "stroke-width": 1 });
      b.insertBefore(b.box, body.el);
      body.el.setAttribute("y", -body.h / 2 + body.lh / 2);
      body.el.querySelectorAll("tspan").forEach(s => s.setAttribute("x", -bw / 2 + 13));
      if (named) textEl(b, -bw / 2 + 4, -bh / 2 - 11, who, { size: 12, anchor: "start", color: "faint", weight: 500 });
      gsap.set(b, { x: bx, y: by });
      Object.assign(b, { _x: bx, _y: by, w: bw, h: bh, rx: 15, _from: mine ? "right" : "left" });
      b.paint = t2 => [[b.box, { fill: wash(t2, 1.15), stroke: edge(t2, 0.4) }]];
      at = by + bh / 2 + 9; last = who;
      g.says.push(b);
      g.size(at + 5);
      return b;
    };
    return g;
  }
  function terminal(x, y, title, { w = 460, h } = {}) {
    const g = piece("terminal", x, y, L.pieces), bg = dark ? "#0a0d10" : "#161b20";
    container(g, w, h, 85, H => { set(g.box, { y: -H / 2, height: H }); g.shade = shade(g.in, w, H, 12, 2); });
    g.box = rect(g.in, w, 10, 12, { fill: bg, stroke: dark ? P.line : "#0d1114", "stroke-width": 1.25 });
    g.in.insertBefore(g.box, g.top);
    make("path", { d: `M${-w / 2 + 12} 0.6 h${w - 24}`, stroke: "#ffffff", "stroke-opacity": 0.1, "stroke-width": 1 }, g.top);
    for (let i = 0; i < 3; i++) make("circle", { cx: -w / 2 + 19 + i * 14, cy: 18, r: 4.2, fill: "#ffffff", "fill-opacity": 0.16 }, g.top);
    g.title = textEl(g.top, 0, 18, title, { size: 13, mono: true, color: D.faint });
    g._max = w - 124;
    clip(g.title, g._max, `The title of terminal "${title}"`);
    Object.assign(g, { rx: 12, _title: title, lines: [] });
    g.line = (str, { tone: tn, prompt = "$" } = {}) => {
      const i = g.lines.length, r = piece("line", -w / 2 + 20, 58 + i * 27, g.top, g);
      let tx = 0;
      if (prompt) { textEl(r, 0, 0, prompt, { size: 15, mono: true, anchor: "start", color: D.accent }); tx = 20; }
      const id = uid(), cp = make("clipPath", { id }, r);
      r.label = textEl(r, tx, 0, str, { size: 15, mono: true, anchor: "start", color: tn ? D[tn] || tn : prompt ? D.ink : D.muted });
      r._max = w - 40 - tx;
      clip(r.label, r._max, `A line of terminal "${title}"`);
      const tw = len(r.label);
      r.cover = make("rect", { x: tx - 2, y: -14, width: tw + 4, height: 28 }, cp);
      r.label.setAttribute("clip-path", `url(#${id})`);
      r.caret = make("rect", { x: tx, y: -9, width: 8.5, height: 18, rx: 1.5, fill: D.ink, opacity: 0 }, r);
      Object.assign(r, { w: tx + tw, h: 24, bb: { l: -6, r: tx + tw + 6, t: -13, b: 13 }, ab: { l: -6, r: tx + tw + 6, t: -13, b: 13 }, _tx: tx, _tw: tw, _chars: r.label.textContent.length });
      r.paint = t => [[r.label, { fill: D[t] || t }]];
      g.lines.push(r);
      g.size(58 + 27 * g.lines.length);
      return r;
    };
    g.paint = t => [[g.box, { stroke: edge(t, 0.62) }]];
    g.size(0);
    return g;
  }
  function doc(x, y, title, { w = 250, h, icon: glyph = "file" } = {}) {
    const g = piece("doc", x, y, L.pieces), f = 22, l = -w / 2;
    container(g, w, h, 98, H => {
      const t = -H / 2;
      g.box.setAttribute("d", `M${l + 11} ${t} H${-l - f} L${-l} ${t + f} V${-t - 11} a11 11 0 0 1 -11 11 H${l + 11} a11 11 0 0 1 -11 -11 V${t + 11} a11 11 0 0 1 11 -11 Z`);
      g.shade = shade(g.in, w, H, 11, 1);
    });
    g.box = make("path", { fill: "url(#fj-surf)", stroke: "url(#fj-rim)", "stroke-width": 1.25 }, g.in);
    g.in.insertBefore(g.box, g.top);
    make("path", { d: `M${-l - f} 0 V${f - 5} a5 5 0 0 0 5 5 H${-l} Z`, fill: mix(P.surfaceHi, P.ink, dark ? 0.1 : 0.07), stroke: mix(P.line, P.ink, 0.1), "stroke-width": 1.25, "stroke-linejoin": "round" }, g.top);
    iconEl(g.top, glyph, l + 27, 27, 17, "muted");
    g.title = textEl(g.top, l + 44, 27, title, { size: 15, weight: 500, anchor: "start", mono: /\.\w+$/.test(title) });
    g._max = w - 44 - f - 6;
    clip(g.title, g._max, `The title of doc "${title}"`);
    make("path", { d: `M${l + 16} 50 H${-l - 16}`, stroke: P.line, "stroke-width": 1 }, g.top);
    Object.assign(g, { rx: 11, _title: title, lines: [] });
    g.line = (str, { tone: tn, mono = true } = {}) => {
      const i = g.lines.length, rw = w - 24, r = piece("line", 0, 74 + i * 27, g.top, g), c = neutral(tn) ? null : tn;
      r.band = rect(r, rw, 24, 6, { fill: c ? wash(c) : "none" });
      if (str) { r.label = textEl(r, -rw / 2 + 8, 0, str, { size: 14, mono, anchor: "start", color: c || "ink" }); r._max = rw - 16; clip(r.label, r._max, `A line of doc "${title}"`); }
      else r.bar = make("rect", { x: -rw / 2 + 8, y: -3.5, width: (rw - 16) * [0.86, 0.62, 0.94, 0.48, 0.78, 0.7, 0.9, 0.56][i % 8], height: 7, rx: 3.5, fill: c ? edge(c, 0.55) : mix(P.surfaceHi, P.ink, 0.13) }, r);
      Object.assign(r, { w: rw, h: 24, rx: 6 });
      r.paint = t2 => [[r.band, { fill: wash(t2) }], [r.label, { fill: tone(t2) }], [r.bar, { fill: edge(t2, 0.55) }]];
      g.lines.push(r);
      g.size(71 + 27 * g.lines.length);
      return r;
    };
    g.paint = t2 => [[g.box, { stroke: edge(t2, 0.62) }]];
    g.size(0);
    return g;
  }
  // An area that groups pieces: a machine, an account, a message. Give it the pieces and it fits around them.
  function region(around, label, { tone: tn, pad = 26, dashed = false, icon: glyph } = {}) {
    const b = Array.isArray(around) || isPiece(around) ? union(list(around)) : bounds(around);
    const fit = Array.isArray(around) || isPiece(around), top = fit && label ? 30 : 0;
    const l = b.l - (fit ? pad : 0), r = b.r + (fit ? pad : 0), bt = b.b + (fit ? pad : 0);
    let t = b.t - (fit ? pad + top : 0);
    if (label && plan) { // a badge that will sit where the label goes: the label gets a taller header, above it
      const probe = textEl(L.regions, 0, 0, label, { size: 14, weight: 500 }), lw = len(probe) + (glyph ? 23 : 0);
      probe.remove();
      if (plan.badges.some(k => k.l < l + 26 + lw && k.r > l + 10 && k.t < t + 36 && k.b > t + 8)) t -= 30;
    }
    const w = r - l, h = bt - t, g = piece("region", (l + r) / 2, (t + bt) / 2, L.regions), c = neutral(tn) ? null : tn;
    g.box = rect(g, w, h, 22, { fill: c ? tone(c) : P.ink, "fill-opacity": c ? 0.06 : dark ? 0.035 : 0.035, stroke: c ? edge(c, 0.55) : mix(P.line, P.ink, dark ? 0 : 0.06), "stroke-width": 1.25, "stroke-dasharray": dashed ? "5 6" : null });
    if (label) {
      let lx = -w / 2 + 18;
      if (glyph) { g.icon = iconEl(g, glyph, lx + 7, -h / 2 + 22, 15, c || "muted"); lx += 23; }
      g.label = textEl(g, lx, -h / 2 + 22, label, { size: 14, weight: 500, anchor: "start", color: c || "muted" });
    }
    Object.assign(g, { w, h, rx: 22 });
    g.paint = t2 => [[g.box, { stroke: edge(t2, 0.55), fill: tone(t2), "fill-opacity": 0.06 }], [g.label, { fill: tone(t2) }], [g.icon, { stroke: tone(t2) }]];
    return g;
  }
  function badge(target, label, { tone: tn = "warn", side = "top", icon: glyph } = {}) {
    must(isPiece(target) && target, "badge", "a piece to sit beside");
    const g = chip(0, 0, label, { tone: tn, size: 13, icon: glyph }), p = anchor(target, side, 0), n = NORMAL[side];
    g._x = p.x + n[0] * (g.w / 2 + 12); g._y = p.y + n[1] * (g.h / 2 + 12);   // clear of a mark on the same corner
    gsap.set(g, { x: g._x, y: g._y });
    g.kind = "badge"; g._on = target;
    if (rec) rec.badges.push({ l: g._x - g.w / 2, r: g._x + g.w / 2, t: g._y - g.h / 2, b: g._y + g.h / 2 });
    return g;
  }
  const MARKS = { ok: ["ok", "check"], bad: ["bad", "cross"], warn: ["warn", "alert"] };
  function mark(target, kind = "bad") {
    must(isPiece(target) && target, "mark", "a piece to mark");
    const [tn, glyph] = MARKS[kind] || MARKS.bad, b = abox(target), k = target.round ? 0.7071 * (target.w / 2 - 8) : 0;
    const pill = target.kind === "chip" || target.kind === "badge";   // on a pill it hangs off the end, clear of the words
    const g = piece("mark", k ? wx(target) + k : b.r - (pill ? -3 : 6), k ? cy(target) - k : b.t + (pill ? 3 : 6), L.top);
    make("circle", { r: 12.5, fill: tone(tn), stroke: P.bg, "stroke-width": 3 }, g);
    iconEl(g, glyph, 0, 0, 15, P.bg).setAttribute("stroke-width", 3.4);
    Object.assign(g, { w: 25, h: 25, round: true, _on: target });
    return g;
  }
  function link(from, to, { from: fs, to: ts, bend = 0, dashed = false, tone: tn = "faint", label, arrow = true } = {}) {
    if (!from || !to) throw new Error("K.link(a, b) needs two ends: pieces, rows or {x, y} points.");
    const fc = isPiece(from) ? anchor(from) : from, tc = isPiece(to) ? anchor(to) : to;
    const inner = p => isPiece(p) && p._owner, A = abox(inner(from) || from), B = abox(inner(to) || to); // a row is reached through its window's side
    // Leave by the sides that face each other across the wider gap.
    const gx = Math.max(B.l - A.r, A.l - B.r), gy = Math.max(B.t - A.b, A.t - B.b);
    const horiz = inner(from) || inner(to) || (gx > 0 || gy > 0 ? gx >= gy : Math.abs(tc.x - fc.x) >= Math.abs(tc.y - fc.y));
    const sx = tc.x > fc.x ? ["right", "left"] : ["left", "right"], sy = tc.y > fc.y ? ["bottom", "top"] : ["top", "bottom"];
    // Side by side or stacked: an S between the facing sides. Diagonal: one turn, out along the wider gap.
    const turn = gx > 24 && gy > 24 && isPiece(from) && isPiece(to) && !inner(from) && !inner(to);
    fs = fs || (horiz ? sx[0] : sy[0]);
    ts = ts || (turn ? (horiz ? sy[1] : sx[1]) : horiz ? sx[1] : sy[1]);
    // A link stops short of what it joins, so its arrowhead always shows.
    const a = isPiece(from) ? anchor(from, fs, 6) : { x: from.x, y: from.y }, b = isPiece(to) ? anchor(to, ts, arrow ? 9 : 6) : { x: to.x, y: to.y };
    const na = NORMAL[fs] || [0, 0], nb = NORMAL[ts] || [0, 0];
    // Two ends that almost line up are made to: a straight link, not a kink.
    if (na[0] && nb[0] === -na[0] && Math.abs(b.y - a.y) <= 14) a.y = b.y = (a.y + b.y) / 2;
    if (na[1] && nb[1] === -na[1] && Math.abs(b.x - a.x) <= 14) a.x = b.x = (a.x + b.x) / 2;
    const dx = b.x - a.x, dy = b.y - a.y, d = Math.hypot(dx, dy) || 1, px = -dy / d * bend * d * 0.35, py = dx / d * bend * d * 0.35;
    // Facing sides make a clean S (handles of half the run); a turn makes a quarter curve.
    const facing = na[0] === -nb[0] && na[1] === -nb[1] && (na[0] ? dx * na[0] > 0 : dy * na[1] > 0);
    const run = n => Math.abs(n[0] ? dx : dy), ka = facing ? Math.max(18, run(na) / 2) : Math.max(36, run(na) * 0.6), kb = facing ? ka : Math.max(36, run(nb) * 0.6);
    const path = `M${a.x} ${a.y} C ${a.x + na[0] * ka + px} ${a.y + na[1] * ka + py}, ${b.x + nb[0] * kb + px} ${b.y + nb[1] * kb + py}, ${b.x} ${b.y}`;
    const g = make("g", { opacity: 0 }, L.links), c = tone(tn);
    g.body = make("g", {}, g);
    g.path = make("path", { d: path, fill: "none", stroke: c, "stroke-width": 2, "stroke-linecap": "round", "stroke-dasharray": dashed ? "1 8" : null }, g.body);
    const len0 = g.path.getTotalLength(), e = g.path.getPointAtLength(len0), q = g.path.getPointAtLength(Math.max(0, len0 - 8));
    g.angle = Math.atan2(e.y - q.y, e.x - q.x);
    if (arrow) g.head = make("path", { d: "M-9 -5.5 L0 0 L-9 5.5", fill: "none", stroke: c, "stroke-width": 2, "stroke-linecap": "round", "stroke-linejoin": "round", transform: `translate(${e.x} ${e.y}) rotate(${g.angle * 180 / Math.PI})` }, g);
    if (dashed) { // DrawSVG would erase the dashes, so a dashed link is revealed through a mask
      const id = uid(), m = make("mask", { id, maskUnits: "userSpaceOnUse" }, g);
      g.reveal = make("path", { d: path, fill: "none", stroke: "#fff", "stroke-width": 10, "stroke-linecap": "round" }, m);
      g.path.setAttribute("mask", `url(#${id})`);
    }
    if (label) { // beside the middle of the path, never on it
      const m = g.path.getPointAtLength(len0 / 2), m2 = g.path.getPointAtLength(len0 / 2 + 2), tx = m2.x - m.x, ty = m2.y - m.y, tl2 = Math.hypot(tx, ty) || 1;
      let nx = ty / tl2, ny = -tx / tl2; if (ny > 0 || (Math.abs(ny) < 0.3 && nx < 0)) { nx = -nx; ny = -ny; }
      const upright = Math.abs(tx) < Math.abs(ty);
      g.label = textEl(g, m.x + nx * (upright ? 14 : 18), m.y + ny * 18, label, { size: 14, color: neutral(tn) ? "muted" : tn, anchor: upright ? "start" : "middle" });
      // A curve can swing back under its own label: then the label takes the other side.
      const hits = () => { const r = g.label.getBBox(); return [...Array(41)].some((_, i) => { const q2 = g.path.getPointAtLength(len0 * i / 40); return q2.x > r.x - 4 && q2.x < r.x + r.width + 4 && q2.y > r.y - 2 && q2.y < r.y + r.height + 2; }); };
      if (hits()) {
        set(g.label, { x: m.x - nx * (upright ? 14 : 18), y: m.y - ny * 18, "text-anchor": upright ? "end" : "middle" });
        if (hits()) set(g.label, { x: m.x + nx * (upright ? 14 : 18), y: m.y + ny * 18, "text-anchor": upright ? "start" : "middle" });
      }
      Object.assign(g.label.style, { paintOrder: "stroke", stroke: P.bg, strokeWidth: "5px", strokeLinejoin: "round" });
    }
    Object.assign(g, { kind: "link", len: len0, from, to, _d: path, _tone: neutral(tn) ? null : tn, _shown: false, _op: 0 });
    g.paint = t => [[g.path, { stroke: tone(t) }], [g.head, { stroke: tone(t) }], [g.label, { fill: tone(t) }]];
    links.push(g);
    return g;
  }

  // ---------- time ----------
  // Motions place themselves on the timeline. With no `at`, each starts when everything before it has finished.
  // at: "<" with the previous motion, "<0.3" 0.3 s after its start, ">" at its end, ">-0.2" overlapping its end,
  // or a number: seconds into the scene. Background motions (flow) never hold the next one back.
  let tl, cur = 0, lastStart = 0, lastEnd = 0, known = 0, sceneStart = 0, built = 0, pending = [], lit = null, camN = 0, cam = { s: 1, x: W / 2, y: H / 2 }, bigs = [];
  function sync() { // raw tl.to() calls by the creator move the timeline's end; follow it
    const d = tl.duration();
    if (d > known + 1e-6) { const r = tl.recent(); lastStart = r ? r.startTime() : d; cur = lastEnd = d; known = d; }
  }
  function when(at) {
    sync();
    if (at == null) return cur;
    if (typeof at === "number") return sceneStart + Math.max(0, at);
    const m = /^([<>])\s*=?\s*([+-]?\d*\.?\d+)?$/.exec(String(at).trim());
    if (!m) throw new Error(`at: "${at}" isn't a position. Use "<", ">", "<0.3", ">-0.2", or seconds into the scene.`);
    return Math.max(0, (m[1] === "<" ? lastStart : lastEnd) + (+m[2] || 0));
  }
  function put(anim, at, background = false) {
    const t = when(at);
    tl.add(anim, t);
    lastStart = t;
    if (!background) { lastEnd = t + anim.duration(); cur = Math.max(cur, lastEnd); }
    known = tl.duration();
    return anim;
  }
  const list = x => (Array.isArray(x) ? x.flat() : [x]).filter(Boolean);
  const must = (p, fn, what = "a piece") => { if (!p || !(p.kind || p.nodeType)) throw new Error(`K.${fn}() needs ${what}${Array.isArray(p) ? ", not a list" : ""}.`); return p; };
  // What a piece shows at now: its creator's opacity, unless the spotlight or the camera holds it back.
  const eff = e => e._cut || e._under ? 0 : (e._op ?? 1) * (e._veil || 1);
  function showing(el) {
    Object.assign(el, { _shown: true, _op: 1 });
    pin(el);
    if (rec && rec.cam && tracked(el) && !rec.cam.seen.has(el)) rec.cam.seen.set(el, { before: false, rect: bounds(el) });
  }
  const gone = el => Object.assign(el, { _shown: false, _op: 0 });

  function scene(i) {
    sync();
    if (i > 0) {
      const end = Math.max(cur, tl.duration());
      tl.to({}, { duration: HOLD }, end);
      settle(end + HOLD, true);
      cur = lastStart = lastEnd = known = end + HOLD;
    }
    tl.addLabel("s" + i, cur);
    sceneStart = cur;
    built = Math.max(built, i + 1);
  }
  // What lasts "for the rest of the scene" is cut to length once the scene's end is known.
  function settle(end, fade) {
    pending = pending.filter(f => {
      if (f.keep && fade) return true;
      const stop = Math.min(end, f.stop ?? end), dur = Math.max(0.3, stop - f.start);
      tl.fromTo(f.dots, { strokeDashoffset: 0 }, { strokeDashoffset: -f.speed * dur, duration: dur, ease: "none", immediateRender: false }, f.start);
      tl.to(f.dots, { opacity: 1, duration: 0.35, ease: "power1.out" }, f.start);
      if (fade || f.stop != null) tl.to(f.dots, { opacity: 0, duration: 0.35, ease: "power1.in" }, stop - 0.38);
      return false;
    });
    if (fade) { const t = gsap.timeline(); if (lit) lift(t, 0.38); uncover(null, t, 0.38); bigs = bigs.filter(b => b._shown); if (bigs.length) clearGround(t, 0.38); tl.add(t, end - 0.4); }
    known = tl.duration();
  }

  // ---------- motion ----------
  const SHIFT = { below: [0, 18], above: [0, -18], left: [-26, 0], right: [26, 0], pop: [0, 0], fade: [0, 0] };
  function appear(els, { from = "below", dur = 0.7, stagger = 0.07, at } = {}) {
    const t = gsap.timeline();
    list(els).forEach((el, i) => {
      const o = i * stagger;
      if (el.kind === "link") { t.add(drawing(el, 0.8), o); return; }
      must(el, "appear");
      showing(el);
      if (el.enter) { t.add(el.enter(dur), o); if (el.lens) { bigs.push(el); clearGround(t, 0.4); } return; }
      const x = el._x ?? +gsap.getProperty(el, "x"), y = el._y ?? +gsap.getProperty(el, "y");
      const f = el._from && from === "below" ? el._from : from, s = SHIFT[f] || SHIFT.below;
      t.fromTo(el, { opacity: 0, x: x + s[0], y: y + s[1], scale: f === "pop" ? 0.86 : f === "fade" ? 1 : 0.975 },
               { opacity: 1, x, y, scale: 1, duration: f === "pop" ? dur * 0.8 : dur, ease: EASE.out }, o);
      (el.extras || []).forEach((x2, j) => // secondary motion: the icon settles a beat after its surface
        t.fromTo(x2, { opacity: 0, scale: (x2._s || 1) * 0.7 }, { opacity: 1, scale: x2._s || 1, duration: dur * 0.9, ease: EASE.out }, o + 0.1 + j * 0.08));
    });
    return put(t, at);
  }
  function hide(els, { dur = 0.4, at } = {}) {
    const t = gsap.timeline();
    list(els).forEach(e => { gone(e); t.to([e, e.lens && e.ground].filter(Boolean), { opacity: 0, duration: dur, ease: "power2.out" }, 0); if (e.lens) uncover(e, t, dur); });
    return put(t, at);
  }
  // Big type clears its ground: the pieces that lie dimmed behind it step out while it shows, instead of showing
  // through in halves. Lit pieces stay, and the preview says so.
  function clearGround(t, dur) {
    const v = view(cam.s, cam.x, cam.y);
    bigs.filter(b => b._shown).forEach(b => {
      const g = { l: (b._x + b.bb.l - 16) / cam.s + v.l, r: (b._x + b.bb.r + 16) / cam.s + v.l, t: (b._y + b.bb.t - 12) / cam.s + v.t, b: (b._y + b.bb.b + 12) / cam.s + v.t };
      const inside = pt => pt.x > g.l && pt.x < g.r && pt.y > g.t && pt.y < g.b;
      [...pieces.filter(tracked), ...links].forEach(e => {
        if (!e._shown || e._under || e._cut || eff(e) > 0.5) return;
        if (e.kind === "link") { if (![...Array(25)].some((_, i) => inside(e.path.getPointAtLength(e.len * i / 24)))) return; }
        else { const r = bounds(e); if (r.r < g.l || r.l > g.r || r.b < g.t || r.t > g.b) return; }
        e._under = b;
        t.to(e, { opacity: 0, duration: dur, ease: "power2.inOut" }, 0);
      });
    });
  }
  function uncover(b, t, dur) {
    [...pieces, ...links].forEach(e => { if (e._under && (!b || e._under === b)) { e._under = null; if (e._shown) t.to(e, { opacity: eff(e), duration: dur, ease: "power2.inOut" }, 0); } });
    if (b) bigs = bigs.filter(x => x !== b);
  }
  function dim(els, { to = 0.3, dur = 0.5, at } = {}) {
    const t = gsap.timeline();
    list(els).forEach(e => { e._op = to; t.to(e, { opacity: eff(e), duration: dur, ease: "power2.out" }, 0); });
    return put(t, at);
  }
  function drawing(lk, dur) {
    const t = gsap.timeline();
    Object.assign(lk, { _shown: true, _op: 1 });
    t.set(lk, { opacity: 1 });
    t.fromTo(lk.reveal || lk.path, { drawSVG: "0%" }, { drawSVG: "100%", duration: dur, ease: EASE.inOut, immediateRender: true });
    if (lk.head) t.fromTo(lk.head, { opacity: 0 }, { opacity: 1, duration: 0.2, immediateRender: true }, dur - 0.18);
    if (lk.label && !mute(lk)) t.fromTo(lk.label, { opacity: 0 }, { opacity: 1, duration: 0.35, immediateRender: true }, dur * 0.5);
    else if (lk.label) t.set(lk.label, { opacity: 0 }, 0);
    return t;
  }
  function draw(lks, { dur = 0.8, stagger = 0.12, at } = {}) {
    const t = gsap.timeline();
    list(lks).forEach((lk, i) => t.add(drawing(must(lk.kind === "link" && lk, "draw", "a link"), dur), i * stagger));
    return put(t, at);
  }
  // A piece's own outline, in its own coordinates: for the ring that leaves it and the light behind it.
  const shape = el => { const b = !el.round && el.bb && el.kind !== "big" ? el.bb : { l: -el.w / 2, r: el.w / 2, t: -el.h / 2, b: el.h / 2 }; return { x: (b.l + b.r) / 2, y: (b.t + b.b) / 2, w: b.r - b.l, h: b.b - b.t }; };
  // A ring leaves the piece's outline and fades: something landed here, or look here.
  function ring(target, tn, t, at) {
    if (!isPiece(target)) return;
    const c = tone(tn), s = shape(target), x = wx(target) + s.x, y = cy(target) + s.y;
    const r = target.round ? make("circle", { r: s.w / 2, fill: "none", stroke: c, "stroke-width": 2, opacity: 0 }, target.lens ? L.lens : L.fx)
                           : rect(target.lens ? L.lens : L.fx, s.w, s.h, target.rx ?? 12, { fill: "none", stroke: c, "stroke-width": 2, opacity: 0 });
    gsap.set(r, { x, y, svgOrigin: "0 0" });
    const k = 1 + 26 / Math.max(s.w, 60), ky = 1 + 26 / Math.max(s.h, 40);
    t.fromTo(r, { opacity: 0.75, scaleX: 1, scaleY: 1 }, { opacity: 0, scaleX: k, scaleY: target.round ? k : ky, duration: 0.75, ease: "power2.out", immediateRender: false }, at);
  }
  // The piece rides the link. By default it lands: it is taken in by the piece at the link's end, which answers with
  // a ring. land: "dock" parks it just before the arrowhead instead, so a parked chip never covers one.
  function travel(el, lk, { dur = 1.1, land, tone: tn, at } = {}) {
    must(el, "travel"); must(lk && lk.kind === "link" && lk, "travel", "a link to ride");
    const target = isPiece(lk.to) ? lk.to : null, t = gsap.timeline();
    land = land || (target ? "absorb" : "dock");
    const reach = Math.abs(Math.cos(lk.angle)) * el.w / 2 + Math.abs(Math.sin(lk.angle)) * el.h / 2;
    const stop = land === "dock" ? Math.max(0.2, (lk.len - reach - (lk.head ? 17 : 5)) / lk.len) : 1;
    if (!el._shown) t.fromTo(el, { opacity: 0, scale: 0.8 }, { opacity: 1, scale: 1, duration: 0.3, ease: "power2.out" }, 0);
    t.to(el, { motionPath: { path: lk.path, start: 0, end: stop }, duration: dur, ease: EASE.inOut, immediateRender: false }, 0);
    if (land === "dock") {
      const p = lk.path.getPointAtLength(lk.len * stop);
      Object.assign(el, { _x: p.x, _y: p.y });
      showing(el);
    } else {
      t.to(el, { opacity: 0, scale: 0.72, duration: 0.24, ease: "power2.in" }, dur - 0.22);
      if (target) ring(target, tn || el._tone || lk._tone || "accent", t, dur - 0.1);
      gone(el);
    }
    return put(t, at);
  }
  // Ongoing traffic: beads stream along the link for the rest of the scene (or `dur` seconds, or the whole film with keep).
  function flow(lks, { tone: tn, dur, speed = 110, keep = false, at } = {}) {
    const start = when(at);
    list(lks).forEach(lk => {
      must(lk.kind === "link" && lk, "flow", "a link");
      const dots = make("path", { d: lk._d, fill: "none", stroke: tone(tn || lk._tone || "accent"), "stroke-width": 5, "stroke-linecap": "round", "stroke-dasharray": "0.1 38", opacity: 0 }, lk.body);
      pending.push({ dots, lk, start, speed, keep, stop: dur ? start + dur : null });
    });
    lastStart = start;
  }
  // The link breaks: a gap opens in it and it turns red. Its traffic stops.
  function cut(lk, { tone: tn = "bad", at } = {}) {
    must(lk && lk.kind === "link" && lk, "cut", "a link");
    // The break is in the middle, or past the middle when the link's label is there.
    const start = when(at), m = lk.path.getPointAtLength(lk.len * (lk.label && lk.len > 150 ? 0.74 : 0.5)), id = uid(), t = gsap.timeline();
    pending.forEach(f => { if (f.lk === lk && f.stop == null) f.stop = start + 0.3; });
    const mask = make("mask", { id, maskUnits: "userSpaceOnUse", x: -W, y: -H, width: W * 3, height: H * 3 }, lk);
    make("rect", { x: -W, y: -H, width: W * 3, height: H * 3, fill: "#fff" }, mask);
    const gap = make("circle", { cx: m.x, cy: m.y, r: 0, fill: "#000" }, mask);
    lk.body.setAttribute("mask", `url(#${id})`);
    const x = make("path", { d: "M-5 -5 L5 5 M5 -5 L-5 5", stroke: tone(tn), "stroke-width": 2.25, "stroke-linecap": "round", opacity: 0 }, lk);
    gsap.set(x, { x: m.x, y: m.y, svgOrigin: "0 0" });
    t.to(gap, { attr: { r: 15 }, duration: 0.45, ease: EASE.out }, 0)
     .fromTo(x, { opacity: 0, scale: 0.4, rotation: -45 }, { opacity: 1, scale: 1, rotation: 0, duration: 0.45, ease: EASE.out, immediateRender: false }, 0.05)
     .to([lk.path, lk.head].filter(Boolean), { stroke: tone(tn), duration: 0.4 }, 0)
     .to([lk.body, lk.head].filter(Boolean), { opacity: 0.5, duration: 0.5 }, 0.1);
    return put(t, at);
  }
  function moveTo(el, x, y, { dur = 0.9, at } = {}) {
    must(el, "moveTo");
    if (typeof x !== "number" || typeof y !== "number") throw new Error("K.moveTo(piece, x, y) needs numbers. To park a piece beside another, use K.dock(piece, target).");
    Object.assign(el, { _x: x, _y: y });
    return put(gsap.to(el, { x, y, duration: dur, ease: EASE.inOut }), at);
  }
  // Park a piece beside another, outside its edge, so neither covers the other (or a link's arrowhead).
  function dock(el, target, { side, gap = 10, dur = 0.9, at } = {}) {
    must(el, "dock"); must(isPiece(target) && target, "dock", "a piece to dock beside");
    if (!side) { const dx = wx(el) - wx(target), dy = wy(el) - wy(target); side = Math.abs(dx) * target.h >= Math.abs(dy) * target.w ? (dx < 0 ? "left" : "right") : (dy < 0 ? "top" : "bottom"); }
    const p = anchor(target, side, gap), n = NORMAL[side];
    return moveTo(el, p.x + n[0] * el.w / 2 - (wx(el) - el._x), p.y + n[1] * el.h / 2 - (wy(el) - el._y), { dur, at });
  }

  // The camera frames pieces (or a point with a zoom); with nothing, the whole stage. The backdrop drifts behind.
  // It never leaves a piece half in the frame: it widens a little to take the piece in whole, and when that would cost
  // the push-in, the piece fades out for as long as the framing holds.
  const EDGE = 16, AIR = 34, GIVE = 0.78;   // px a piece keeps from the frame's edge; px it gets when taken in; how much zoom may be given up
  const view = (s, x, y) => ({ l: x - W / 2 / s, r: x + W / 2 / s, t: y - H / 2 / s, b: y + H / 2 / s });
  function framed(r, v, s) { // "in"; "cut" by the frame's edge; "near" enough outside it that its shadow shows; "out"
    const m = EDGE / s, o = 3 / s, n = 30;
    if (r.r < v.l - n || r.l > v.r + n || r.b < v.t - n || r.t > v.b + n) return "out";
    if (r.r < v.l + o || r.l > v.r - o || r.b < v.t + o || r.t > v.b - o) return "near";
    return r.l >= v.l + m && r.r <= v.r - m && r.t >= v.t + m && r.b <= v.b - m ? "in" : "cut";
  }
  function mute(lk) { const b = lk.label.getBBox(); lk._mute = framed({ l: b.x, t: b.y, r: b.x + b.width, b: b.y + b.height }, view(cam.s, cam.x, cam.y), cam.s) === "cut"; return lk._mute; }
  function closeCam() { // first pass: what stood on stage while the last framing held
    if (!rec || !rec.cam) return;
    rec.cam.seen.forEach((v, p) => { if (p._shown) rec.cam.out.push({ i: p._i, before: v.before, ...join(v.rect, bounds(p)) }); });
    rec.cam = null;
  }
  function camera(target, { zoom, pad = 90, dur = 1.3, at } = {}) {
    let s = 1, cx = W / 2, cy0 = H / 2;
    pieces.forEach(p => { if (p._shown) pin(p); });
    if (target) {
      const b = Array.isArray(target) || target.kind ? union(list(target)) : { l: target.x, r: target.x, t: target.y, b: target.y };
      cx = (b.l + b.r) / 2; cy0 = (b.t + b.b) / 2;
      s = zoom || Math.min(W / (b.r - b.l + pad * 2), H / (b.b - b.t + pad * 2), 2.6);
      s = Math.max(0.4, Math.min(4, s));
    }
    const k = camN++, t = gsap.timeline(), fade = new Set();
    if (rec) {
      closeCam();
      rec.cam = { seen: new Map(), out: [] };
      rec.cams[k] = rec.cam.out;
      pieces.forEach(p => { if (p._shown && tracked(p)) rec.cam.seen.set(p, { before: true, rect: bounds(p) }); });
    } else {
      const there = plan && plan.cams[k] || [], floor = Math.max(0.4, s * GIVE);
      for (let n = 0; n < 12; n++) { // take in whole the cut pieces that cost the least zoom, while the cost is small
        const v = view(s, cx, cy0);
        let best = null;
        there.filter(r => framed(r, v, s) === "cut").forEach(r => {
          const a = AIR / s, u = join(v, { l: r.l - a, r: r.r + a, t: r.t - a, b: r.b + a }), s2 = Math.min(W / (u.r - u.l), H / (u.b - u.t));
          if (s2 >= floor && (!best || s2 > best.s)) best = { s: s2, x: (u.l + u.r) / 2, y: (u.t + u.b) / 2 };
        });
        if (!best) break;
        s = best.s; cx = best.x; cy0 = best.y;
      }
      const v = view(s, cx, cy0);
      there.forEach(r => { if (r.before && pieces[r.i] && /cut|near/.test(framed(r, v, s))) fade.add(pieces[r.i]); });
      pieces.forEach(p => { if (p._on && fade.has(p._on)) fade.add(p); });
    }
    // Pieces the last framing faded come back; the ones this framing cuts go. Their links go with them.
    const host = x => isPiece(x) ? x._owner || x : null, d = Math.max(0.01, dur * 0.55);
    [...pieces, ...links].forEach(e => {
      const now = e.kind === "link" ? fade.has(host(e.from)) || fade.has(host(e.to)) : fade.has(e);
      if (!!e._cut === now) return;
      e._cut = now;
      if (e._shown) t.to(e, { opacity: eff(e), duration: d, ease: "power2.inOut" }, now ? 0 : Math.max(0, dur - d));
    });
    cam = { s, x: cx, y: cy0 };
    links.forEach(lk => { // a label the frame would cut steps out with it
      if (!lk.label || !lk._shown) return;
      const hid = !!lk._mute; mute(lk);
      if (hid !== lk._mute) t.to(lk.label, { opacity: lk._mute ? 0 : 1, duration: d, ease: "power2.inOut" }, lk._mute ? 0 : Math.max(0, dur - d));
    });
    const to = { x: W / 2 - cx * s, y: H / 2 - cy0 * s, scale: s }, far = { x: to.x * 0.3 - (s - 1) * 0.12 * W / 2, y: to.y * 0.3 - (s - 1) * 0.12 * H / 2, scale: 1 + (s - 1) * 0.12 };
    const start = when(at);
    if (dur <= 0) { t.set(L.world, to, 0).set(L.grid, far, 0); if (start === 0) { gsap.set(L.world, to); gsap.set(L.grid, far); } }
    else t.to(L.world, { ...to, duration: dur, ease: EASE.cam }, 0).to(L.grid, { ...far, duration: dur, ease: EASE.cam }, 0);
    return put(t, at);
  }
  // Light these pieces: everything else falls back, and comes back by itself when the scene ends (K.spotlight(null)
  // sooner). A lit piece keeps its badges and marks, and the links between lit pieces stay. Light a row and the rest
  // of its window falls back around it. No veil is drawn: the others only fade, which is cheap and keeps the backdrop.
  function lift(t, dur) {
    lit.forEach(e => { e._veil = 0; e._under = null; if (e._shown) t.to(e, { opacity: eff(e), duration: dur, ease: "power2.inOut" }, 0); });
    lit = null;
  }
  function spotlight(els, { dim: k, dur = 0.6, at } = {}) {
    const t = gsap.timeline();
    if (lit) lift(t, dur);
    if (els != null) {
      const keep = new Set(list(els)), held = x => isPiece(x) && (keep.has(x) || keep.has(x._owner));
      k = k ?? (keep.size ? 0.16 : 0.07);                         // with nothing lit, the stage goes dark for big type
      pieces.forEach(p => { if (p._on && held(p._on)) keep.add(p); });
      links.forEach(l => { if (held(l.from) && held(l.to)) keep.add(l); });
      const homes = new Set([...keep].map(e => e._owner).filter(Boolean));
      lit = [...pieces, ...links].filter(e => {
        if (e.lens || !e._shown || keep.has(e)) return false;
        return e._owner ? homes.has(e._owner) && !keep.has(e._owner) : !homes.has(e);
      });
      lit.forEach(e => { e._veil = k; t.to(e, { opacity: eff(e), duration: dur, ease: "power2.inOut" }, 0); });
      clearGround(t, dur);
    }
    return put(t, at);
  }
  // A soft light behind the subject, in a tone, and its outline takes the tone. It stays.
  // A row, line or bubble is lit in place with a crisp outline, so nothing spills over its neighbours.
  function glow(els, { tone: tn = "accent", at } = {}) {
    const t = gsap.timeline();
    list(els).forEach(el => {
      must(el, "glow");
      if (isPiece(el)) {
        const c = tone(tn), s = shape(el);
        const light = el._owner ? make("rect", { x: s.x - s.w / 2, y: s.y - s.h / 2, width: s.w, height: s.h, rx: el.rx ?? 8, fill: "none", stroke: c, "stroke-width": 1.5, opacity: 0 }, el)
                                : halo(el.in || el, s.w, s.h, el.round ? s.w / 2 : el.rx ?? 12, c, el.shade);
        if (!el._owner) gsap.set(light, { x: s.x, y: s.y });
        t.to(light, { opacity: el._owner ? 0.85 : 1, duration: 0.7, ease: "power2.out" }, 0);
        ring(el, tn, t, 0.05);
      }
      paintTo(t, el, tn, 0.5, 0);
    });
    return put(t, at);
  }
  function pulse(els, { tone: tn, at } = {}) {
    const t = gsap.timeline();
    list(els).forEach((el, i) => {
      must(el, "pulse");
      if (!isPiece(el)) return;
      ring(el, tn || el._tone || "accent", t, i * 0.08);
      t.to(el, { scale: 1.035, duration: 0.16, ease: "power2.out" }, i * 0.08).to(el, { scale: 1, duration: 0.5, ease: "power3.out" }, i * 0.08 + 0.16);
    });
    return put(t, at);
  }
  function paintTo(t, el, tn, dur, pos) {
    const jobs = el.paint ? el.paint(tn) : [[el, { stroke: tone(tn) }]];
    jobs.forEach(([target, props]) => { if (target) t.to(target, { ...props, duration: dur, ease: "power1.inOut" }, pos); });
    el._tone = tn;
  }
  function recolor(els, tn, { dur = 0.45, at } = {}) {
    const t = gsap.timeline();
    list(els).forEach(el => paintTo(t, must(el, "recolor"), tn, dur, 0));
    return put(t, at);
  }
  // First pass: a card that will hold wider words is built wide enough for them.
  function room(el, tw) {
    const need = Math.ceil(el._lead + tw + 24);
    if (rec && el.kind === "card" && need > el.w) rec.reserve[el._i] = Math.max(rec.reserve[el._i] || 0, need);
  }
  function relabel(el, str, { dur = 0.4, color, at } = {}) {
    must(el, "relabel");
    // crossfade to a twin so seeking backwards shows the old words again
    const key = el.label ? "label" : el.text ? "text" : el.title ? "title" : null, old = el[key];
    if (!old) throw new Error("K.relabel() needs a piece with words.");
    const twin = old.cloneNode(true), t = gsap.timeline();
    twin.textContent = str;
    twin.style.opacity = 0;
    twin.removeAttribute("clip-path");
    old.after(twin);
    el[key] = twin;
    if (color) twin.setAttribute("fill", tone(color)); // else the twin wears the tone the piece was last recoloured to
    else if (el._tone && el.paint) el.paint(el._tone).forEach(([target, props]) => { if (target === twin && props.fill) twin.setAttribute("fill", props.fill); });
    if (el._max) clip(twin, el._max, `The new words of ${el.kind} "${old.textContent.slice(0, 24)}"`);
    t.to(old, { opacity: 0, duration: dur }, 0).to(twin, { opacity: 1, duration: dur }, 0);
    room(el, len(twin));
    if (key === "title" && el._titleX) { // a window's title stays centred
      const x = el._titleX(len(twin));
      twin.setAttribute("x", x);
      if (el.glyph) t.to(el.glyph, { x: x - el._lead + 7.5, duration: dur, ease: EASE.inOut }, 0);
    }
    if (el.kind === "actor") el.bb = { ...el.bb, l: Math.min(el.bb.l, -len(twin) / 2), r: Math.max(el.bb.r, len(twin) / 2) };
    if (el.fit) { // a chip grows or shrinks to its new words
      const f = el.fit(len(twin));
      twin.setAttribute("x", f.labelX);
      t.to(el.box, { attr: { x: -f.w / 2, width: f.w }, duration: dur, ease: EASE.inOut }, 0);
      el.shade.fit(f.w, t, dur);
      if (el.icon) t.to(el.icon, { x: f.iconX, duration: dur, ease: EASE.inOut }, 0);
      el.w = f.w;
    }
    return put(t, at);
  }
  // A number that ticks: K.count(piece, 0, 74, { suffix: " min" }). Works on text, big type, chips and cards.
  function count(el, from, to, { prefix = "", suffix = "", decimals = 0, dur = 1.6, at } = {}) {
    must(el, "count");
    const words = el.label || el.text;
    if (!words) throw new Error("K.count() needs a piece with words.");
    const fmt = v => (+v).toLocaleString("en-US", { minimumFractionDigits: decimals, maximumFractionDigits: decimals });
    words.textContent = "";
    words.style.fontVariantNumeric = "tabular-nums";
    if (prefix) make("tspan", {}, words).textContent = prefix;
    const num = make("tspan", {}, words);
    if (suffix) make("tspan", {}, words).textContent = suffix;
    num.textContent = fmt(Math.abs(to) > Math.abs(from) ? to : from);   // sized for its widest value
    if (el.resize) el.resize();
    if (el.refit) el.refit();
    room(el, len(words));
    num.textContent = fmt(from);
    const step = 10 ** -decimals;
    return put(gsap.fromTo(num, { textContent: from }, { textContent: to, duration: dur, ease: "power2.out", snap: { textContent: step }, modifiers: { textContent: v => fmt(parseFloat(v)) } }), at);
  }
  // A terminal line types itself out, caret and all.
  function type(lines, { dur, stagger = 0.15, at } = {}) {
    const t = gsap.timeline();
    let o = 0;
    list(lines).forEach(r => {
      if (!r.cover) throw new Error("K.type() types a terminal's line: const l = term.line(\"…\"); K.type(l).");
      const d = dur || Math.max(0.45, Math.min(1.5, r._chars * 0.034)), steps = `steps(${Math.max(1, r._chars)})`;
      showing(r);
      t.set(r, { opacity: 1 }, o).set(r.caret, { opacity: 0.9 }, o)
       .fromTo(r.cover, { attr: { width: 0 } }, { attr: { width: r._tw + 4 }, duration: d, ease: steps }, o)
       .fromTo(r.caret, { x: 0 }, { x: r._tw + 3, duration: d, ease: steps }, o)
       .to(r.caret, { opacity: 0, duration: 0.01 }, o + d + 0.25);
      o += d + stagger;
    });
    return put(t, at);
  }

  const K = { W, H, tone, gsap, get world() { return L.world; }, get lens() { return L.lens; },
              card, actor, chip, icon, text, big, window: win, thread, terminal, doc, region, link, badge, mark,
              appear, hide, dim, draw, travel, flow, cut, moveTo, dock, camera, spotlight, glow, pulse, recolor, relabel, count, type };

  // ---------- checks ----------
  // At each scene's held frame, on the rendered picture: nothing sits on an arrowhead, nothing is cut by the frame,
  // big type covers nothing that is lit, and no two pieces lie on each other.
  function lint(ends) {
    const seen = el => { let o = 1; for (let n = el; n && n !== stage; n = n.parentNode) o *= +gsap.getProperty(n, "opacity"); return o; };
    const end = p => (isPiece(p) ? label(p) : "a point");
    const box = p => [p.box, p.label, p.sub, p.text, p.kind === "icon" && p.icon].filter(Boolean).map(e => e.getBoundingClientRect())
      .reduce((a, r) => ({ l: Math.min(a.l, r.left), t: Math.min(a.t, r.top), r: Math.max(a.r, r.right), b: Math.max(a.b, r.bottom) }), { l: 1e9, t: 1e9, r: -1e9, b: -1e9 });
    const over = (a, b) => Math.max(0, Math.min(a.r, b.r) - Math.max(a.l, b.l)) * Math.max(0, Math.min(a.b, b.b) - Math.max(a.t, b.t));
    const area = a => (a.r - a.l) * (a.b - a.t);
    const SOLID = ["card", "actor", "window", "thread", "terminal", "doc", "chip", "badge", "text", "icon"];
    ends.forEach((time, i) => {
      tl.time(time);
      const S = stage.getBoundingClientRect(), st = { l: S.left, t: S.top, r: S.right, b: S.bottom }, u = S.width / W;
      links.filter(lk => lk.head && seen(lk) > 0.3 && +gsap.getProperty(lk.head, "opacity") > 0.3).forEach(lk => {
        const h = lk.head.getBoundingClientRect(), hx = h.x + h.width / 2, hy = h.y + h.height / 2;
        pieces.filter(p => ["chip", "badge", "card", "actor", "text", "icon", "mark"].includes(p.kind) && p !== lk.to && p !== lk.from && p._on !== lk.to && seen(p) > 0.3).forEach(p => {
          const r = (p.box || p).getBoundingClientRect();
          if (hx > r.left - 2 && hx < r.right + 2 && hy > r.top - 2 && hy < r.bottom + 2)
            note(`Scene ${i}: ${label(p)} sits on the arrowhead of the link ${end(lk.from)} → ${end(lk.to)}. Let K.travel() land it, or K.dock() it beside the target.`);
        });
      });
      const up = pieces.filter(p => (tracked(p) || p.lens) && seen(p) > 0.45).map(p => ({ p, r: box(p) }));
      up.forEach(({ p, r }) => {
        if (over(r, st) < 4) return;
        const sides = [r.l < st.l - 1.5 && "left", r.r > st.r + 1.5 && "right", r.t < st.t - 1.5 && "top", r.b > st.b + 1.5 && "bottom"].filter(Boolean);
        if (sides.length) note(p.lens ? `Scene ${i}: the big type "${p.text.textContent}" runs off the ${sides.join(" and ")} of the stage. Move it in, or make it smaller with size.`
                                      : `Scene ${i}: ${label(p)} is cut by the ${sides.join(" and ")} edge of the frame. Add it to K.camera([...]), or move it.`);
      });
      up.filter(a => a.p.lens).forEach(a => up.filter(b => !b.p.lens && over(a.r, b.r) > 30 * u * u).forEach(b =>
        note(`Scene ${i}: the big type "${a.p.text.textContent}" covers ${label(b.p)}, which is lit. Move the big type into clear space, or leave that piece out of the spotlight.`)));
      const solid = up.filter(a => SOLID.includes(a.p.kind));
      solid.forEach((a, n) => solid.slice(n + 1).forEach(b => {
        if (a.p._on === b.p || b.p._on === a.p || (a.p._on && a.p._on === b.p._on)) return;
        if (over(a.r, b.r) > 0.12 * Math.min(area(a.r), area(b.r))) note(`Scene ${i}: ${label(a.p)} and ${label(b.p)} overlap. Move one, or K.hide() the one that is done.`);
      }));
    });
  }

  // ---------- player ----------
  const $ = s => document.querySelector(s);
  const query = new URLSearchParams(location.search);
  if (/^(dark|light)$/.test(query.get("theme") || "")) root.dataset.theme = query.get("theme");
  // `?embed=1`: the page is the stage alone in Fjord's Show me panel (player.css `.embed`). Fjord draws the answer, caption,
  // controls and Sources natively, drives the film through `window.__player`, and hears its state through the
  // `fjordPlayer` message handler, the page's only way out. The page's own keys are off: Fjord handles them.
  const embed = query.has("embed");
  if (embed) root.classList.add("embed");
  const scheme = matchMedia("(prefers-color-scheme: dark)");
  const themeNow = () => (/^(dark|light)$/.test(root.dataset.theme || "") ? root.dataset.theme : scheme.matches ? "dark" : "light");
  // Errors stop the film and show under it. Layout notes are for the creator's preview only: they stay in the page, unseen.
  const crashes = [];
  let failures = [];
  // WebKit's `stack` leaves out the message (Chromium's starts with it): keep both, once.
  const said = e => !e ? "Unknown error" : typeof e !== "object" ? String(e)
    : e.stack && e.message && e.stack.includes(e.message) ? e.stack
    : [(e.name ? e.name + ": " : "") + (e.message || String(e)), e.stack].filter(Boolean).join("\n");
  function fail(e, build) { (build ? failures : crashes).push(said(e)); report(); }
  function report() {
    $("#errors").textContent = [...crashes, ...failures, ...notes].join("\n");
    document.body.classList.toggle("broken", crashes.length + failures.length > 0);
  }
  // A script from another file hides its error ("Script error."): name the file that threw it.
  addEventListener("error", e => fail(e.error || (e.message === "Script error." ? "explainer.js threw an error before explainer() ran (" + e.message + ")" : e.message)));
  addEventListener("unhandledrejection", e => fail(e.reason));
  // A host waits for `data-ready`: "1" once the film is built, "error" when it never will be (explainer.js didn't parse,
  // threw before calling explainer(), never called it, or explainer() itself threw), so the wait always ends.
  const unready = () => { if (!document.body.dataset.ready) document.body.dataset.ready = "error"; };
  addEventListener("error", () => { if (!spec) unready(); });
  addEventListener("load", () => { if (!spec) { fail(new Error("explainer.js never called explainer({...})")); unready(); } });

  let spec, theme, starts = [], ends = [], shown = -1;
  const sceneAt = t => { let k = 0; starts.forEach((s, i) => { if (t >= s - 0.001) k = i; }); return k; };
  const clock = t => Math.floor(t / 60) + ":" + String(Math.floor(t % 60)).padStart(2, "0");
  const GLYPH = { Play: "M8 5.5v13l11-6.5z", Pause: "M7 5h3.6v14H7zM13.4 5H17v14h-3.6z", Replay: "M12 5a7 7 0 1 1-6.6 4.7M12 5 8.5 2M12 5 8.5 8" };
  function show() {
    const t = tl.time(), i = sceneAt(t), n = spec.scenes.length, segs = $(".segs");
    if (i !== shown) {
      shown = i; const s = spec.scenes[i] || {};
      $(".caption").textContent = s.caption || "";
      $(".source").textContent = s.source ? "“" + s.source + "”" : "";
      $(".src").hidden = !spec.scenes.some(x => x.source);
    }
    [...segs.children].forEach((d, k) => { const a = starts[k], b = k + 1 < n ? starts[k + 1] : tl.duration(); d.firstChild.style.transform = `scaleX(${Math.max(0, Math.min(1, (t - a) / (b - a)))})`; d.classList.toggle("on", k === i); });
    const state = tl.paused() ? (t >= tl.duration() - 0.01 ? "Replay" : "Play") : "Pause", pp = $(".pp");
    if (pp.dataset.state !== state) {
      pp.dataset.state = state; pp.setAttribute("aria-label", state); pp.title = state;
      pp.innerHTML = `<svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true"><path d="${GLYPH[state]}" ${state === "Replay" ? 'fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"' : 'fill="currentColor"'}/></svg>`;
    }
    $(".time").textContent = clock(t) + " / " + clock(tl.duration());
    post();
  }
  // The film's state for Fjord: on every change of scene or of playing, after a jump, and once a second while it plays
  // (Fjord runs the clock between). Numbers and short strings only: `{t, scene, playing, total, scenes: [{caption, source,
  // start}]}`.
  let sent = null;
  function post(force) {
    const h = embed && window.webkit && window.webkit.messageHandlers && window.webkit.messageHandlers.fjordPlayer;
    if (!h || !tl || !spec) return;
    const t = tl.time(), total = tl.duration(), playing = !tl.paused() && t < total - 0.01, scene = sceneAt(t);
    if (!force && sent && sent.scene === scene && sent.playing === playing && sent.total === total
        && (playing ? Math.abs(t - sent.t) < 1 : Math.abs(t - sent.t) < 0.001)) return;
    sent = { t, scene, playing, total };
    const cut = (v, n) => (typeof v === "string" ? v : "").slice(0, n);
    h.postMessage({ t, scene, playing, total,
      scenes: spec.scenes.slice(0, 24).map((x, i) => ({ caption: cut(x && x.caption, 200), source: cut(x && x.source, 400),
        start: Math.max(0, Math.min(starts[i] || 0, total)) })) });
  }
  function construct(measure) {
    setStage();
    if (tl) tl.kill();
    tl = gsap.timeline({ paused: true });
    cur = lastStart = lastEnd = known = sceneStart = built = camN = 0; pending = []; lit = null; bigs = []; cam = { s: 1, x: W / 2, y: H / 2 };
    rec = measure ? { cams: [], reserve: {}, badges: [], cam: null } : null;
    spec.build(K, tl, scene);
    closeCam();
  }
  // Build the film in the current theme. On a rebuild it keeps its time and whether it was playing.
  function mount() {
    const was = tl ? { time: tl.time(), paused: tl.paused() } : null;
    theme = themeNow(); dark = theme === "dark"; P = PALETTES[theme];
    for (const k in P) root.style.setProperty("--" + k, P[k]);
    root.style.colorScheme = theme;
    failures = []; plan = null;
    try { construct(true); plan = rec; } catch (e) { /* the second pass meets the same error and reports it */ }
    try {
      construct(false);
      sync();
      const end = Math.max(cur, tl.duration());
      tl.to({}, { duration: END_HOLD }, end);
      settle(end + END_HOLD, false);
    } catch (e) { fail(e, true); }
    const n = spec.scenes.length;
    starts = spec.scenes.map((_, i) => tl.labels["s" + i] ?? 0);
    ends = starts.map((s, i) => (i + 1 < n ? starts[i + 1] - HOLD + 0.9 : tl.duration() - 0.05));
    window.__timing = { total: +tl.duration().toFixed(2), scenes: starts.map((s, i) => [+s.toFixed(2), +ends[i].toFixed(2)]) };
    document.body.dataset.timing = JSON.stringify(window.__timing);
    if (built !== n) fail(new Error(`scenes has ${n} entries but build() called scene() for ${built}`), true);
    try { if (!failures.length) lint(ends); } catch (e) { fail(e, true); }
    report();
    tl.eventCallback("onUpdate", show).eventCallback("onComplete", show);
    shown = -1;
    tl.time(0.0001).time(was ? was.time : 0);
    if (was && !was.paused) tl.play();
    return was;
  }
  window.__film = { get timeline() { return tl; } }; // for a host that scrubs, and for tests
  window.explainer = s => build(s).catch(e => fail(e, true)).finally(unready);
  async function build(s) {
    spec = s;
    // What a host reads back without parsing the code: the film's title, its answer and each scene's caption.
    window.__spec = { title: s && s.title || "", answer: s && s.answer || "", captions: (s && s.scenes || []).map(x => x && x.caption || "") };
    $(".ask").textContent = spec.ask || window.ASK || "";
    $(".answer").textContent = spec.answer;
    document.title = spec.title || "Explainer";
    // Widths are measured from the words, so Fjord's type has to be in before anything is built.
    try { await Promise.all(["400 16px Geist", "500 16px Geist", "400 16px 'Geist Mono'"].map(f => document.fonts.load(f))); await document.fonts.ready; } catch (e) { fail(e); }
    mount();
    const n = spec.scenes.length, segs = $(".segs");
    spec.scenes.forEach((sc, i) => { const d = document.createElement("button"); d.className = "seg"; d.innerHTML = "<i></i>"; d.setAttribute("aria-label", `Scene ${i + 1}: ${sc.caption || ""}`); d.onclick = e => { if (e.detail === 0) go(i); }; segs.appendChild(d); });
    const go = i => { tl.seek(starts[Math.max(0, Math.min(n - 1, i))] + 0.001); tl.play(); show(); };
    const toggle = () => { if (tl.time() >= tl.duration() - 0.01) tl.restart(); else tl.paused(!tl.paused()); show(); };
    $(".pp").onclick = toggle;
    $(".src").onclick = () => $(".src").setAttribute("aria-pressed", document.body.classList.toggle("sources"));
    // The bar scrubs: press a scene to play it from its start, drag along the bar to move through the film.
    let drag = null;
    const timeAt = x => {
      const kids = [...segs.children];
      let k = kids.findIndex(d => x <= d.getBoundingClientRect().right);
      if (k < 0) k = n - 1;
      const r = kids[k].getBoundingClientRect(), a = starts[k], b = k + 1 < n ? starts[k + 1] : tl.duration();
      return { k, t: a + Math.max(0, Math.min(1, (x - r.left) / r.width)) * (b - a) };
    };
    segs.addEventListener("pointerdown", e => { drag = { x: e.clientX, moved: false, playing: !tl.paused() }; segs.setPointerCapture(e.pointerId); });
    segs.addEventListener("pointermove", e => {
      if (!drag || (!drag.moved && Math.abs(e.clientX - drag.x) < 4)) return;
      drag.moved = true; tl.pause(); tl.seek(Math.min(timeAt(e.clientX).t, tl.duration() - 0.02)); show();
    });
    const drop = e => {
      if (!drag) return;
      if (!drag.moved) go(timeAt(e.clientX).k); else if (drag.playing) tl.play();
      drag = null; show();
    };
    segs.addEventListener("pointerup", drop);
    segs.addEventListener("pointercancel", () => { drag = null; });
    // Fjord's calls (fixed ones: play, pause, toggle, seek to seconds, scene by index).
    window.__player = {
      play() { if (tl.time() >= tl.duration() - 0.01) tl.restart(); else tl.play(); show(); },
      pause() { tl.pause(); show(); },
      toggle,
      seek(t) { if (Number.isFinite(t)) { tl.seek(Math.max(0, Math.min(t, tl.duration() - 0.02))); show(); } },
      scene(i) { if (Number.isInteger(i)) go(i); },
    };
    // Embedded, a click on the stage plays or pauses.
    if (embed) stage.addEventListener("click", toggle);
    addEventListener("keydown", e => {
      if (embed || e.metaKey || e.ctrlKey || e.altKey) return;
      if (e.key === " " && e.target === document.body) { e.preventDefault(); toggle(); }
      if (e.key === "ArrowRight") go(sceneAt(tl.time()) + 1);
      if (e.key === "ArrowLeft") { const i = sceneAt(tl.time()); go(tl.time() - starts[i] < 0.8 ? i - 1 : i); }
    });
    const retheme = () => { if (themeNow() !== theme) { mount(); show(); post(true); } };
    new MutationObserver(retheme).observe(root, { attributes: true, attributeFilter: ["data-theme"] });
    scheme.addEventListener("change", retheme);
    if (query.has("t")) tl.seek(Math.min(+query.get("t"), tl.duration()));
    else if (query.has("scene")) tl.seek(ends[Math.min(+query.get("scene"), n - 1)]);
    else tl.play();
    show();
    post(true);
    document.body.dataset.ready = "1";
  }
})();
