explainer({
  title: "What you can add",
  answer: "An extension adds rules, a computer, or both. No tool it can use grants or destroys.",
  scenes: [
    { caption: "An extension adds rules, a computer, or both.",
      source: "An extension adds rules, a computer, or both. Its manifest lists the rules, the recipe of the computer, what wakes that computer and the tools it may use." },
    { caption: "Importing changes nothing. Installing switches it on.",
      source: "Importing an extension changes nothing: it shows the rules it adds, the computer it would make, what wakes that computer and the tools it may use. Installing it switches them on." },
    { caption: "What is executed runs in a computer of its own.",
      source: "An extension that executes anything ships a computer: its manifest names the recipe, and installing the extension makes that computer. What Silo only reads, such as rules, is not executed. Nothing of an extension is executed inside Silo." },
    { caption: "It uses only the tools its manifest names.",
      source: "An extension can use only the tools its manifest names, and installing it grants them. A tool reads, shows or protects: it reads a path or the record of a computer, shows an alert, a question or a panel in the app, or protects by denying, holding or pausing. Where a tool applies to computers, the user chooses which ones when installing: all of them, or specific ones. Which tools Silo offers is settled when they are built." },
    { caption: "You can always see what wakes it.",
      source: "Either something happened and nothing waits: a time, a new line in the record, the user's click in one of its panels. Or a request waits at the boundary and Silo asks the extension, which answers one of three things: let it go on, deny it, hold it. \"Let it go on\" is not \"allow\": the rules still decide. If it does not answer in time, the wait ends as its manifest says. The user can always see what wakes it." },
    { caption: "It may deny or hold by itself.",
      source: "Yes, when its manifest names that tool and the user installed it. A protective tool names the computer it applies to. Granting it sets a precaution on that computer, which rests on the extension." },
    { caption: "No tool grants or destroys.",
      source: "No tool an extension can use grants or destroys." },
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
    const place = (p, x, y) => { K.gsap.set(p, { x, y }); p._x = x; p._y = y; return p; };

    // ---------- the forms of the films before ----------
    // Something that does not exist: a grey chip with a line through its words. It never moves.
    const strike = chip => svg("line", { x1: -chip.w / 2 + 9, x2: chip.w / 2 - 9, y1: 0, y2: 0, stroke: K.tone("muted"), "stroke-width": 1.5, "stroke-opacity": 0.75, "stroke-linecap": "round" }, chip);
    const cannot = (x, y, words, o = {}) => { const c = K.chip(x, y, words, { tone: "muted", size: 15, ...o }); strike(c); return c; };
    // A rule: exact, so a solid card, with the icon of what it does (check: it allows; ban: it denies).
    const rule = (x, y, words, icon, o = {}) => K.card(x, y, words, { icon, ...o });
    // A precaution: not exact, so a dashed card, with a shield.
    const precaution = (x, y, words, o = {}) => K.card(x, y, words, { icon: "shield", dashed: true, ...o });
    // A computer: a region whose edge is drawn as a clear line. That edge is its boundary.
    const computer = (x, y, w, h, name) => {
      const r = K.region({ x, y, w, h }, name, { icon: "machine" });
      r.box.setAttribute("stroke-width", 2.5);
      r.box.setAttribute("stroke", K.tone("muted"));
      return r;
    };
    // A program: a pill with three dots at its right that blink while it runs.
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
    const run = (p, s, t0, t1) => p.dots.forEach((d, i) => {
      const n = Math.max(1, Math.floor((t1 - t0 - i * 0.16) / 0.42));
      tl.fromTo(d, { opacity: 1 }, { opacity: 0.18, duration: 0.42, repeat: n - 1, yoyo: true, ease: "sine.inOut", immediateRender: false }, S(s) + t0 + i * 0.16);
    });
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
    // The parked chip is let through: it rides the rest of the link and is taken in at its end.
    const pass = (chip, lk, s, t, dur = 0.8) => {
      tl.to(chip, { motionPath: { path: lk.path, start: chip._stop, end: 1 }, duration: dur, ease: "power2.inOut", immediateRender: false }, T(s, t));
      tl.to(chip, { opacity: 0, scale: 0.72, duration: 0.24, ease: "power2.in" }, T(s, t + dur - 0.22));
      Object.assign(chip, { _shown: false, _op: 0 });
    };
    // A deny at the boundary: the same red cut as the kit's, at the fraction f of the link.
    let cuts = 0;
    const cutAt = (lk, s, t, f) => {
      const m = lk.path.getPointAtLength(lk.path.getTotalLength() * f), id = "ex-cut-" + (cuts++);
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

    // ---------- the forms of this film ----------
    // A card: a region drawn with a card's surface, so that it can hold other pieces. Its name is at its top left.
    const asCard = r => {
      r.box.setAttribute("fill", "url(#fj-surf)");
      r.box.setAttribute("fill-opacity", 1);
      r.box.setAttribute("stroke", "url(#fj-rim)");
      if (r.label) { r.label.setAttribute("font-size", 16); r.label.setAttribute("fill", K.tone("ink")); }
      return r;
    };
    // A manifest: a document with the same four lines, in the same order. A line the extension does not use reads
    // "none" in grey. With no value given, a grey bar stands for it.
    const KEYS = ["rules", "recipe", "wakes on", "tools"];
    const manifest = (x, y, w, vals) => {
      const d = K.doc(x, y, "manifest", { w });
      d.rows = KEYS.map((k, i) => {
        const ln = d.line(k), x0 = +ln.label.getAttribute("x"), col = x0 + ln.label.getComputedTextLength() / k.length * 10;
        const v = vals && vals[i];
        if (v == null) svg("rect", { x: col, y: -3.5, width: (ln.w / 2 - 12 - col) * [0.9, 0.6, 0.78, 0.5][i], height: 7, rx: 3.5, fill: K.tone("faint"), "fill-opacity": 0.5 }, ln);
        else {
          ln.val = ln.label.cloneNode(false);
          ln.parts = [].concat(v).map(words => { const t = svg("tspan", {}, ln.val); t.textContent = words; return t; });
          ln.val.setAttribute("x", col);
          ln.val.setAttribute("fill", K.tone(v === "none" ? "muted" : "ink"));
          ln.appendChild(ln.val);
        }
        return ln;
      });
      return d;
    };
    // An extension: a card with its manifest on it.
    const extension = (doc, name) => asCard(K.region([doc], name, { icon: "package" }));
    // A line of a manifest lights for as long as the voice is on it.
    const flash = (row, s, t, hold, tn = "accent") => {
      const o = svg("rect", { x: -row.w / 2, y: -row.h / 2, width: row.w, height: row.h, rx: 6, fill: "none", stroke: K.tone(tn), "stroke-width": 1.6, opacity: 0 }, row);
      tl.to(o, { opacity: 0.95, duration: 0.3, ease: "power2.out" }, T(s, t));
      if (hold != null) tl.to(o, { opacity: 0, duration: 0.4, ease: "power2.in" }, T(s, t + hold));
    };
    // A piece comes out of another: it starts on that piece, small, and moves to its own place.
    const emerge = (c, from, s, t, dur = 0.8) => {
      if (from.at) from = from.at("center");
      Object.assign(c, { _shown: true, _op: 1 });
      tl.fromTo(c, { x: from.x, y: from.y, opacity: 0, scale: 0.6 }, { x: c._x, y: c._y, opacity: 1, scale: 1, duration: dur, ease: "power3.out", immediateRender: false }, T(s, t));
    };

    // ---------- 1. What an extension is. Left: main. Middle: the two things it can add. Right: the card and its manifest. ----------
    const main1 = computer(190, 360, 260, 400, "main");                   // x 60..320, y 160..560
    const agent1 = K.actor(190, 300, "agent", "agent");
    const prog1 = program(190, 470, "cargo build");
    const man1 = manifest(1069, 221.5, 250);
    const ext1 = extension(man1, "extension");                            // x 918..1220, y 76..337
    const stack1 = K.card(620, 235, "rules", { icon: "list", w: 150 });
    [14, 7].forEach(o => svg("rect", { x: -75 - o, y: -30 - o, width: 150, height: 60, rx: 16, fill: K.tone("surface"), stroke: K.tone("faint"), "stroke-width": 1.25 }, stack1, stack1.firstChild));
    const comp1 = computer(620, 470, 200, 120, "computer");               // x 520..720, y 410..530
    const cprog1 = program(620, 482, "program");
    // The two things an extension can add, held by a bracket. No link is drawn to the computer: its only link is to Silo.
    const adds1 = K.text(792, 367, "adds", { size: 17, color: "muted", anchor: "start" });
    svg("path", { d: `M-46 ${205 - 367} h14 V${530 - 367} h-14 M-32 0 h14`, fill: "none", stroke: K.tone("muted"), "stroke-width": 1.8, "stroke-linecap": "round", "stroke-linejoin": "round" }, adds1);
    const man1b = manifest(1069, 528.5, 250);
    const ext1b = extension(man1b, "extension");                          // x 918..1220, y 383..644
    // Whose each card is, at the right of its top line.
    const who1 = K.text(1220 - 18, ext1._y - ext1.h / 2 + 22, "what people add", { size: 15, color: "muted", anchor: "end" });
    const who1b = K.text(1220 - 18, ext1b._y - ext1b.h / 2 + 22, "ships with Silo", { size: 15, color: "muted", anchor: "end" });
    const same1 = K.text(1069, 360, "=", { size: 28, color: "muted" });

    // 1. Main is there. The card "extension" comes in with its manifest. Out of it come a stack of rules, lit alone, then
    //    a computer with its own edge, lit alone, then both. Each line of the manifest lights as the voice names it.
    //    Last, a second card under the first, marked "ships with Silo", with the same four lines.
    //    Hero: the manifest with its four lines, and the two things it can add.
    scene(0);
    K.camera(null, { dur: 0 });
    K.appear(main1, { from: "fade", at: 0.1 });
    K.appear(agent1, { from: "pop", at: "<0.2" });
    K.appear(prog1, { at: "<0.2" });
    K.appear([ext1, man1], { from: "right", stagger: 0.1, at: V.word(0, 0, "An extension") });
    K.appear(man1.rows, { from: "fade", stagger: 0.12, at: "<0.4" });
    const tR = V.at(0, 1), tC = Math.max(V.word(0, 1, "a computer"), tR + 0.9), tB = Math.max(V.word(0, 1, "or both"), tC + 0.9);
    emerge(stack1, ext1, 0, tR + 0.1);
    K.pulse(stack1, { tone: "ink", at: tR + 0.6 });
    K.dim(stack1, { to: 0.3, dur: 0.3, at: tC });
    emerge(comp1, ext1, 0, tC + 0.1);
    emerge(cprog1, ext1, 0, tC + 0.1);
    K.pulse(comp1, { tone: "ink", at: tC + 0.6 });
    K.dim(stack1, { to: 1, dur: 0.3, at: tB });
    K.appear(adds1, { from: "fade", at: tB + 0.1 });
    K.pulse([stack1, comp1], { tone: "ink", at: tB + 0.1 });
    [["the rules", "the recipe"], ["the recipe", "what wakes"], ["what wakes", "and the tools"], ["the tools", null]].forEach(([a, b], i) => {
      const t0 = V.word(0, 2, a);
      flash(man1.rows[i], 0, t0, (b ? V.word(0, 2, b) : V.end(0, 2) + 0.2) - t0 - 0.1);
    });
    K.pulse(stack1, { tone: "ink", at: V.word(0, 2, "the rules") });
    K.pulse(comp1, { tone: "ink", at: V.word(0, 2, "the recipe of the computer") + 0.3 });
    run(prog1, 0, 0.4, V.total(0) - 1.5);
    run(cprog1, 0, tC + 0.9, V.total(0) - 1.5);
    K.appear([ext1b, man1b], { from: "below", stagger: 0.1, at: V.word(0, 3, "What ships") });
    K.appear(man1b.rows, { from: "fade", stagger: 0.1, at: "<0.3" });
    K.appear(who1b, { from: "fade", at: "<0.1" });
    K.appear(same1, { from: "pop", at: V.word(0, 3, "the same manifest") });
    K.pulse([man1, man1b], { tone: "ink", at: "<0.1" });
    K.appear(who1, { from: "fade", at: V.word(0, 3, "what people add") });
    const all1 = [main1, agent1, prog1, man1, ...man1.rows, ext1, stack1, comp1, cprog1, adds1, man1b, ...man1b.rows, ext1b, who1, who1b, same1];

    // ---------- 2. Registry rules. Left: main and the request. Right: the card, and under it the import view. ----------
    const E2 = 360;
    const main2 = computer(210, 360, 300, 330, "main");                   // x 60..360, y 195..525
    const agent2 = K.actor(140, 320, "agent", "agent");
    const prog2 = program(210, 462, "cargo build");
    const aR2 = agent2.at("right");
    const host2 = K.card(2 * E2 - (aR2.x + 6) + 9 + 85, aR2.y, "crates.io", { icon: "globe", w: 170 });   // the link's middle is on the edge
    const out2 = K.link(agent2, host2, { tone: "ink", from: "right", to: "left" });
    const req2 = K.chip(0, 0, "request", { tone: "ink", icon: "send", size: 14 });
    parkBefore(req2, out2);
    const noRule2 = K.text(E2 + 14, aR2.y - 44, "no rule allows crates.io", { size: 15, color: "muted", anchor: "start" });
    const man2 = manifest(1029, 232, 330, ["allow crates.io on main", "none", "none", "none"]);
    const ext2 = extension(man2, "registry rules");                       // x 838..1220
    const view2 = asCard(K.region({ x: 995, y: 520, w: 450, h: 210 }, "import", { icon: "eye" }));   // x 770..1220, y 415..625
    const shown2 = rule(0, 0, "allow crates.io on main", "check");
    place(shown2, 770 + 24 + shown2.w / 2, 505);
    const grant2 = K.chip(0, 0, "grant", { tone: "you", size: 15 });
    place(grant2, shown2._x + shown2.w / 2 + 14 + grant2.w / 2, 505);
    const leave2 = K.chip(0, 0, "leave", { tone: "ink", size: 15 });
    place(leave2, 1220 - 24 - leave2.w / 2, 585);
    const install2 = K.chip(0, 0, "install", { tone: "ink", size: 15 });
    place(install2, leave2._x - leave2.w / 2 - 12 - install2.w / 2, 585);
    const opens2 = K.link({ x: 1029, y: ext2._y + ext2.h / 2 + 6 }, { x: 1029, y: view2._y - view2.h / 2 - 9 }, { tone: "muted" });
    const you2 = K.actor(680, 575, "you", "you", { tone: "you" });
    const onEdge2 = rule(0, 0, "allow crates.io on main", "check");
    place(onEdge2, E2 + 22 + onEdge2.w / 2, aR2.y + 100);

    // 2. The card "registry rules": one rule, and "none" three times. The import view opens and shows that rule, with
    //    "grant"; beside main a request to crates.io stops at the boundary. You choose "install": the rule moves to the
    //    edge of main and the request passes. Hero: the request stopped while the view is open, then passing.
    scene(1);
    K.hide(all1, { at: 0 });
    K.appear(main2, { from: "fade", at: 0.1 });
    K.appear(agent2, { from: "pop", at: "<0.2" });
    K.appear(prog2, { at: "<0.2" });
    K.appear([ext2, man2], { from: "right", stagger: 0.1, at: V.at(1, 0) });
    K.appear(man2.rows, { from: "fade", stagger: 0.1, at: "<0.4" });
    flash(man2.rows[0], 1, V.word(1, 1, "one rule"), V.end(1, 1) - V.word(1, 1, "one rule") + 0.2);
    K.appear(host2, { at: V.word(1, 1, "crates.io") - 0.2 });
    [1, 2, 3].forEach(i => tl.to(man2.rows[i].val, { fill: K.tone("ink"), duration: 0.3, yoyo: true, repeat: 1, repeatDelay: 0.5, ease: "power2.inOut" }, T(1, V.word(1, 2, "Nothing") + (i - 1) * 0.12)));
    flash(man2.rows[1], 1, V.word(1, 2, "no computer"), V.end(1, 2) - V.word(1, 2, "no computer") + 0.2, "muted");
    K.appear(you2, { from: "pop", at: V.at(1, 3) - 0.5 });
    K.draw(opens2, { at: V.at(1, 3) - 0.3, dur: 0.4 });
    K.appear(view2, { from: "above", at: V.at(1, 3) - 0.1 });
    drawTo(out2, 1, V.at(1, 3) + 0.1);
    ride(req2, out2, 1, V.at(1, 3) + 0.2, 0.9);
    K.appear(noRule2, { from: "fade", at: V.word(1, 3, "changes nothing") + 0.3 });
    K.appear(shown2, { from: "below", at: V.at(1, 4) - 0.2 });
    K.appear(grant2, { from: "pop", at: V.word(1, 4, "the rule it adds") });
    K.appear([install2, leave2], { from: "fade", stagger: 0.1, at: V.end(1, 4) - 0.3 });
    const tI = V.word(1, 5, "Installing");
    K.pulse(you2, { tone: "you", at: tI });
    K.recolor(install2, "you", { at: tI + 0.15 });
    K.pulse(install2, { tone: "you", at: "<" });
    K.dim(leave2, { to: 0.35, at: "<" });
    emerge(onEdge2, shown2, 1, tI + 0.35, 0.8);
    K.hide(noRule2, { at: tI + 0.9, dur: 0.3 });
    K.glow(onEdge2, { tone: "ink", at: tI + 1.0 });
    onward(out2, 1, tI + 1.15);
    pass(req2, out2, 1, tI + 1.15);
    K.pulse(host2, { tone: "ink", at: tI + 1.85 });
    run(prog2, 1, 0.4, V.total(1) - 1.5);
    const all2 = [main2, agent2, prog2, host2, out2, req2, noRule2, man2, ...man2.rows, ext2, view2, shown2, grant2, leave2, install2, opens2, you2, onEdge2];

    // ---------- 3 and 4. Disk watch. Top: main, Silo, the computer "disk watch". Bottom right: the card and its manifest. ----------
    // A chip rides part of a link, from the fraction a to the fraction b. It fades in where it starts, and out where it
    // ends unless it is kept.
    const along = (chip, lk, s, t, dur, a = 0, b = 1, keep = false) => {
      const p0 = lk.path.getPointAtLength(lk.len * a), p1 = lk.path.getPointAtLength(lk.len * b);
      tl.set(chip, { x: p0.x, y: p0.y }, T(s, t));
      tl.fromTo(chip, { opacity: 0, scale: 0.8 }, { opacity: 1, scale: 1, duration: 0.25, ease: "power2.out", immediateRender: false }, T(s, t));
      tl.to(chip, { motionPath: { path: lk.path, start: a, end: b }, duration: dur, ease: "power2.inOut", immediateRender: false }, T(s, t));
      if (keep) Object.assign(chip, { _x: p1.x, _y: p1.y, _shown: true, _op: 1 });
      else tl.to(chip, { opacity: 0, scale: 0.72, duration: 0.22, ease: "power2.in" }, T(s, t + dur - 0.2));
    };
    // A shelf: three columns with the kind of tool as their title.
    const shelf = (x0, y0, widths, h) => {
      let x = x0;
      return ["reads", "shows", "protects"].map((name, i) => {
        const r = K.region({ x: x + widths[i] / 2, y: y0 + h / 2, w: widths[i], h }, name);
        r.label.setAttribute("font-size", 17); r.label.setAttribute("font-weight", 600); r.label.setAttribute("fill", K.tone("ink"));
        x += widths[i] + 14;
        return r;
      });
    };
    const tool = words => K.chip(0, 0, words, { tone: "ink", mono: true, size: 14 });

    const main3 = computer(210, 180, 300, 220, "main");                   // x 60..360, y 70..290
    const agent3 = K.actor(135, 172, "agent", "agent", { r: 30 });
    const work3 = K.chip(275, 180, "/work", { tone: "ink", icon: "folder", mono: true, size: 15 });
    const dw3 = computer(1070, 180, 300, 220, "disk watch");              // x 920..1220, y 70..290
    const ghost3 = K.region({ x: 1070, y: 180, w: 300, h: 220 }, null, { dashed: true });
    const dprog3 = program(1070, 188, "program");
    const silo3 = asCard(K.region({ x: 640, y: 180, w: 240, h: 116 }, "Silo", { icon: "layers" }));   // x 520..760, y 122..238
    const noCode3 = cannot(640, 196, "extension code");
    const man3 = manifest(1054, 505, 280, ["none", "disk watch", "every minute", "read, panel, alert"]);
    const ext3 = extension(man3, "disk watch");                           // x 888..1220, y 360..620
    const recipe3 = K.doc(730, 470, "recipe", { w: 130 });
    const recipeLines3 = [recipe3.line(), recipe3.line()];
    const app3 = K.window(420, 495, "app", { w: 320, icon: "window" });
    const appRows3 = [app3.row("main", { icon: "machine" }), app3.row("disk watch", { icon: "machine" })];

    // 3. The card "disk watch" and its manifest. What it executes is a program; it needs a computer, drawn first as an
    //    empty dashed place. The line "recipe" lights and a document "recipe" slides out of it; on "installing" the
    //    document goes to that place and the computer "disk watch" is made there, with its own edge. Then Silo, between
    //    the two computers, with "extension code" struck through. Last, the app lists both computers.
    //    Hero: the two computers, each with its own edge, and the struck-through chip on Silo.
    scene(2);
    K.hide(all2, { at: 0 });
    K.appear(main3, { from: "fade", at: 0.1 });
    K.appear(agent3, { from: "pop", at: "<0.2" });
    K.appear(work3, { at: "<0.2" });
    K.appear([ext3, man3], { from: "right", stagger: 0.1, at: V.at(2, 0) });
    K.appear(man3.rows, { from: "fade", stagger: 0.1, at: "<0.4" });
    flash(man3.rows[2], 2, V.word(2, 1, "Every minute"), V.end(2, 1) - V.word(2, 1, "Every minute"));
    K.pulse(work3, { tone: "ink", at: V.word(2, 1, "a folder of main") });
    emerge(dprog3, ext3, 2, V.at(2, 2) - 0.1);
    K.pulse(dprog3, { tone: "ink", at: V.word(2, 2, "executed") });
    K.appear(ghost3, { from: "fade", at: V.word(2, 3, "ships a computer") - 0.2 });
    const tRec = V.word(2, 4, "names the recipe") - 0.2, tInst = Math.max(V.word(2, 4, "installing"), tRec + 1.6);
    flash(man3.rows[1], 2, tRec, tInst - tRec + 0.8);
    emerge(recipe3, man3.rows[1].at("left"), 2, tRec + 0.3, 0.8);
    K.appear(recipeLines3, { from: "fade", stagger: 0.1, at: tRec + 0.6 });
    tl.to(recipe3, { x: 1070, y: 180, duration: 0.9, ease: "power2.inOut" }, T(2, tInst));
    tl.to(recipe3, { opacity: 0, scale: 0.7, duration: 0.35, ease: "power2.in" }, T(2, tInst + 0.3));
    Object.assign(recipe3, { _shown: false, _op: 0 });
    K.hide(ghost3, { at: tInst + 0.6, dur: 0.3 });
    K.appear(dw3, { from: "fade", at: tInst + 0.6 });
    K.pulse(dw3, { tone: "ink", at: Math.max(V.word(2, 4, "makes that computer"), tInst + 1.1) });
    run(dprog3, 2, tInst + 1.2, V.total(2) - 1.5);
    K.appear(silo3, { from: "fade", at: V.at(2, 5) - 0.1 });
    K.appear(noCode3, { from: "pop", at: V.word(2, 5, "executed inside Silo") - 0.2 });
    K.pulse(silo3, { tone: "ink", at: V.word(2, 5, "inside Silo") + 0.3 });
    K.appear(app3, { from: "below", at: V.at(2, 6) - 0.3 });
    K.appear(appRows3, { from: "fade", stagger: 0.15, at: "<0.3" });
    K.glow(appRows3[1], { tone: "accent", at: V.word(2, 6, "that computer") });

    // Scene 4: the one link, the tools on a shelf, then the tools at work, then the choice of computers.
    const ask4 = K.link(dw3, silo3, { tone: "ink", from: "left", to: "right" });
    const asked4 = K.chip(0, 0, "tool", { tone: "ink", icon: "tool", size: 14 });
    const tools4 = [tool("read  /work on main"), tool("panel"), tool("alert")];
    tools4.forEach((c, i) => place(c, 753, 440 + i * 46));
    const shelf4 = shelf(60, 372, [240, 150, 150], 190);                  // x 60..628, y 372..562
    const slots4 = [[shelf4[0]._x, 452], [shelf4[1]._x, 452], [shelf4[1]._x, 500]];
    const read4 = K.link(silo3, work3, { tone: "muted", dashed: true, from: "left", to: "right" });
    const readAsk4 = tool("read"), size4 = tool("41 GB"), size4b = tool("41 GB");
    const you4 = K.actor(730, 500, "you", "you", { tone: "you" });
    const app4 = K.window(1075, 500, "app", { w: 290, h: 222, icon: "window" });   // x 930..1220, y 389..611
    const appRows4 = [app4.row("main", { icon: "machine" }), app4.row("disk watch", { icon: "machine" })];
    const panel4 = svg("g", { opacity: 0 }, K.world);
    svg("rect", { x: 1075 - 131, y: 536, width: 262, height: 62, rx: 9, fill: "none", stroke: K.tone("accent"), "stroke-width": 1.25, "stroke-opacity": 0.7 }, panel4);
    const panelName4 = svg("text", { x: 1075 - 117, y: 567, "font-size": 15, "font-family": "'Geist Mono', ui-monospace, Menlo, monospace", fill: K.tone("ink"), "dominant-baseline": "central" }, panel4);
    panelName4.textContent = "/work  41 GB";
    [16, 22, 20, 30, 38].forEach((h, i) => svg("rect", { x: 1075 + 28 + i * 19, y: 590 - h, width: 12, height: h, rx: 3, fill: K.tone("accent"), "fill-opacity": 0.6 }, panel4));
    // Both leave Silo downwards, and pass under the computer "disk watch" without touching it.
    const toApp4 = K.link({ x: 735, y: 244 }, { x: 1010, y: 380 }, { tone: "ink", from: "bottom", to: "top" });
    const toYou4 = K.link({ x: 640, y: 244 }, you4, { tone: "ink", from: "bottom", to: "top" });
    const panelAsk4 = tool("panel"), panelGoes4 = tool("panel");
    const alertAsk4 = K.chip(0, 0, "alert", { tone: "warn", icon: "bell", size: 14 }), alert4 = K.chip(0, 0, "alert", { tone: "warn", icon: "bell", size: 15 });
    const view4 = asCard(K.region({ x: 1010, y: 490, w: 420, h: 200 }, "install", { icon: "plug" }));   // x 800..1220, y 390..590
    const line4 = K.text(830, 462, "read  /work on", { size: 16, mono: true, anchor: "start" });
    const pick4 = K.chip(0, 0, "main", { tone: "you", icon: "check", size: 15 });
    place(pick4, 830 + pick4.w / 2, 528);
    const other4 = K.chip(0, 0, "all computers", { tone: "ink", size: 15 });
    place(other4, pick4._x + pick4.w / 2 + 14 + other4.w / 2, 528);

    // 4. One link, from the computer "disk watch" to Silo. The line "tools" of the manifest opens into three, and each
    //    goes to its column of the shelf; "protects" stays empty. Then the tools work: Silo reads /work inside main and
    //    the size comes back to disk watch; a panel appears in the app; an alert reaches you. Last, the install view:
    //    "read /work on", with "main" ticked. Hero: the three columns with disk watch's tools in two of them.
    scene(3);
    K.hide([app3, ...appRows3], { at: 0 });
    K.draw(ask4, { at: V.word(3, 0, "asks Silo") - 0.2, dur: 0.6 });
    K.travel(asked4, ask4, { dur: 0.8, tone: "ink", at: V.word(3, 0, "through tools") - 0.3 });
    const tOpen = V.word(3, 1, "the tools its manifest names") - 0.2;
    flash(man3.rows[3], 3, tOpen, V.end(3, 1) - tOpen);
    tools4.forEach((c, i) => emerge(c, man3.rows[3].at("left"), 3, tOpen + 0.4 + i * 0.18, 0.7));
    K.pulse(tools4, { tone: "ink", at: V.word(3, 1, "grants them") });
    [["reads", [0]], ["shows", [1, 2]], ["protects", []]].forEach(([w, which], k) => {
      const t = V.word(3, 2, w) - 0.15;
      K.appear(shelf4[k], { from: "fade", dur: 0.5, at: t });
      which.forEach((i, n) => K.moveTo(tools4[i], slots4[i][0], slots4[i][1], { dur: 0.7, at: t + 0.15 + n * 0.12 }));
    });
    // The tools at work. The card and its manifest make room for the app and you.
    const tWork = V.at(3, 3);
    K.hide([ext3, man3, ...man3.rows], { at: tWork - 0.4 });
    K.pulse(tools4[0], { tone: "ink", at: tWork });
    along(readAsk4, ask4, 3, tWork, 0.5);
    K.draw(read4, { at: tWork + 0.45, dur: 0.5 });
    K.pulse(work3, { tone: "ink", at: tWork + 0.9 });
    along(size4, read4, 3, tWork + 1.0, 0.6, 1, 0);
    along(size4b, ask4, 3, tWork + 1.55, 0.6, 1, 0);
    K.pulse(dw3, { tone: "ink", at: tWork + 2.1 });
    const tShow = Math.max(V.at(3, 4), tWork + 2.3);
    K.appear(app4, { from: "below", at: tShow - 0.5 });
    K.appear(appRows4, { from: "fade", stagger: 0.1, at: "<0.2" });
    K.appear(you4, { from: "pop", at: tShow - 0.3 });
    K.pulse(tools4[1], { tone: "ink", at: tShow });
    along(panelAsk4, ask4, 3, tShow, 0.45);
    K.draw(toApp4, { at: tShow + 0.4, dur: 0.4 });
    along(panelGoes4, toApp4, 3, tShow + 0.7, 0.5);
    tl.to(panel4, { opacity: 1, duration: 0.5, ease: "power2.out" }, T(3, tShow + 1.15));
    const tAl = Math.max(V.word(3, 4, "an alert") - 0.5, tShow + 0.5);
    K.pulse(tools4[2], { tone: "ink", at: tAl });
    along(alertAsk4, ask4, 3, tAl, 0.45);
    K.draw(toYou4, { at: tAl + 0.4, dur: 0.4 });
    K.travel(alert4, toYou4, { land: "dock", dur: 0.7, at: tAl + 0.7 });
    // The choice of computers, in place of the app.
    const tPick = Math.max(V.word(3, 5, "applies to computers"), tAl + 2.4);
    K.hide([app4, ...appRows4, you4, toApp4, toYou4, alert4], { at: tPick - 0.3 });
    tl.to(panel4, { opacity: 0, duration: 0.4 }, T(3, tPick - 0.3));
    K.appear(view4, { from: "below", at: tPick });
    K.appear(line4, { from: "fade", at: tPick + 0.3 });
    K.appear([pick4, other4], { from: "fade", stagger: 0.12, at: Math.max(V.word(3, 5, "you choose which ones"), tPick + 0.6) });
    K.dim(other4, { to: 0.45, at: V.word(3, 5, "when you install") });
    K.pulse(pick4, { tone: "you", at: "<" });
    K.pulse(main3, { tone: "you", at: "<0.2" });
    run(dprog3, 3, 0.2, V.total(3) - 1.5);
    const all34 = [main3, agent3, work3, dw3, dprog3, silo3, noCode3, ask4, ...tools4, ...shelf4, read4, view4, line4, pick4, other4];

    // ---------- 5. What wakes it. First the two manifests, each over its computer. Then main at the left with one request
    // per line, Silo and the computer "request check" at the right, and the answers that come back between them. ----------
    // A card drawn smaller: its drawing is scaled inside the piece, and the piece takes the smaller size.
    const shrink = (p, k) => {
      const kids = [...p.childNodes], inner = svg("g", { transform: `scale(${k})` }, p);
      kids.forEach(c => inner.appendChild(c));
      Object.assign(p, { w: p.w * k, h: p.h * k, rx: p.rx * k });
      delete p.shade;
      return p;
    };
    // Silo, small: a card with its name only.
    const siloCard = (x, y) => {
      const r = asCard(K.region({ x, y, w: 150, h: 60 }, "Silo", { icon: "layers" }));
      r.label.setAttribute("y", 0); K.gsap.set(r.icon, { y: 0 });
      r.label.setAttribute("font-size", 18);
      return r;
    };
    const RC = ["none", "request check", "a network request of main waits", ["deny, hold  the network requests ", "of main"]];
    const DW = ["none", "disk watch", "every minute", "read, panel, alert"];
    const E5 = 330, LY = [180, 255, 330, 415, 585];
    const man5a = manifest(505, 206, 280, DW);
    const ext5a = extension(man5a, "disk watch");                         // x 339..671, y 60..321
    const man5b = manifest(961.5, 206, 465, RC);
    const ext5b = extension(man5b, "request check");                      // x 703..1220, y 60..321
    const dwc5 = computer(505, 420, 250, 110, "disk watch");              // x 380..630, y 365..475
    const dwp5 = program(505, 433, "program");
    const clock5 = K.icon(344, 420, "clock", { size: 40 });
    const rc5 = computer(1060, 420, 320, 160, "request check");           // x 900..1220, y 340..500
    const rcp5 = program(1060, 432, "program");
    const main5 = computer(195, 370, 270, 520, "main");                   // x 60..330, y 110..630
    const lanes5 = LY.map((y, i) => K.link({ x: 96, y }, { x: i ? 430 : 500, y }, { tone: "ink" }));
    const f5 = lanes5.map(l => (E5 - 96) / l.len);                        // where each line meets the edge of main
    const reqs5 = LY.map((y, i) => K.chip(0, 0, i === 3 ? "paste.rs" : "example.com", { tone: "ink", icon: "send", mono: true, size: 14 }));
    reqs5.forEach((c, i) => parkBefore(c, lanes5[i], f5[i]));
    const silo5 = siloCard(690, 420);                                     // x 615..765, y 390..450
    const one5 = K.link(rc5, silo5, { tone: "ink", from: "left", to: "right" });
    const fan5 = [0, 1, 2, 3].map(i => K.link(silo5, { x: E5 + 16, y: LY[i] + 30 }, { tone: "muted", from: "left" }));
    const copy5 = tool("example.com"), copy5b = tool("example.com");
    const ans5 = ["go on", "deny", "hold", "go on"].map(w => K.chip(0, 0, w, { tone: "ink", size: 15 }));
    const ansUp5 = ["go on", "deny", "hold", "go on"].map(w => K.chip(0, 0, w, { tone: "ink", size: 15 }));
    const you5 = K.actor(1060, 578, "you", "you", { tone: "you", r: 30 });
    const toYou5 = K.link(silo5, you5, { tone: "ink", from: "bottom", to: "left" });
    const held5 = K.chip(0, 0, "held request", { tone: "warn", icon: "pause", size: 14 });
    const rule5 = shrink(rule(0, 0, "allow example.com", "check"), 0.85);
    place(rule5, E5 + 22 + rule5.w / 2, 494);
    const noRule5 = K.text(E5 + 24, 538, "no rule allows paste.rs", { size: 15, color: "muted", anchor: "start" });
    const timer5 = K.icon(E5 + 30, LY[4], "timer", { size: 26, tone: "warn" });
    const sweep5 = svg("circle", { cx: E5 + 30, cy: LY[4], r: 19, fill: "none", stroke: K.tone("warn"), "stroke-width": 2.5, "stroke-linecap": "round", opacity: 0, transform: `rotate(-90 ${E5 + 30} ${LY[4]})` }, K.world);
    const ends5 = K.text(E5 + 60, LY[4], "the wait ends as its manifest says", { size: 15, color: "muted", anchor: "start" });
    const app5 = K.window(940, 190, "app", { w: 560, icon: "window" });   // x 660..1220, y 121..259
    const appRows5 = [app5.row("disk watch", { icon: "package", meta: "wakes on every minute" }),
                      app5.row("request check", { icon: "package", meta: "wakes on a network request of main waits" })];
    const metaPulse = (row, t) => tl.to(row.meta, { fill: K.tone("ink"), duration: 0.35, yoyo: true, repeat: 1, repeatDelay: 0.4, ease: "power2.inOut" }, t);
    // A request rides to the edge of main and waits there, in amber.
    const waits5 = (i, t) => {
      drawTo(lanes5[i], 4, t, f5[i], 0.5);
      ride(reqs5[i], lanes5[i], 4, t + 0.1, 0.7);
      K.recolor(reqs5[i], "warn", { dur: 0.3, at: t + 0.75 });
    };
    // An answer comes back from the computer "request check" to Silo, and Silo brings it to the request it is for.
    const answer5 = (i, t) => {
      along(ansUp5[i], one5, 4, t, 0.4);
      if (i) K.draw(fan5[i], { at: t + 0.2, dur: 0.4 });
      K.travel(ans5[i], fan5[i], { land: "dock", dur: 0.55, at: t + 0.35 });
      return t + 0.9;
    };

    // 5. Two manifests side by side, the line "wakes on" lit; a clock ticks and the computer "disk watch" runs once.
    //    Then the camera pulls back: a request of main waits at the boundary in amber, its copy goes to Silo and on to
    //    the computer "request check". Three answers come back, each to its own request: "go on" and the request leaves;
    //    "deny" and a red cut closes the boundary while a refusal returns into main; "hold" and the request stays amber
    //    while a card reaches you. Then a request to paste.rs: the answer is "go on" and the rule still denies it. Then a
    //    wait that runs out. Last, the app lists both extensions with what wakes them.
    //    Hero: the three answers, and the rule that still denies after "go on".
    scene(4);
    K.hide(all34, { at: 0 });
    K.camera([clock5, dwc5, ext5a, ext5b, rc5], { pad: 44, dur: 0.9, at: 0 });
    K.appear([ext5a, man5a], { from: "below", stagger: 0.1, at: 0.2 });
    K.appear(man5a.rows, { from: "fade", stagger: 0.08, at: "<0.3" });
    K.appear([dwc5, dwp5], { from: "fade", at: "<0.2" });
    const third5 = [ext5b, man5b, rc5, rcp5];
    K.appear([ext5b, man5b], { from: "below", stagger: 0.1, at: 0.3 });
    K.appear(man5b.rows, { from: "fade", stagger: 0.08, at: "<0.3" });
    K.appear([rc5, rcp5], { from: "fade", at: "<0.2" });
    const tLit = V.word(4, 0, "wakes");
    const tStage = V.word(4, 3, "waits at the boundary") - 0.7;               // the picture changes here
    flash(man5a.rows[2], 4, tLit, tStage - 0.4 - tLit);
    const tClock = V.word(4, 1, "wakes on a time") - 0.2;
    K.appear(clock5, { from: "pop", at: tClock });
    K.pulse(clock5, { tone: "ink", at: tClock + 0.5 });
    K.pulse(clock5, { tone: "ink", at: tClock + 1.0 });
    run(dwp5, 4, tClock + 0.6, tClock + 2.1);
    K.pulse(dwc5, { tone: "ink", at: tClock + 0.7 });
    flash(man5b.rows[2], 4, tLit + 0.15, V.at(4, 7) - 0.6 - tLit - 0.15);
    K.dim(third5, { to: 0.3, at: V.at(4, 1) - 0.2 });
    K.dim(third5, { to: 1, at: V.at(4, 2) });
    K.pulse(ext5b, { tone: "ink", at: V.word(4, 2, "request check") });
    K.hide([ext5a, man5a, ...man5a.rows, dwc5, dwp5, clock5], { at: tStage - 0.3 });
    K.camera(null, { at: tStage - 0.2, dur: 1.0 });
    K.appear([main5, rule5], { from: "fade", at: tStage });
    waits5(0, tStage + 0.5);
    const tAsk = Math.max(V.word(4, 3, "and Silo asks it") - 0.3, tStage + 1.5);
    K.appear(silo5, { from: "fade", at: tAsk - 0.5 });
    K.draw([fan5[0], one5], { at: tAsk - 0.3, dur: 0.4, stagger: 0 });
    along(copy5, fan5[0], 4, tAsk, 0.55, 1, 0);
    along(copy5b, one5, 4, tAsk + 0.5, 0.45, 1, 0);
    K.pulse(rc5, { tone: "ink", at: tAsk + 0.95 });
    const t4 = V.at(4, 4);
    waits5(1, t4);
    waits5(2, t4 + 0.3);
    const tGo = V.word(4, 4, "let it go on") - 0.4;
    let e = answer5(0, tGo);
    K.recolor(reqs5[0], "ink", { dur: 0.3, at: e });
    onward(lanes5[0], 4, e + 0.1, 0.5);
    pass(reqs5[0], lanes5[0], 4, e + 0.1, 0.7);
    const tDeny = Math.max(V.word(4, 4, "deny it") - 0.4, tGo + 0.7);
    e = answer5(1, tDeny);
    cutAt(lanes5[1], 4, e, f5[1]);
    K.recolor(reqs5[1], "ink", { dur: 0.3, at: e });
    K.relabel(reqs5[1], "refusal", { at: e + 0.1 });
    K.moveTo(reqs5[1], 104 + reqs5[1].w / 2, LY[1], { dur: 0.6, at: e + 0.3 });
    const tHold = Math.max(V.word(4, 4, "hold it") - 0.4, tDeny + 0.7);
    e = answer5(2, tHold);
    K.pulse(reqs5[2], { tone: "warn", at: e });
    K.appear(you5, { from: "pop", at: e - 0.4 });
    K.draw(toYou5, { at: e, dur: 0.5 });
    K.travel(held5, toYou5, { land: "dock", dur: 0.8, at: e + 0.3 });
    const t5 = Math.max(V.at(4, 5), e + 0.6);
    waits5(3, t5);
    e = answer5(3, Math.max(V.word(4, 5, "is not allow") - 0.3, t5 + 0.9));
    K.recolor(reqs5[3], "ink", { dur: 0.3, at: e });
    const tRule = Math.max(V.word(4, 5, "the rules still decide"), e + 0.3);
    K.glow(rule5, { tone: "ink", at: tRule });
    cutAt(lanes5[3], 4, tRule + 0.3, f5[3]);
    K.appear(noRule5, { from: "fade", at: tRule + 0.4 });
    const t6 = Math.max(V.at(4, 6), tRule + 1.0);
    waits5(4, t6);
    K.appear(timer5, { from: "pop", at: t6 + 0.9 });
    const tEnds = Math.max(V.word(4, 6, "the wait ends"), t6 + 2.0);
    tl.set(sweep5, { opacity: 1 }, T(4, t6 + 1.0));
    tl.fromTo(sweep5, { drawSVG: "0% 100%" }, { drawSVG: "0% 0%", duration: tEnds - t6 - 1.0, ease: "none", immediateRender: false }, T(4, t6 + 1.0));
    tl.to(sweep5, { opacity: 0, duration: 0.2 }, T(4, tEnds));
    K.pulse(timer5, { tone: "warn", at: tEnds });
    K.recolor(timer5, "muted", { at: tEnds });
    K.recolor(reqs5[4], "ink", { dur: 0.3, at: tEnds });
    K.appear(ends5, { from: "fade", at: tEnds + 0.1 });
    K.pulse(man5b, { tone: "ink", at: V.word(4, 6, "as its manifest says") });
    const t7 = V.at(4, 7);
    K.hide([ext5b, man5b, ...man5b.rows], { at: t7 - 0.5 });
    K.appear(app5, { from: "above", at: t7 - 0.2 });
    K.appear(appRows5, { from: "fade", stagger: 0.2, at: "<0.3" });
    metaPulse(appRows5[0], T(4, V.word(4, 7, "what wakes")));
    metaPulse(appRows5[1], T(4, V.word(4, 7, "what wakes") + 0.2));
    run(rcp5, 4, tAsk + 0.9, V.total(4) - 1.5);
    const all5 = [rc5, rcp5, main5, ...lanes5, ...reqs5, silo5, one5, ...fan5, ...ans5, you5, toYou5, held5, rule5, noRule5, timer5, ends5, app5, ...appRows5];

    // ---------- 6. It may protect by itself. Top: main and the card "request check". Bottom: the shelf and the install view. ----------
    const SHELF = [200, 130, 400];
    // The tools of the three examples, each in its column of a shelf.
    const stock = (cols, y) => {
      const t = [tool("read  /work on main"), tool("panel"), tool("alert"), tool("deny, hold  the network requests of main")];
      [[0, 0], [1, 0], [1, 1], [2, 0]].forEach(([c, n], i) => place(t[i], cols[c]._x, y + n * 46));
      return t;
    };
    const main6 = computer(260, 175, 400, 230, "main");                   // x 60..460, y 60..290
    const agent6 = K.actor(260, 165, "agent", "agent");
    const man6 = manifest(961.5, 206, 465, RC);
    const ext6 = extension(man6, "request check");                        // x 703..1220, y 60..321
    const shelf6 = shelf(60, 440, SHELF, 190);                            // x 60..818, y 440..630
    const tools6 = stock(shelf6, 525);
    const view6 = asCard(K.region({ x: 1029, y: 520, w: 382, h: 190 }, "install", { icon: "plug" }));   // x 838..1220, y 425..615
    const line6 = K.text(862, 492, "deny, hold  the network requests of", { size: 15, mono: true, anchor: "start" });
    const pick6 = K.chip(0, 0, "main", { tone: "you", icon: "check", size: 15 });
    place(pick6, 862 + pick6.w / 2, 555);
    const other6 = K.chip(0, 0, "all computers", { tone: "ink", size: 15 });
    place(other6, pick6._x + pick6.w / 2 + 14 + other6.w / 2, 555);
    const prec6 = precaution(0, 0, "request check may deny or hold", { sub: "network requests" });
    place(prec6, 260, 290 + 22 + prec6.h / 2);
    const tie6 = K.link(prec6, ext6, { tone: "muted", dashed: true, arrow: false, from: "right", to: "left" });

    // 6. The line "tools" of the manifest lights, and that tool goes to "protects" on the shelf, beside the tools of disk
    //    watch. The words "of main" light and main answers; the install view shows "main" ticked. Then a precaution
    //    card comes from that choice to the edge of main, and a dashed line ties it to the extension.
    //    Hero: the precaution card on main's edge, tied to the extension.
    scene(5);
    K.hide(all5, { at: 0 });
    K.appear(main6, { from: "fade", at: 0.1 });
    K.appear(agent6, { from: "pop", at: "<0.2" });
    K.appear([ext6, man6], { from: "fade", stagger: 0.1, at: 0.1 });
    K.appear(man6.rows, { from: "fade", stagger: 0.06, at: "<0.2" });
    K.appear(shelf6, { from: "below", stagger: 0.08, at: 0.3 });
    K.appear(tools6.slice(0, 3), { from: "fade", stagger: 0.06, at: "<0.3" });
    K.dim(tools6.slice(0, 3), { to: 0.6, dur: 0.3, at: "<0.5" });
    const tTool = V.word(5, 0, "its manifest names that tool") - 0.2;
    flash(man6.rows[3], 5, V.word(5, 0, "denies and holds") - 0.1, V.at(5, 1) - V.word(5, 0, "denies and holds"));
    emerge(tools6[3], man6.rows[3].at("left"), 5, tTool, 0.9);
    K.pulse(tools6[3], { tone: "ink", at: V.word(5, 0, "you installed it") });
    const tOf = V.word(5, 1, "names the computer");
    tl.to(man6.rows[3].parts[1], { fill: K.tone("accent"), fontWeight: 700, duration: 0.3 }, T(5, tOf));
    flash(man6.rows[3], 5, tOf, null);
    K.pulse(main6, { tone: "accent", at: tOf + 0.3 });
    K.appear(view6, { from: "below", at: V.word(5, 1, "it applies to") - 0.5 });
    K.appear(line6, { from: "fade", at: "<0.3" });
    K.appear([pick6, other6], { from: "fade", stagger: 0.12, at: "<0.2" });
    K.dim(other6, { to: 0.45, at: "<0.4" });
    const tSet = V.at(5, 2);
    K.pulse(pick6, { tone: "you", at: tSet });
    emerge(prec6, pick6, 5, V.word(5, 2, "sets a precaution") - 0.3, 0.9);
    K.pulse(main6, { tone: "ink", at: V.word(5, 2, "on that computer") });
    K.draw(tie6, { at: V.word(5, 2, "which rests") - 0.2, dur: 0.7 });
    K.pulse(ext6, { tone: "ink", at: V.word(5, 2, "the extension") });
    const all6 = [main6, agent6, man6, ...man6.rows, ext6, ...shelf6, ...tools6, view6, line6, pick6, other6, prec6, tie6];

    // ---------- 7. No tool grants or destroys. Top: main, Silo, the computer "request check". Middle: the shelf, and the import
    // view of scene 2. Bottom: the tools that do not exist. ----------
    const main7 = computer(210, 180, 300, 180, "main");                   // x 60..360, y 90..270
    const agent7 = K.actor(190, 172, "agent", "agent", { r: 30 });
    const lock7 = K.icon(360, 180, "lock", { size: 22 });
    svg("circle", { r: 18, fill: K.tone("surfaceHi"), stroke: K.tone("muted"), "stroke-width": 2 }, lock7, lock7.firstChild);
    const rc7 = computer(1050, 180, 340, 180, "request check");           // x 880..1220, y 90..270
    const rcp7 = program(1050, 152, "program");
    const said7 = tool("\"open the network of main\""), saidGoes7 = tool("\"open the network of main\"");
    place(said7, 1050, 218);
    const silo7 = siloCard(640, 180);                                     // x 565..715, y 150..210
    const one7 = K.link(rc7, silo7, { tone: "ink", from: "left", to: "right" });
    const shelf7 = shelf(60, 330, SHELF, 170);                            // x 60..818, y 330..500
    const tools7 = stock(shelf7, 408);
    const look7 = K.link({ x: 640, y: 216 }, { x: 640, y: 321 }, { tone: "muted", dashed: true });
    const ask7 = tool("open a network");
    const none7 = K.text(0, 580, "no such tool", { size: 17, weight: 500, anchor: "start" });
    const no7 = ["allow", "open a network", "give a secret", "stop", "drop"].map(w => cannot(0, 0, w));
    let nx = 439 - (none7.w + 22 + no7.reduce((a, c) => a + c.w + 14, -14)) / 2;   // the shelf's middle is at x 439
    place(none7, nx, 580);
    nx += none7.w + 22;
    no7.forEach(c => { place(c, nx + c.w / 2, 580); nx += c.w + 14; });
    const found7 = svg("rect", { x: no7[1]._x - no7[1].w / 2 - 5, y: 580 - no7[1].h / 2 - 5, width: no7[1].w + 10, height: no7[1].h + 10, rx: no7[1].h / 2 + 5, fill: "none", stroke: K.tone("ink"), "stroke-width": 1.6, opacity: 0 }, K.world);
    const view7 = asCard(K.region({ x: 1035, y: 415, w: 370, h: 170 }, "import", { icon: "eye" }));   // x 850..1220, y 330..500
    const shown7 = shrink(rule(0, 0, "allow crates.io on main", "check"), 0.8);
    place(shown7, 850 + 22 + shown7.w / 2, 432);
    const grant7 = K.chip(0, 0, "grant", { tone: "you", size: 15 });
    place(grant7, shown7._x + shown7.w / 2 + 12 + grant7.w / 2, 432);

    // 7. The shelf holds the tools of the three examples. Under it, the tools that do not exist come one by one, grey
    //    and struck through, and stay where they are. A line of text arrives in the computer "request check": "open the
    //    network of main". The computer asks Silo, Silo looks on the shelf, and "open a network" is not there: the edge
    //    of main stays closed. Last, the import view of scene 2, small, with its one rule and "grant".
    //    Hero: the struck-through chips beside the shelf, and main's edge unchanged.
    scene(6);
    K.hide(all6, { at: 0 });
    K.appear(shelf7, { from: "fade", stagger: 0.06, at: 0.1 });
    K.appear(tools7, { from: "fade", stagger: 0.06, at: "<0.2" });
    K.appear([main7, rc7], { from: "fade", at: 0.2 });
    K.appear([agent7, lock7, rcp7], { from: "pop", stagger: 0.08, at: "<0.2" });
    K.appear(silo7, { from: "fade", at: "<0.1" });
    K.draw(one7, { at: "<0.2", dur: 0.5 });
    K.pulse(tools7, { tone: "ink", at: V.word(6, 0, "No tool") + 0.2 });
    along(saidGoes7, one7, 6, V.word(6, 0, "grants or destroys") - 0.6, 0.5, 1, 0);
    emerge(said7, rc7.at("left"), 6, V.word(6, 0, "grants or destroys") - 0.15, 0.6);
    K.appear(none7, { from: "fade", at: V.at(6, 1) });
    K.appear(no7[0], { from: "pop", at: V.word(6, 1, "allow a request") });
    K.appear(no7[1], { from: "pop", at: V.word(6, 1, "open a network") });
    K.appear(no7[4], { from: "pop", at: V.word(6, 1, "drop a computer") });
    K.appear([no7[2], no7[3]], { from: "pop", stagger: 0.2, at: V.word(6, 1, "drop a computer") + 0.45 });
    const tAsk7 = V.word(6, 1, "open a network") + 0.3;
    K.pulse(said7, { tone: "ink", at: tAsk7 - 0.3 });
    along(ask7, one7, 6, tAsk7, 0.5);
    K.pulse(silo7, { tone: "ink", at: tAsk7 + 0.45 });
    K.draw(look7, { at: tAsk7 + 0.5, dur: 0.5 });
    tl.to(found7, { opacity: 1, duration: 0.3, ease: "power2.out" }, T(6, tAsk7 + 1.0));
    K.pulse(none7, { tone: "ink", at: tAsk7 + 1.0 });
    K.pulse(lock7, { tone: "ink", at: tAsk7 + 1.4 });
    const tImp = Math.max(V.at(6, 2), tAsk7 + 1.6);
    K.appear(view7, { from: "right", at: tImp });
    K.appear(shown7, { from: "fade", at: Math.max(V.word(6, 2, "the rules it adds") - 0.2, tImp + 0.3) });
    K.appear(grant7, { from: "pop", at: Math.max(V.word(6, 2, "can allow something"), tImp + 0.7) });
    K.pulse(view7, { tone: "ink", at: V.word(6, 2, "importing shows them") });
    run(rcp7, 6, 0.6, V.total(6) - 2);
  },
});
