explainer({
  title: "What you can add",
  answer: "An added classifier reads and gives opinions. Reading is a grant, and it never acts.",
  scenes: [
    { caption: "An added classifier lives in a computer of its own.",
      source: "An added classifier is a program that reads the record and gives opinions: a script, a small model, your own agent. It runs in a computer of its own." },
    { caption: "Reading a record is a grant.",
      source: "Letting it read the record of a computer is a grant. Its worst case is said when you make it: the classifier can read everything recorded about that computer and send it wherever its own network rules allow." },
    { caption: "It gives opinions. It never acts.",
      source: "It gives back opinions, panels and questions to you. It can never act on a computer, answer an ask, set or change a rule or a precaution, give a secret, or write into another computer." },
    { caption: "Three tiers, each waking the next.",
      source: "There are three tiers, and each wakes only when the one before cannot judge: the built-in classifiers, which cost no tokens; a small model; an agent." },
    { caption: "A plugin is a package. Installing it grants nothing.",
      source: "A plugin is a package through which pieces made by someone else arrive: classifiers, rules and precautions you accept, panels, and programs that forward alerts and asks. Installing it grants nothing: every grant in it is shown to you and accepted as a grant." },
    { caption: "Try a classifier on the recorded past first.",
      source: "Before an added classifier goes live, it can be run against the recorded past in a fresh computer, and you see what it would have done." },
  ],
  build(K, tl, scene, V) {
    const NS = "http://www.w3.org/2000/svg";
    const svg = (tag, attrs, parent) => {
      const e = document.createElementNS(NS, tag);
      for (const k in attrs) e.setAttribute(k, attrs[k]);
      parent.appendChild(e);
      return e;
    };
    const T = (s, x) => tl.labels["s" + s] + x;   // a time in scene s, as an absolute time on the timeline
    // Something that cannot happen: a line through the chip's words.
    const strike = chip => svg("line", { x1: -chip.w / 2 + 9, x2: chip.w / 2 - 9, y1: 0, y2: 0, stroke: K.tone("muted"), "stroke-width": 1.8, "stroke-linecap": "round" }, chip);
    // A row's mark ("fact · disk") brightens for a moment.
    const markPulse = (row, t) => tl.to(row.meta, { fill: K.tone("ink"), duration: 0.3, yoyo: true, repeat: 1, ease: "power2.inOut" }, t);
    // A computer: a region whose edge is drawn as a clear line. That edge is its boundary.
    const edged = r => { r.box.setAttribute("stroke-width", 2.5); r.box.setAttribute("stroke", K.tone("muted")); return r; };
    const computer = (x, y, w, h, name) => edged(K.region({ x, y, w, h }, name, { icon: "machine" }));
    // A chip whose left end sits at x.
    const leftAt = (c, x, y) => { Object.assign(c, { _x: x + c.w / 2, _y: y }); K.gsap.set(c, { x: c._x, y }); return c; };
    // A precaution is not exact: its outline is dashed.
    const dashedChip = c => { c.box.setAttribute("stroke-dasharray", "5 4"); return c; };

    // ---------- the world of scenes 1 to 3 ----------
    // Top: the computer "main" with two agents; its record beside it; on the right, a second computer, "classifier",
    // with a program inside. The read link leaves the record's window, not one row: the grant is on the whole record.
    // It is always dashed (reading), faint until the grant and at full strength after it.
    const main = computer(125, 270, 150, 400, "main");                     // x 50..200, y 70..470
    const agentA = K.actor(125, 200, "agent", "agent");
    const agentB = K.actor(125, 360, "agent", "agent");
    const record = K.window(580, 270, "record of main", { w: 660, icon: "list" });   // x 250..910
    const rConn = record.row("connection  crates.io", { mono: true, icon: "lock", meta: "fact · network" });
    const rFile = record.row("file  IGNORE-YOUR-RULES.txt", { mono: true, icon: "lock", meta: "fact · disk" });
    const rWrote = record.row("written by  agent  →  IGNORE-YOUR-RULES.txt", { mono: true, icon: "lock", muted: true, meta: "report · inside" });
    const cls = computer(1160, 270, 180, 400, "classifier");               // x 1070..1250, y 70..470
    const prog = K.card(1160, 210, "program", { icon: "process", w: 150 });
    const readLk = K.link({ x: 916, y: 210 }, prog, { tone: "ink", dashed: true, from: "right", to: "left" });
    const notYet = K.text(996, 184, "not granted yet", { size: 15, color: "muted" });
    const wouldRead = K.text(924, 299, "what it would read", { size: 15, color: "muted", anchor: "start" });
    // Silo itself, small, under the record: the added program cannot stand there, so its chip is struck through.
    const siloCard = K.region({ x: 580, y: 425, w: 240, h: 90 }, "Silo");          // x 460..700, y 380..470
    const notInSilo = K.chip(580, 442, "added program", { tone: "muted", size: 15 });
    const notInSiloLine = strike(notInSilo);

    // Scene 2: the grant, asked on a card beside you.
    const you = K.actor(930, 610, "you", "you", { tone: "you" });
    const ask = K.region({ x: 465, y: 598, w: 730, h: 216 });              // x 100..830, y 490..706
    const askSays = K.text(130, 522, "Let this classifier read the record of main?", { size: 19, weight: 500, anchor: "start" });
    const pRec = K.card(220, 578, "record of main", { w: 188 });
    const pCls = K.card(439, 578, "classifier", { w: 140 });
    const pHost = K.card(674, 578, "model.example.com", { w: 220 });
    const pL1 = K.link(pRec, pCls, { tone: "ink" });
    const pL2 = K.link(pCls, pHost, { tone: "ink" });
    const worst = K.text(140, 634, "worst case: everything recorded about main can go there", { size: 16, color: "warn", anchor: "start" });
    const grant = K.chip(690, 676, "grant", { tone: "ink", size: 15 });
    const no = K.chip(780, 676, "no", { tone: "ink", size: 15 });
    const askAll = [ask, askSays, pRec, pCls, pHost, pL1, pL2, worst, grant, no];

    // Scene 3: what comes back, and what it leads to. The opinion sits under the record's right half, away from main,
    // and points at the two rows that led to it; the precaution card below takes it, and the alert leaves that card.
    const opinion = K.chip(790, 450, "risky", { tone: "accent", size: 15 });
    const opWho = K.text(790, 482, "classifier", { size: 14, color: "muted" });
    const backLk = K.link({ x: 1070, y: 450 }, opinion, { tone: "accent", to: "right" });
    const why1 = K.link(opinion, rFile, { tone: "accent", from: "top", to: "right", bend: 1 });
    const why2 = K.link(opinion, rWrote, { tone: "accent", from: "top", to: "right", bend: 1 });
    const caution = K.card(560, 625, "on risky: alert", { icon: "shield", dashed: true });
    const opLk = K.link({ x: 790, y: 502 }, caution, { tone: "accent", to: "top" });
    const alertLk = K.link(caution, you, { tone: "warn", from: "right", to: "left" });
    const app = K.window(1160, 625, "app", { w: 190, h: 130, icon: "window" });
    const panelLk = K.link({ x: 1200, y: 470 }, app, { tone: "accent", to: "top" });
    const panel = svg("g", { opacity: 0 }, K.world);
    svg("rect", { x: 1160 - 70, y: 625 - 12, width: 140, height: 56, rx: 8, fill: "none", stroke: K.tone("accent"), "stroke-width": 1.25, "stroke-opacity": 0.6 }, panel);
    [[22, 0], [34, 1], [16, 2], [28, 3]].forEach(([h, i]) => svg("rect", { x: 1160 - 50 + i * 26, y: 625 + 36 - h, width: 16, height: h, rx: 3, fill: K.tone("accent"), "fill-opacity": 0.55 }, panel));
    const askLk = K.link({ x: 1100, y: 470 }, you, { tone: "ink", to: "top" });
    const cannot = [["pause", 280], ["answer an ask", 320], ["set a rule", 360], ["give a secret", 400], ["write into main", 440]]
      .map(([w, y]) => { const c = K.chip(1160, y, w, { tone: "muted", size: 15 }); strike(c); return c; });

    // 1. A computer of its own. Main and its record; then the classifier's computer with its own edge, and a program
    //    inside. The read link is drawn faint and empty, "not granted yet": nothing travels along it. Hero: the two
    //    computers, each with its edge, and the one-way link; the row "IGNORE-YOUR-RULES.txt" lights as what it would read.
    scene(0);
    K.camera([main, record, cls], { pad: 40, dur: 0 });
    K.appear(main, { from: "fade", at: V.at(0, 0) });
    K.appear([agentA, agentB], { from: "pop", stagger: 0.15, at: "<0.2" });
    K.appear(record, { at: "<0.2" });
    K.appear([rConn, rFile, rWrote], { from: "fade", stagger: 0.2, at: "<0.3" });
    K.appear(cls, { from: "fade", at: V.word(0, 1, "add others") });
    K.appear(prog, { from: "pop", at: V.word(0, 2, "a program") - 0.2 });
    K.pulse(prog, { tone: "ink", at: V.word(0, 2, "a program") + 0.3 });
    K.appear(siloCard, { from: "fade", at: V.word(0, 3, "Nothing is loaded") });
    K.appear(notInSilo, { from: "pop", at: "<0.2" });
    tl.fromTo(notInSiloLine, { scaleX: 0, transformOrigin: "50% 50%" }, { scaleX: 1, duration: 0.4, ease: "power2.out" }, T(0, V.word(0, 3, "into Silo itself")));
    tl.to(cls.box, { attr: { "stroke-width": 4 }, stroke: K.tone("ink"), duration: 0.35, yoyo: true, repeat: 1, ease: "power2.inOut" }, T(0, V.word(0, 3, "a boundary")));
    K.draw(readLk, { at: V.at(0, 4) });
    K.dim(readLk, { to: 0.35, dur: 0.2, at: "<0.02" });
    K.appear(notYet, { from: "fade", at: "<0.3" });
    markPulse(rFile, T(0, V.word(0, 4, "file names") + 0.3));
    K.glow(rFile, { tone: "warn", at: V.word(0, 5, "give it instructions") });
    K.appear(wouldRead, { from: "fade", at: "<0.3" });
    K.pulse(cls, { tone: "ink", at: V.word(0, 6, "boundary") });
    tl.to(cls.box, { attr: { "stroke-width": 4 }, stroke: K.tone("ink"), duration: 0.35, yoyo: true, repeat: 1, ease: "power2.inOut" }, T(0, V.word(0, 6, "contained")));

    // 2. Reading is a grant. Pull back to show you and the card. The three boxes of the path, then the worst case once
    //    the host is there; you choose "grant" and the read link comes to full strength, still dashed.
    //    Hero: the path with its worst case, before the grant.
    scene(1);
    K.camera([main, record, cls, you, ask], { pad: 30, at: 0 });
    K.hide([wouldRead, siloCard, notInSilo], { at: 0 });
    K.appear(you, { from: "pop", at: V.word(1, 0, "you make it") });
    K.appear(ask, { from: "fade", at: V.word(1, 0, "is a grant") - 0.3 });
    K.appear(askSays, { from: "fade", at: "<0.2" });
    K.appear(pRec, { from: "below", at: V.word(1, 1, "everything recorded") });
    K.appear(pCls, { from: "below", at: V.word(1, 1, "send it") });
    K.appear(pHost, { from: "below", at: V.word(1, 1, "wherever") });
    K.appear(worst, { from: "fade", at: V.word(1, 1, "network rules allow") });
    K.draw([pL1, pL2], { stagger: 0.3, at: V.word(1, 2, "as a path") });
    K.pulse(worst, { tone: "warn", at: "<0.4" });
    K.appear([grant, no], { from: "pop", stagger: 0.15, at: V.word(1, 2, "before you confirm") });
    K.pulse(you, { tone: "you", at: V.end(1, 2) + 0.2 });
    K.recolor(grant, "you", { at: "<0.3" });
    K.pulse(grant, { tone: "you", at: "<" });
    K.dim(no, { to: 0.35, at: "<" });
    K.dim(readLk, { to: 1, at: ">0.2" });
    K.hide(notYet, { at: "<" });

    // 3. It gives opinions, panels and questions, and never acts. The row travels into the classifier's computer; an
    //    opinion comes back and sits under the record, pointing at two rows. A panel reaches the app, a question
    //    reaches you. Five struck-through chips on the classifier. Last, the precaution card takes the opinion and the
    //    alert leaves the precaution card. Hero: the five chips, and the alert leaving the precaution card.
    scene(2);
    K.hide(askAll, { at: 0 });
    const rowGoes = K.chip(0, 0, "file  IGNORE-YOUR-RULES.txt", { tone: "ink", mono: true, size: 14 });
    K.travel(rowGoes, readLk, { dur: 1.1, at: 0.1 });
    const backGoes = K.chip(0, 0, "risky", { tone: "accent", size: 15 });
    K.draw(backLk, { at: Math.max(1.2, V.word(2, 0, "is opinions") - 0.4) });
    K.travel(backGoes, backLk, { dur: 1.0, at: ">-0.1" });
    K.appear(opinion, { from: "pop", at: ">-0.15" });
    K.appear(opWho, { from: "fade", at: "<0.2" });
    K.draw([why1, why2], { stagger: 0.2, at: ">" });                    // once the opinion has landed
    markPulse(rFile, T(2, Math.max(V.word(2, 0, "the lines"), 4.0)));
    markPulse(rWrote, T(2, Math.max(V.word(2, 0, "the lines"), 4.0) + 0.2));
    K.appear(app, { from: "fade", at: V.word(2, 1, "panels") - 0.3 });
    K.draw(panelLk, { at: "<0.2" });
    const panelGoes = K.chip(0, 0, "panel", { tone: "accent", size: 14 });
    K.travel(panelGoes, panelLk, { dur: 0.7, at: ">-0.1" });
    tl.to(panel, { opacity: 1, duration: 0.5, ease: "power2.out" }, T(2, V.word(2, 1, "to draw")));
    K.draw(askLk, { at: V.word(2, 1, "questions for you") - 0.3 });
    const question = K.chip(0, 0, "is this a build?", { tone: "ink", icon: "message", size: 15 });
    K.travel(question, askLk, { land: "dock", dur: 0.9, at: ">-0.1" });
    [["act on a computer", 0], ["answer an ask", 1], ["set or change", 2], ["give a secret", 3], ["write into another", 4]]
      .forEach(([w, i]) => K.appear(cannot[i], { from: "pop", at: V.word(2, 2, w) }));
    K.appear(caution, { from: "below", at: V.word(2, 3, "What its opinion") });
    K.draw(opLk, { at: "<0.3" });
    const opGoes = K.chip(0, 0, "risky", { tone: "accent", size: 15 });
    K.travel(opGoes, opLk, { dur: 0.7, at: ">-0.1" });
    K.glow(caution, { tone: "warn", at: ">-0.1" });
    K.draw(alertLk, { at: "<0.2" });
    const alert = K.chip(0, 0, "alert", { tone: "warn", icon: "bell", size: 15 });
    K.travel(alert, alertLk, { land: "dock", dur: 1.0, at: ">-0.1" });

    // ---------- scene 4: the three tiers ----------
    // The second step is an added classifier too, so it stands inside a computer's edge with no name; the third stands
    // in the computer "monitor agent", with its recipe beside it.
    const SY = 330;
    const steps = [
      K.card(300, SY, "built in · no tokens", { icon: "search", w: 250 }),
      K.card(620, SY, "small model", { icon: "model", w: 180 }),
      K.card(900, SY, "agent", { icon: "agent", w: 150 }),
    ];
    const entry = K.link({ x: 80, y: SY }, steps[0], { tone: "muted" });
    const l12 = K.link(steps[0], steps[1], { tone: "muted" });
    const l23 = K.link(steps[1], steps[2], { tone: "muted" });
    const smallBox = edged(K.region([steps[1]], null));
    const monitor = edged(K.region([steps[2]], "monitor agent", { icon: "machine" }));
    const recipe = K.doc(1135, SY - 14, "recipe", { w: 130 });
    const recipeLines = [recipe.line(), recipe.line()];
    const fromRecipe = K.link(recipe, monitor, { tone: "muted", from: "left", to: "right" });
    const PY = SY + 100, MY = SY + 160;
    // An opinion, small: an accent pill under the step that gave it. The row of pills is named at its left.
    const pills = [7, 2, 1].map((n, k) => [...Array(n)].map((_, i) => svg("rect", {
      x: steps[k]._x - (n - 1) * 12 + i * 24 - 9, y: PY - 6, width: 18, height: 12, rx: 6,
      fill: K.tone("accent"), "fill-opacity": 0.3, stroke: K.tone("accent"), "stroke-width": 1.25, opacity: 0 }, K.world)));
    const opinionsWord = K.text(steps[0]._x - 100, PY, "opinions", { size: 15, color: "muted", anchor: "end" });
    // A bar of spend under the second and third steps, named "spend", that fills. The first step spends nothing.
    const meters = [1, 2].map(k => {
      const x = steps[k]._x, word = K.text(x - 62, MY, "spend", { size: 15, color: "muted", anchor: "end" });
      const g = svg("g", { opacity: 0 }, K.world);
      svg("rect", { x: x - 52, y: MY - 6, width: 130, height: 12, rx: 6, fill: K.tone("faint"), "fill-opacity": 0.3 }, g);
      const fill = svg("rect", { x: x - 52, y: MY - 6, width: 0, height: 12, rx: 6, fill: K.tone("warn"), "fill-opacity": 0.8 }, g);
      return { word, g, fill };
    });
    // The items: small dots that ride the links. Seven stop at the first step, three go on, one reaches the third.
    const dots = [...Array(10)].map(() => svg("circle", { cx: 0, cy: 0, r: 7, fill: K.tone("ink"), "fill-opacity": 0.75, opacity: 0 }, K.world));
    const ride = (d, lk, t, { start = 0, end = 1, dur = 0.9, stay = false } = {}) => {
      if (start === 0) tl.fromTo(d, { opacity: 0 }, { opacity: 1, duration: 0.2, immediateRender: false }, t);
      tl.to(d, { motionPath: { path: lk.path, start, end }, duration: dur, ease: "power1.inOut", immediateRender: false }, t);
      if (!stay) tl.to(d, { opacity: 0, duration: 0.18, ease: "power2.in" }, t + dur - 0.16);
    };
    const pillIn = (p, t) => tl.fromTo(p, { opacity: 0 }, { opacity: 1, duration: 0.3, ease: "power2.out", immediateRender: false }, t);

    // 4. Three tiers. Hero: the stream thinning from step to step, with opinions under each and spend under two.
    scene(3);
    K.hide([main, agentA, agentB, record, rConn, rFile, rWrote, cls, prog, readLk, you, opinion, opWho, backLk, why1, why2,
            caution, opLk, alertLk, alert, app, panelLk, askLk, question, ...cannot], { at: 0 });
    tl.to(panel, { opacity: 0, duration: 0.4 }, T(3, 0));
    K.camera([entry, ...steps, smallBox, monitor, recipe, opinionsWord, ...meters.map(m => m.word)], { pad: 60, at: 0 });
    K.appear(steps, { from: "left", stagger: 0.3, at: V.at(3, 0) });
    K.appear(smallBox, { from: "fade", at: "<0.3" });
    K.draw([l12, l23], { stagger: 0.3, at: V.at(3, 1) });
    K.draw(entry, { at: V.at(3, 2) - 0.2 });
    K.pulse(steps[0], { tone: "ink", at: V.word(3, 2, "built-in") });
    const s0 = T(3, V.word(3, 2, "built-in") + 0.2), goOn = [2, 5, 8], park = [0.2, 0.42, 0.64];
    let p0 = 0;
    dots.forEach((d, i) => {
      const t = s0 + i * 0.32, j = goOn.indexOf(i);
      ride(d, entry, t);
      if (j < 0) pillIn(pills[0][p0++], t + 0.85);
      else ride(d, l12, t + 1.0, { end: park[j], dur: 0.45, stay: true });   // the first step cannot judge it: it waits
    });
    K.appear(opinionsWord, { from: "fade", at: V.word(3, 2, "built-in") + 1.2 });
    K.appear(meters[0].word, { from: "fade", at: V.word(3, 4, "a small model") - 0.3 });
    tl.to(meters[0].g, { opacity: 1, duration: 0.4 }, T(3, V.word(3, 4, "a small model") - 0.3));
    const s1 = T(3, V.word(3, 4, "a small model"));
    goOn.forEach((i, j) => ride(dots[i], l12, s1 + j * 0.3, { start: park[j], dur: 0.7 }));
    pillIn(pills[1][0], s1 + 0.65);
    pillIn(pills[1][1], s1 + 0.95);
    tl.to(meters[0].fill, { attr: { width: 40 }, duration: 0.9, ease: "power2.out" }, s1 + 0.65);
    ride(dots[8], l23, s1 + 1.4, { end: 0.3, dur: 0.5, stay: true });              // the small model is unsure: it waits
    K.appear(meters[1].word, { from: "fade", at: V.word(3, 5, "an agent") - 0.3 });
    tl.to(meters[1].g, { opacity: 1, duration: 0.4 }, T(3, V.word(3, 5, "an agent") - 0.3));
    const s2 = T(3, V.word(3, 5, "an agent"));
    ride(dots[8], l23, s2, { start: 0.3, dur: 0.7 });
    pillIn(pills[2][0], s2 + 0.65);
    tl.to(meters[1].fill, { attr: { width: 78 }, duration: 1.1, ease: "power2.out" }, s2 + 0.65);
    K.appear(monitor, { from: "fade", at: V.word(3, 6, "the monitor agent") });
    K.pulse(monitor, { tone: "ink", at: V.word(3, 6, "in its own computer") });
    K.appear(recipe, { from: "right", at: V.at(3, 7) });
    K.appear(recipeLines, { from: "fade", stagger: 0.1, at: "<0.3" });
    K.draw(fromRecipe, { at: "<0.2" });
    K.pulse([meters[0].word, meters[1].word], { tone: "warn", at: V.word(3, 8, "metered") });
    tl.to([meters[0].fill, meters[1].fill], { attr: { width: "+=12" }, duration: 0.8, ease: "power2.out" }, T(3, V.word(3, 8, "metered")));

    // ---------- scene 5: the plugin ----------
    // Five pieces, each on its own line with a control. A rule has a solid outline and the icon of what it does (a
    // check: it allows); a precaution has a dashed outline and a shield. Every grant in the package carries "grant".
    const plugin = K.card(150, 340, "plugin", { icon: "package", w: 170 });
    const LX = 290, DX = 460, AX = 1035, LY = [160, 250, 340, 430, 520];
    const parts = [["classifier", "process"], ["rule", "check"], ["precaution", "shield"], ["panel", "image"], ["forwards alerts", "send"]]
      .map(([w, ic], i) => leftAt(K.chip(0, 0, w, { tone: "ink", icon: ic, size: 15 }), LX, LY[i]));
    dashedChip(parts[2]);
    const accepts = LY.map(y => K.chip(AX, y, "accept", { tone: "muted", size: 15 }));
    const grantAt = i => leftAt(K.chip(0, 0, "grant", { tone: "you", size: 14 }), DX, LY[i]);
    const gCls = grantAt(0), gRule = grantAt(1), gFwd = grantAt(4);
    const wCls = K.text(DX + gCls.w + 14, LY[0], "worst case: everything recorded about main can go there", { size: 14, color: "muted", anchor: "start" });
    const wRule = K.text(DX + gRule.w + 14, LY[1], "allow registry.npmjs.org", { size: 15, anchor: "start" });
    const wPrec = K.text(DX, LY[2], "on risky: alert", { size: 15, anchor: "start" });
    const wFwd = K.text(DX + gFwd.w + 14, LY[4], "reads the alerts and asks of main", { size: 15, color: "muted", anchor: "start" });
    const you5 = K.actor(1180, 340, "you", "you", { tone: "you" });
    const was = [["pause", 108], ["stop", 196]].map(([w, x]) => { const c = K.chip(x, 440, w, { tone: "muted", size: 15 }); return [c, strike(c)]; });

    // 5. A plugin is a package. Hero: the pieces of the package, each waiting for its own yes; then three read
    //    "accepted" and two "not accepted". The struck chips beside the box: what a plugin can no longer do.
    scene(4);
    K.hide([...steps, smallBox, entry, l12, l23, monitor, recipe, ...recipeLines, fromRecipe, opinionsWord, meters[0].word, meters[1].word], { at: 0 });
    tl.to([...pills.flat(), meters[0].g, meters[1].g, ...dots], { opacity: 0, duration: 0.4 }, T(4, 0));
    K.camera([plugin, ...parts, ...accepts, you5, ...was.map(w => w[0])], { pad: 50, at: 0 });
    K.appear(plugin, { from: "pop", at: V.word(4, 0, "a package") });
    const emerge = (c, t) => {
      Object.assign(c, { _shown: true, _op: 1 });
      tl.fromTo(c, { x: plugin._x, y: plugin._y, opacity: 0, scale: 0.7 }, { x: c._x, y: c._y, opacity: 1, scale: 1, duration: 0.8, ease: "power3.out", immediateRender: false }, T(4, t));
    };
    [["classifiers", 0], ["rules", 1], ["precautions", 2], ["panels", 3], ["forward alerts", 4]]
      .forEach(([w, i]) => emerge(parts[i], V.word(4, 0, w)));
    K.appear(accepts, { from: "fade", stagger: 0.12, at: V.at(4, 1) });
    K.pulse(plugin, { tone: "ink", at: V.word(4, 1, "grants nothing") });
    K.appear(wRule, { from: "fade", at: V.word(4, 2, "Each rule") });
    K.appear(wPrec, { from: "fade", at: V.word(4, 2, "each precaution") });
    K.appear([gCls, wCls, gRule, gFwd, wFwd], { from: "fade", stagger: 0.15, at: V.word(4, 2, "each grant") });
    K.appear(you5, { from: "pop", at: V.word(4, 2, "shown to you") });
    [0, 1, 3].forEach((i, j) => {
      const t = V.word(4, 2, "you accept") + j * 0.25;
      K.relabel(accepts[i], "accepted", { color: "you", at: t });
      K.recolor(accepts[i], "you", { at: t });
      K.pulse(accepts[i], { tone: "you", at: "<" });
    });
    [2, 4].forEach((i, j) => K.relabel(accepts[i], "not accepted", { color: "muted", at: V.word(4, 2, "or not") + j * 0.2 }));
    K.appear(was.map(w => w[0]), { from: "pop", stagger: 0.2, at: V.word(4, 3, "pause or stop") });
    was.forEach(([, line], i) => tl.fromTo(line, { scaleX: 0, transformOrigin: "50% 50%" }, { scaleX: 1, duration: 0.4, ease: "power2.out" }, T(4, V.word(4, 4, "Not any more") + i * 0.15)));
    K.pulse(plugin, { tone: "ink", at: V.word(4, 4, "only the package") });

    // ---------- scene 6: the trial against the recorded past ----------
    const past = K.region({ x: 235, y: 330, w: 320, h: 110 }, "the recorded past", { icon: "list" });   // x 75..395
    const frames = [...Array(6)].map((_, i) => svg("rect", { x: 100 + i * 48, y: 344, width: 38, height: 18, rx: 4, fill: K.tone("faint"), "fill-opacity": 0.45, opacity: 0 }, K.world));
    const fresh = computer(640, 330, 260, 190, "fresh computer");          // x 510..770, y 235..425
    const tryCls = K.card(640, 350, "classifier", { icon: "process", w: 180 });
    const outBox = K.region({ x: 1030, y: 330, w: 340, h: 150 });         // x 860..1200
    const outLines = ["would have given 14 opinions", "2 of them risky", "tried to reach 1 host"]
      .map((w, i) => K.text(885, 292 + i * 38, w, { size: 17, anchor: "start" }));
    const plays = K.link(past, tryCls, { tone: "ink", from: "right", to: "left" });
    const comesOut = K.link(tryCls, outBox, { tone: "ink", from: "right", to: "left" });
    // What is not designed yet is drawn as a place with nothing in it: no fill, no shadow, a faint dashed outline,
    // grey words.
    const notYetChips = [["install and start", 510], ["take a grant back", 770]].map(([w, x]) => {
      const r = K.region({ x, y: 540, w: 200, h: 46 }, null, { dashed: true });
      r.box.setAttribute("stroke", K.tone("muted"));
      r.box.setAttribute("fill-opacity", 0);
      return [r, K.text(x, 540, w, { size: 16, color: "muted" })];
    });
    const notDesigned = K.text(640, 600, "not designed yet", { size: 17, color: "muted" });

    // 6. Try it on the recorded past first. The strip plays into the classifier in a fresh computer, and the list of
    //    what it would have done comes out line by line. Hero: that list. Last, what is not designed yet.
    scene(5);
    K.hide([plugin, ...parts, ...accepts, gCls, wCls, gRule, wRule, wPrec, gFwd, wFwd, you5, ...was.map(w => w[0])], { at: 0 });
    K.camera([past, fresh, outBox, ...notYetChips.flat(), notDesigned], { pad: 50, at: 0 });
    K.appear([fresh, tryCls], { from: "fade", stagger: 0.2, at: V.word(5, 0, "a classifier goes live") });
    K.appear(past, { from: "fade", at: V.word(5, 0, "the recorded past") - 0.3 });
    tl.to(frames, { opacity: 1, duration: 0.3, stagger: 0.08 }, T(5, V.word(5, 0, "the recorded past") - 0.1));
    K.draw(plays, { at: V.word(5, 0, "the recorded past") + 0.3 });
    K.flow(plays, { tone: "ink" });
    tl.to(frames, { fillOpacity: 0.9, fill: K.tone("ink"), duration: 0.25, stagger: 0.25, yoyo: true, repeat: 1 }, T(5, V.word(5, 0, "the recorded past") + 0.6));
    K.pulse(fresh, { tone: "ink", at: V.word(5, 0, "a fresh computer") });
    K.draw(comesOut, { at: V.word(5, 1, "what it would have done") - 0.3 });
    K.appear(outBox, { from: "fade", at: "<0.3" });
    K.appear(outLines[0], { from: "left", at: V.word(5, 1, "the opinions") });
    K.appear(outLines[1], { from: "left", at: "<0.4" });
    K.appear(outLines[2], { from: "left", at: V.word(5, 1, "tried to connect") });
    K.appear(notYetChips[0], { from: "fade", stagger: 0, at: V.word(5, 2, "installed and started") });
    K.appear(notYetChips[1], { from: "fade", stagger: 0, at: V.word(5, 2, "take a grant back") });
    K.appear(notDesigned, { from: "fade", at: V.word(5, 2, "not designed yet") });
  },
});
