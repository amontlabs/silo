explainer({
  title: "Actions and their kinds",
  answer: "At the boundary it allows, holds or denies. Every action is granting, destructive or reversible.",
  scenes: [
    { caption: "The boundary, and the channels through it.",
      source: "The boundary is the edge of a computer. A channel is one way through it: the network, ports, files, secrets, the screen, the clipboard, repository access. None exists by default." },
    { caption: "Allow, hold or deny.",
      source: "One thing waiting at the boundary is allowed, held for your answer, or denied. Everything is denied unless a rule allows it." },
    { caption: "A hold waits for you, and always ends.",
      source: "A hold makes only one thing wait: the thing it is about. The computer keeps running. Your yes is a grant." },
    { caption: "Rules have no order.",
      source: "Rules have no order. Deny beats hold, hold beats allow, and no rule means deny." },
    { caption: "Deny: one thing. Pause and stop: the whole computer.",
      source: "Deny forbids one thing and the computer keeps running. Pause and stop act on a whole computer." },
    { caption: "Every action has a kind.",
      source: "Every action has a kind. Granting: a computer can do something it could not just before. Destructive: it ends or erases something, and that cannot be undone. Reversible: neither, so a wrong one costs only time." },
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
    const dark = K.tone("bg") === "#0d1114";
    const place = (p, x, y) => { Object.assign(p, { _x: x, _y: y }); K.gsap.set(p, { x, y }); return p; };

    // A computer: a region whose edge is drawn as a clear line. That edge is the boundary.
    const computer = (x, y, w, h) => {
      const r = K.region({ x, y, w, h }, "main", { icon: "machine" });
      r.box.setAttribute("stroke-width", 2.5);
      r.box.setAttribute("stroke", K.tone("muted"));
      return r;
    };
    // A veil over a computer's ground, under its name: pale when paused, dark when stopped.
    const veil = (r, fill) => svg("rect", { x: -r.w / 2, y: -r.h / 2, width: r.w, height: r.h, rx: 22, fill, opacity: 0 }, r, r.label && (r.icon || r.label));
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
    // Blinking from t0 to t1. A program that is still running at t1 ends with its dots lit; a paused one keeps them as they stopped.
    const run = (progs, s, t0, t1, lit = true) => progs.forEach(p => p.dots.forEach((d, i) => {
      const n = Math.max(1, Math.floor((t1 - t0 - i * 0.16) / 0.42));
      tl.fromTo(d, { opacity: 1 }, { opacity: 0.18, duration: 0.42, repeat: n - 1, yoyo: true, ease: "sine.inOut", immediateRender: false }, S(s) + t0 + i * 0.16);
      if (lit) tl.set(d, { opacity: 1 }, S(s) + t1);
    }));
    // A link drawn only as far as the boundary, its middle: the thing waits there and nothing passes.
    // Each such link is placed so that its middle lies on the boundary.
    const toEdge = (lk, s, t, dur = 0.6) => {
      Object.assign(lk, { _shown: true, _op: 1 });
      if (lk.head) K.gsap.set(lk.head, { opacity: 0 });
      tl.set(lk, { opacity: 1 }, S(s) + t);
      tl.fromTo(lk.path, { drawSVG: "0% 0%" }, { drawSVG: "0% 50%", duration: dur, ease: "power2.inOut", immediateRender: false }, S(s) + t);
    };
    // The rest of that link, once the thing is let through.
    const onward = (lk, s, t, dur = 0.6) => {
      tl.to(lk.path, { drawSVG: "0% 100%", duration: dur, ease: "power2.inOut" }, S(s) + t);
      if (lk.head) tl.to(lk.head, { opacity: 1, duration: 0.2 }, S(s) + t + dur - 0.15);
    };
    // A denied link: drawn to the boundary, then cut there in red. It has no arrowhead and never reaches its host.
    const denied = (lk, s, t, tCut) => { toEdge(lk, s, t); K.cut(lk, { at: tCut }); };
    // A chip that leaves a piece and moves along a link, stopping just inside the boundary (the link's middle).
    const toBoundary = (chip, lk, s, t, dur = 1.1) => {
      const len = lk.path.getTotalLength(), a = lk.path.getPointAtLength(0), m = lk.path.getPointAtLength(len / 2);
      const stop = Math.max(0.05, (m.x - a.x - 12 - chip.w / 2) / (m.x - a.x) * 0.5);
      const p = lk.path.getPointAtLength(len * stop);
      Object.assign(chip, { _x: p.x, _y: p.y, _shown: true, _op: 1 });
      tl.fromTo(chip, { opacity: 0, scale: 0.8 }, { opacity: 1, scale: 1, duration: 0.3, ease: "power2.out", immediateRender: false }, S(s) + t);
      tl.to(chip, { motionPath: { path: lk.path, start: 0, end: stop }, duration: dur, ease: "power2.inOut", immediateRender: false }, S(s) + t);
    };
    const side = { from: "right", to: "left" };
    // The quiet mark "proposed", beside or under what it marks, in type large enough to read.
    const proposed = (p, at) => {
      const c = K.chip(0, 0, "proposed", { tone: "muted", size: 14 }), a = p.at(at);
      return place(c, at === "right" ? a.x + 12 + c.w / 2 : a.x, at === "bottom" ? a.y + 10 + c.h / 2 : a.y);
    };

    // ---------- the world of scenes 1 and 2: the computer "main", its edge, and the outside on the right ----------
    const E = 900;                                   // main's right edge: the boundary where the channels sit
    const main = computer(560, 365, 680, 520);       // x 220..900, y 105..625
    const agent = K.actor(560, 350, "agent", "agent");
    // Silo: a thin band along the outside of the edge, all the way round.
    const band = K.region({ x: 560, y: 365, w: 680 + 24, h: 520 + 24 });
    band.box.setAttribute("fill", "none");
    band.box.setAttribute("stroke", K.tone("ink"));
    band.box.setAttribute("stroke-opacity", dark ? 0.2 : 0.16);
    band.box.setAttribute("stroke-width", 7);
    band.box.setAttribute("rx", 34);
    // Its name stands on the band, centred above the top edge and away from the channel names, so it names the band and
    // not the list.
    const silo = K.text(560, 80, "Silo", { size: 20, weight: 600 });
    // Seven places where a channel could be: faint dashed outlines sitting on the band, the edge unbroken behind them.
    const names = ["network", "ports", "files", "secrets", "screen", "clipboard", "repository"];
    const slots = names.map((n, i) => K.region({ x: E + 9, y: 170 + i * 66, w: 24, h: 46 }, null, { dashed: true }));
    slots.forEach(s => { s.box.setAttribute("stroke", K.tone("muted")); s.box.setAttribute("fill-opacity", 0); });
    const slotNames = names.map((n, i) => K.text(E + 36, 170 + i * 66, n, { size: 17, color: "muted", anchor: "start" }));

    // Scene 2: the network channel, large, on the same edge. Outside: three hosts, the rules, and you.
    const gate = K.region({ x: E, y: 365, w: 130, h: 300 }, "network", { tone: "accent", icon: "unlock" });
    const host = (y, name) => K.card(1199 + 125, y, name, { icon: "globe", w: 250 });
    const crates = host(285, "crates.io"), paste = host(425, "paste.rs"), stripe = host(565, "api.stripe.com");
    const ruleAllow = K.card(1324, 100, "Allow: crates.io", { icon: "check", w: 250 });
    const ruleHold = K.card(1324, 180, "Hold: api.stripe.com", { icon: "clock", w: 250 });
    const you = K.actor(1075, 600, "you", "you", { tone: "you" });
    const noRule = K.text(1324, 478, "no rule allows it", { size: 16, color: "muted" });
    const toCrates = K.link(agent, crates, { tone: "accent", ...side });
    const toPaste = K.link(agent, paste, { tone: "faint", arrow: false, ...side });
    const toStripe = K.link(agent, stripe, { tone: "warn", arrow: false, ...side });
    const sm = toStripe.path.getPointAtLength(toStripe.len / 2);
    const timer2 = K.icon(sm.x + 24, sm.y - 2, "timer", { size: 22, tone: "warn" });     // at the held link's tip, on the boundary
    const askLink = K.link(gate, you, { tone: "you", dashed: true, from: "bottom", to: "left" });   // from the channel's frame, not from the link
    const ask = K.chip(0, 0, "ask", { tone: "warn", icon: "message", size: 15 });
    // The rules are kept outside the computer. What cannot happen is grey and struck through, and never leaves its place.
    const keptOut = K.text(1178, 140, "kept outside the computer", { size: 15, color: "muted", anchor: "end" });
    const noChange = K.chip(560, 225, "change a rule", { tone: "muted", size: 15 });
    svg("line", { x1: -noChange.w / 2 + 10, x2: noChange.w / 2 - 10, y1: 0, y2: 0, stroke: K.tone("muted"), "stroke-width": 2 }, noChange);
    const ownRules = K.card(560, 515, "agent program's own rules", { icon: "list", w: 260 });
    const notWall = K.text(560, 572, "not a boundary", { size: 15, color: "muted" });

    // 1. The boundary. Hero: the edge with its seven named places, and the one channel that exists.
    scene(0);
    K.camera([main, band, silo, ...slotNames], { dur: 0, pad: 60 });
    K.appear(main, { from: "fade", at: V.at(0, 0) });
    K.appear(agent, { from: "pop", at: "<0.3" });
    K.pulse(main, { tone: "ink", at: V.word(0, 0, "the boundary") });
    const tSilo = V.word(0, 1, "passes through Silo");
    K.appear([band, silo], { from: "fade", at: tSilo });
    tl.fromTo(band.box, { attr: { "stroke-opacity": 0.55 } }, { attr: { "stroke-opacity": dark ? 0.2 : 0.16 }, duration: 1.4, ease: "power2.out", immediateRender: false }, S(0) + tSilo + 0.3);
    names.forEach((n, i) => K.appear([slots[i], slotNames[i]], { from: "fade", at: V.word(0, 2, n), stagger: 0 }));
    K.pulse(slots, { tone: "faint", at: V.word(0, 3, "None exists") });
    const tAdd = V.word(0, 4, "add each");
    tl.set(slots[0].box, { attr: { "stroke-dasharray": "none" } }, S(0) + tAdd);
    tl.to(slots[0].box, { attr: { stroke: K.tone("accent"), fill: K.tone("accent"), "fill-opacity": 0.9, "stroke-width": 2 }, duration: 0.45, ease: "power2.out" }, S(0) + tAdd);
    K.recolor(slotNames[0], "accent", { at: tAdd });
    K.pulse(slots[0], { tone: "accent", at: tAdd + 0.3 });

    // 2. Allow, hold or deny. Push in on the edge and the outside. Three attempts, one per outcome.
    //    Hero: the three outcomes side by side: crates.io flows, api.stripe.com waits at the edge, paste.rs is cut there.
    scene(1);
    K.hide([band, silo, ...slots, ...slotNames], { at: 0 });
    K.camera([agent, gate, ruleAllow, ruleHold, crates, paste, stripe, you, noRule, timer2, ownRules, keptOut], { pad: 50, at: 0 });
    K.appear(gate, { from: "fade", at: 0.5 });
    K.appear([ruleAllow, ruleHold], { at: 0.3, stagger: 0.15 });
    K.pulse([ruleAllow, ruleHold], { tone: "ink", at: V.word(1, 0, "rules") });
    K.appear(keptOut, { from: "fade", at: V.word(1, 1, "outside the computer") });
    K.appear(noChange, { from: "pop", at: V.word(1, 1, "nothing inside") });
    K.appear(ownRules, { at: V.word(1, 2, "Rules that live") });
    K.appear(notWall, { from: "fade", at: V.word(1, 2, "not a boundary") });
    K.hide([ownRules, notWall], { at: V.at(1, 3) - 0.4 });
    K.appear([crates, paste, stripe], { at: V.word(1, 3, "reaches"), stagger: 0.12 });
    K.pulse(gate, { tone: "accent", at: V.word(1, 3, "the boundary") });
    K.draw(toCrates, { at: V.word(1, 4, "allowed") - 0.2 });
    K.pulse(ruleAllow, { tone: "accent", at: "<" });
    K.flow(toCrates, { at: ">" });
    toEdge(toStripe, 1, V.word(1, 5, "held") - 0.2);
    K.appear(timer2, { from: "pop", at: V.word(1, 5, "held") + 0.3 });
    K.pulse(ruleHold, { tone: "warn", at: V.word(1, 5, "held") });
    K.appear(you, { from: "pop", at: V.word(1, 5, "waits") });
    K.draw(askLink, { at: V.word(1, 5, "your answer") - 0.5 });
    K.travel(ask, askLink, { land: "dock", dur: 0.9, at: ">-0.2" });
    denied(toPaste, 1, V.word(1, 6, "denied") - 0.3, V.word(1, 6, "before anything"));
    K.appear(noRule, { from: "fade", at: V.word(1, 7, "no rule allows") });

    // ---------- scene 3: the hold, then its endings side by side. Here the boundary is each computer's top edge. ----------
    // Three columns: the left ending (you answer yes), the right ending (no answer), and a smaller third (the program gives up).
    const BY = 335, HY = 250, AY = 425;              // boundary, host and agent heights: the link's middle lies on the boundary
    const you3 = K.actor(98, 95, "you", "you", { tone: "you", r: 30 });
    const askL = K.card(0, 95, "main wants to reach api.stripe.com", { icon: "message" });
    const askR = K.card(0, 95, "main wants to reach api.stripe.com", { icon: "message" });
    const cL = 154 + askL.w / 2, cR = 154 + askL.w + 36 + askR.w / 2, c3 = 154 + askL.w + 36 + askR.w + 36 + 125;
    place(askL, cL, 95); place(askR, cR, 95);
    const answers = c => [K.chip(c - 60, 160, "yes", { tone: "ink", size: 15 }), K.chip(c + 60, 160, "no", { tone: "ink", size: 15 })];
    const [yes, no] = answers(cL), [yesR, noR] = answers(cR);
    const ending = (cx, kind) => {                // kind: "yes", "no", or "gave up" (smaller, with no programs)
      const big = kind !== "gave up", bottom = big ? 650 : 520;
      const r = computer(cx, (BY + bottom) / 2, big ? 240 : 200, bottom - BY);
      const a = K.actor(cx, AY, "agent", "agent", { r: 30 });
      const h = K.card(cx, HY, "api.stripe.com", { icon: "globe", w: 200 });
      const lk = K.link(a, h, { tone: "warn", from: "top", to: "bottom", arrow: kind === "yes" });
      const timer = K.icon(cx + 24, BY - 20, "timer", { size: 22, tone: "warn" });
      const p = big ? pair(cx, 568, 612) : [];
      return { r, a, h, lk, timer, p };
    };
    const L = ending(cL, "yes"), R = ending(cR, "no"), T = ending(c3, "gave up");
    const err = K.chip(cR, 522, "error: held too long", { tone: "ink", mono: true, size: 14 });
    const sameNo = K.text(cR, 682, "your no ends the same way", { size: 16, color: "muted" });
    const gaveUp = K.text(c3, 312, "the program gave up: Silo sees it", { size: 16 });

    // 3. A hold waits for you, and always ends. Left: you say yes and the rest of the link draws and flows.
    //    Right: no answer; at Silo's limit the link is cut at the edge and an error goes back in.
    //    Third: the agent program lets go first, and Silo sees it. Hero: the endings side by side.
    scene(2);
    K.hide([main, agent, gate, ruleAllow, ruleHold, crates, paste, stripe, you, noRule, toCrates, toPaste, toStripe, timer2, askLink, ask, keptOut, noChange], { at: 0 });
    K.camera([you3, askL, askR, yes, noR, L.r, R.r, T.r, L.h, T.h, sameNo, gaveUp], { pad: 30, at: 0 });
    K.appear(L.r, { from: "fade", at: V.at(2, 0) });
    K.appear([L.a, L.h], { at: "<0.2", stagger: 0.1 });
    toEdge(L.lk, 2, V.word(2, 0, "makes only"));
    K.appear(L.timer, { from: "pop", at: V.word(2, 0, "the thing it is about") });
    K.appear(L.p, { at: V.at(2, 1) - 0.3, stagger: 0.12 });
    K.pulse(L.p, { tone: "accent", at: V.word(2, 1, "keeps running") });
    K.appear(you3, { from: "pop", at: V.at(2, 2) });
    K.appear(askL, { at: V.word(2, 2, "an ask") });
    K.appear([yes, no], { from: "pop", at: "<0.3", stagger: 0.1 });
    K.glow(yes, { tone: "ok", at: V.word(2, 3, "Your yes") });
    K.hide(L.timer, { at: V.word(2, 3, "a grant") });
    K.recolor(L.lk, "accent", { at: "<" });
    onward(L.lk, 2, V.word(2, 3, "a grant"));
    K.flow(L.lk, { at: V.word(2, 3, "a grant") + 0.6 });
    K.pulse(askL, { tone: "accent", at: V.word(2, 4, "exactly what") });
    K.appear(R.r, { from: "fade", at: V.at(2, 5) - 0.2 });
    K.appear([R.a, R.h], { at: "<0.2", stagger: 0.1 });
    K.appear(R.p, { at: "<0.2", stagger: 0.12 });
    toEdge(R.lk, 2, V.at(2, 5) + 0.5);
    K.appear(R.timer, { from: "pop", at: V.at(2, 5) + 0.9 });
    K.appear(askR, { at: V.word(2, 6, "do not answer") - 0.2 });
    K.appear([yesR, noR], { from: "pop", at: "<0.3", stagger: 0.1 });
    K.pulse(R.timer, { tone: "warn", at: V.word(2, 6, "own limit") - 0.3 });
    K.cut(R.lk, { at: V.word(2, 7, "denied") - 0.2 });
    K.appear(err, { from: "above", at: V.word(2, 7, "an error") });
    K.pulse(R.a, { tone: "ink", at: V.word(2, 7, "can read") });
    K.appear(sameNo, { from: "fade", at: V.end(2, 7) });
    K.appear(T.r, { from: "fade", at: V.at(2, 8) - 0.35 });
    K.appear([T.a, T.h], { at: "<0.15", stagger: 0.1 });
    toEdge(T.lk, 2, V.at(2, 8) - 0.1, 0.5);
    K.appear(T.timer, { from: "pop", at: V.at(2, 8) + 0.3 });
    const tGive = V.word(2, 8, "gives up");
    K.pulse(T.a, { tone: "ink", at: tGive });
    tl.to(T.lk.path, { drawSVG: "50% 50%", duration: 1.0, ease: "power2.in" }, S(2) + tGive + 0.2);   // it fades from the inside
    tl.to(T.lk, { opacity: 0, duration: 0.3 }, S(2) + tGive + 1.0);
    K.hide(T.timer, { at: tGive + 0.8 });
    K.appear(gaveUp, { from: "fade", at: V.word(2, 8, "Silo sees") - 0.2 });
    run(L.p, 2, V.at(2, 1) - 0.3, V.total(2) - 1.5);
    run(R.p, 2, V.at(2, 5), V.total(2) - 1.5);

    // ---------- scene 4: two rules, no order ----------
    const allowRule = K.card(395, 140, "allow registry.npmjs.org", { icon: "check", w: 420 });
    const denyRule = K.card(885, 140, "deny packages younger than 7 days", { icon: "ban", w: 420 });
    const order = K.text(640, 232, "deny > hold > allow", { size: 18, color: "muted", mono: true });
    const none = K.text(640, 266, "no rule = deny", { size: 18, color: "muted", mono: true });
    const alertLine = K.text(640, 300, "alert = one more action, not a decision", { size: 18, color: "muted", mono: true });
    const pAlert = proposed(alertLine, "right");
    const main4 = computer(355, 470, 470, 230);                    // x 120..590
    const agent4 = K.actor(220, 470, "agent", "agent");
    const registry = K.card(919 + 135, 470, "registry.npmjs.org", { icon: "globe", w: 270 });
    const req1 = K.link(agent4, registry, { tone: "faint", arrow: false, ...side });
    const req2 = K.link(agent4, registry, { tone: "faint", arrow: false, ...side });
    const ask1 = K.chip(0, 0, "get left-pad (2 days old)", { tone: "ink", mono: true, size: 15 });
    const ask2 = K.chip(0, 0, "get left-pad (2 days old)", { tone: "ink", mono: true, size: 15 });

    // 4. Rules have no order. The request stops at the edge; both rules light; deny wins and the link is cut there.
    //    The rules swap places and the same request gets the same answer. Hero: the same result after the swap.
    scene(3);
    K.hide([L.r, L.a, ...L.p, L.h, L.lk, R.r, R.a, ...R.p, R.h, R.lk, R.timer, L.timer, T.r, T.a, T.h, T.lk, T.timer, err, you3, askL, askR, yes, no, yesR, noR, sameNo, gaveUp], { at: 0 });
    K.camera(null, { at: 0 });
    K.appear([allowRule, denyRule], { at: V.at(3, 0), stagger: 0.15 });
    K.appear(main4, { from: "fade", at: V.end(3, 0) - 0.3 });
    K.appear([agent4, registry], { at: "<0.2", stagger: 0.1 });
    toEdge(req1, 3, V.end(3, 0) + 0.3);
    toBoundary(ask1, req1, 3, V.end(3, 0) + 0.7);
    K.pulse(allowRule, { tone: "ink", at: V.word(3, 1, "allows") });
    K.pulse(denyRule, { tone: "ink", at: V.word(3, 1, "denies") });
    K.glow(denyRule, { tone: "ink", at: V.word(3, 1, "deny wins") });
    K.cut(req1, { at: "<0.2" });
    K.appear(order, { from: "fade", at: V.at(3, 2) });
    K.appear(none, { from: "fade", at: V.word(3, 3, "no rule") });
    const tSwap = V.word(3, 4, "the order") - 0.2;
    K.moveTo(allowRule, 885, 140, { at: tSwap });
    K.moveTo(denyRule, 395, 140, { at: "<" });
    K.hide([ask1, req1], { at: "<" });
    toEdge(req2, 3, tSwap + 1.0);
    toBoundary(ask2, req2, 3, tSwap + 1.6);
    K.pulse([allowRule, denyRule], { tone: "ink", at: tSwap + 2.6 });
    K.cut(req2, { at: ">" });
    K.appear(alertLine, { from: "fade", at: V.at(3, 5) });
    K.appear(pAlert, { from: "pop", at: V.word(3, 5, "never a decision") });

    // ---------- scene 5: three copies of main ----------
    const xs = [240, 640, 1040];
    const mains = xs.map(x => computer(x, 443, 340, 334));       // y 276..610: the top edge is the boundary
    const progs = xs.map(x => pair(x, 410, 470));
    const paste5 = K.card(240, 130, "paste.rs", { icon: "globe", w: 200 });
    const deny5 = K.link(progs[0][0], paste5, { tone: "faint", from: "top", to: "bottom", arrow: false });
    const tags = ["deny", "pause", "stop"].map((t, i) => K.text(xs[i], 648, t, { size: 20, weight: 600 }));
    const pale = veil(mains[1], K.tone("surfaceHi"));
    const shut = veil(mains[2], dark ? "#000000" : "#14273a");
    const resume = K.chip(640, 576, "resume", { tone: "ink", icon: "play", size: 15 });
    const disk = K.chip(1040, 462, "disk", { tone: "ink", icon: "database", size: 15 });
    const start = K.chip(1040, 576, "start", { tone: "ink", icon: "play", size: 15 });

    // 5. Deny acts on one thing; pause and stop on the whole computer. Each label comes with its change.
    //    Hero: the three computers side by side.
    scene(4);
    K.hide([allowRule, denyRule, order, none, alertLine, pAlert, main4, agent4, registry, req2, ask2], { at: 0 });
    K.appear(mains[0], { from: "fade", at: V.at(4, 0) - 0.2 });
    K.appear(progs[0], { at: "<0.2", stagger: 0.1 });
    K.appear(paste5, { at: "<0.2" });
    const tCut = V.word(4, 0, "forbids") + 0.2;
    denied(deny5, 4, V.word(4, 0, "forbids") - 0.4, tCut);
    K.appear(tags[0], { from: "fade", at: tCut });
    K.pulse(progs[0], { tone: "accent", at: V.word(4, 0, "keeps running") });
    K.appear(mains[1], { from: "fade", at: V.word(4, 1, "Pause") - 0.1 });
    K.appear(progs[1], { at: "<0.2", stagger: 0.1 });
    K.appear(mains[2], { from: "fade", at: V.word(4, 1, "stop") - 0.1 });
    K.appear(progs[2], { at: "<0.2", stagger: 0.1 });
    const tFreeze = V.word(4, 2, "freezes"), tStop = V.word(4, 3, "ends the programs");
    tl.to(pale, { opacity: dark ? 0.75 : 0.85, duration: 0.6, ease: "power2.out" }, S(4) + tFreeze);
    K.dim(progs[1], { to: 0.4, at: tFreeze });
    K.appear(tags[1], { from: "fade", at: tFreeze });
    K.appear(resume, { from: "pop", at: V.word(4, 2, "resume") });
    tl.to(shut, { opacity: dark ? 0.6 : 0.42, duration: 0.6, ease: "power2.out" }, S(4) + tStop);
    K.hide(progs[2], { at: tStop });
    K.appear(tags[2], { from: "fade", at: tStop });
    if (!dark) tl.to(mains[2].label, { attr: { fill: "#eef0ee" }, duration: 0.6 }, S(4) + tStop);
    K.appear(disk, { from: "pop", at: V.word(4, 4, "disk") });
    K.appear(start, { from: "pop", at: ">" });
    run(progs[0], 4, V.at(4, 0), V.total(4) - 1.5);
    run(progs[1], 4, V.word(4, 1, "Pause"), tFreeze, false);
    run(progs[2], 4, V.word(4, 1, "stop"), tStop, false);

    // ---------- scene 6: every action has a kind ----------
    // The three headings share one colour: amber stays the colour of a hold.
    const col = (x, title, items) => ({
      head: K.text(x, 100, title, { size: 26, weight: 600, color: "ink" }),
      chips: items.map((s, i) => K.chip(x, 165 + i * 54, s, { tone: "ink", size: 17 })),
    });
    const G = col(235, "Granting", ["allow", "resume", "start", "fork", "give a secret", "open a port or any other channel", "let a classifier read a record"]);
    const Dst = col(640, "Destructive", ["stop", "restore", "drop"]);
    const Rv = col(1045, "Reversible", ["alert", "checkpoint", "hold", "deny", "pause", "take back a secret", "close a network or any other channel"]);
    // In the granting column the words "or any other channel" are quieter than the rest of their chip: they are what is
    // proposed. The reversible chip "close a network or any other channel" stays in one tone: all of it is proposed.
    const quiet = (c, plain, rest) => {
      const t = c.label; t.textContent = "";
      svg("tspan", {}, t).textContent = plain;
      svg("tspan", { fill: K.tone("muted") }, t).textContent = rest;
    };
    quiet(G.chips[5], "open a port ", "or any other channel");
    const yesOnly = K.text(235, 568, "your yes to an ask: only you", { size: 17, color: "muted" });
    const d1 = K.text(640, 340, "each ends the running programs;", { size: 17, color: "muted" });
    const d2 = K.text(640, 368, "the disk is not lost", { size: 17, color: "muted" });
    const pStart = proposed(G.chips[2], "right"), pOpen = proposed(G.chips[5], "right"), pYes = proposed(yesOnly, "right");
    const pD = proposed(d2, "bottom"), pClose = proposed(Rv.chips[6], "bottom");   // pD: centred under both lines
    const pTake = proposed(Rv.chips[5], "right");
    const r1 = K.text(1045, 588, "if these two are reversible,", { size: 17, color: "muted" });
    const r2 = K.text(1045, 616, "a precaution can do them", { size: 17, color: "muted" });

    // 6. Three columns, each as the voice names it. Hero: the three columns complete.
    scene(5);
    K.camera([G.head, Rv.head, ...G.chips, ...Rv.chips, yesOnly, pYes, pOpen, pClose, pTake, r2], { pad: 50, at: 0 });
    K.hide([...mains, ...progs.flat(), paste5, deny5, ...tags, resume, disk, start], { at: V.word(5, 0, "has a kind") - 0.3 });
    K.appear(G.head, { from: "fade", at: V.at(5, 1) });
    K.appear(G.chips, { at: "<0.3", stagger: 0.12 });
    K.appear([pStart, pOpen], { from: "pop", at: ">", stagger: 0.1 });
    K.appear(yesOnly, { from: "fade", at: V.end(5, 1) - 0.4 });
    K.appear(pYes, { from: "pop", at: ">-0.2" });
    K.appear(Dst.head, { from: "fade", at: V.at(5, 2) });
    K.appear(Dst.chips, { at: "<0.3", stagger: 0.12 });
    K.pulse(Dst.chips, { tone: "ink", at: V.word(5, 3, "Stop, restore and drop") });
    K.appear(d1, { from: "fade", at: V.word(5, 3, "each ends") });
    K.appear(d2, { from: "fade", at: V.at(5, 4) });
    K.appear(pD, { from: "pop", at: ">-0.2" });
    K.appear(Rv.head, { from: "fade", at: V.at(5, 5) });
    K.appear(Rv.chips, { at: "<0.3", stagger: 0.12 });
    K.pulse(Rv.head, { tone: "muted", at: V.word(5, 6, "costs only time") });
    K.appear(pTake, { from: "pop", at: V.word(5, 7, "taking back a secret") });
    K.appear(pClose, { from: "pop", at: V.word(5, 7, "closing a network") });
    K.appear(r1, { from: "fade", at: V.at(5, 8) });
    K.appear(r2, { from: "fade", at: "<0.2" });
    K.pulse([G.head, Dst.head, Rv.head], { tone: "ink", at: V.word(5, 9, "the kind") });
  },
});
