explainer({
  title: "What Silo knows",
  answer: "Silo saw it, Silo was told it, or something thinks it.",
  scenes: [
    { caption: "A fact: Silo saw it.",
      source: "A fact is one thing Silo saw itself, from outside the computer. It is written once and never changed." },
    { caption: "A report: Silo was told it.",
      source: "A report is one thing a program inside the computer told Silo. It is written once, unverified, and the agent can interfere with it." },
    { caption: "The record: facts and reports, in order.",
      source: "Facts and reports, in the order they came, are the record. Nothing in it is changed afterwards." },
    { caption: "An opinion: something thinks it.",
      source: "An opinion is what a classifier, or you, think about the record. It is kept apart from the record, it can be wrong, and it says whose it is. A classifier's opinion is computed from the record, and computed again when the classifier improves." },
    { caption: "A classifier gives opinions.",
      source: "A classifier is whatever gives an opinion: a pattern, a function, a small model, an agent." },
    { caption: "As sure as its least sure ingredient.",
      source: "Something computed is as sure as its least sure ingredient. A count of facts is as good as a fact, a count of reports is still unverified, and it becomes an opinion only when a judgement enters." },
    { caption: "Silo never fills a hole.",
      source: "Silo never fills a hole." },
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
    // Something that cannot happen, or an opinion that was replaced: a line through the chip's words.
    const strike = (chip, width = 1.8) => svg("line", { x1: -chip.w / 2 + 9, x2: chip.w / 2 - 9, y1: 0, y2: 0, stroke: K.tone("muted"), "stroke-width": width, "stroke-linecap": "round" }, chip);
    // A computer: a region whose edge is drawn as a clear line, as in the films after this one. That edge is the boundary.
    const computer = (x, y, w, h) => {
      const r = K.region({ x, y, w, h }, "main", { icon: "machine" });
      r.box.setAttribute("stroke-width", 2.5);
      r.box.setAttribute("stroke", K.tone("muted"));
      return r;
    };
    // A row's mark ("fact · disk") brightens for a moment.
    const markPulse = (row, t) => tl.to(row.meta, { fill: K.tone("ink"), duration: 0.3, yoyo: true, repeat: 1, ease: "power2.inOut" }, t);
    // An opinion: a pill beside what it is about, in its own colour, with whose it is under it.
    const opinion = (x, y, words, whose) => [K.chip(x, y, words, { tone: "accent", size: 15 }), K.text(x, y + 32, whose, { size: 14, color: "muted" })];

    // The world. Left: the computer "main". On its edge, outside it, two parts of Silo: network and disk. Inside: the file
    // beside "disk", then the process and the probe, and the agent at the bottom. Right: the record, outside the computer.
    // Every row shows its mark (what it is, where it was seen) from the moment it lands; a report is drawn lighter than a
    // fact. Each row carries a lock: it is never changed. Each link ends at the row it produced, and the sources are
    // stacked in the order of their rows, so no two links cross; the agent's connection goes round the file to "network".
    const main = computer(245, 385, 350, 550);    // x 70..420, y 110..660
    const net = K.card(515, 205, "network", { icon: "globe", w: 170 });   // same width, so both stand 10 px off the edge
    const disk = K.card(515, 315, "disk", { icon: "database", w: 170 });
    const file = K.card(300, 315, "target/app", { icon: "file", w: 150 });
    const proc = K.chip(300, 397, "cargo build", { tone: "ink", icon: "process", size: 15 });
    const probe = K.card(300, 475, "probe", { icon: "eye", w: 130 });
    const agent = K.actor(150, 572, "agent", "agent");

    const record = K.window(925, 370, "The record", { w: 580, icon: "list" });
    const rConn = record.row("connection  crates.io", { mono: true, icon: "lock", meta: "fact · network" });
    const rFile = record.row("file  target/app  +48 MB", { mono: true, icon: "lock", meta: "fact · disk" });
    const rWrote = record.row("written by  cargo build  →  target/app", { mono: true, icon: "lock", muted: true, meta: "report · inside" });
    const rOwn = record.row("own account  3 model calls", { mono: true, icon: "lock", muted: true, meta: "report · inside" });

    const toNet = K.link(agent, net, { tone: "ink", from: "top", to: "left", bend: -0.35 });
    const netIn = K.link(net, rConn, { tone: "ink" });
    const looks = K.link(disk, file, { tone: "ink", dashed: true });
    const diskIn = K.link(disk, rFile, { tone: "ink" });
    const probeIn = K.link(probe, rWrote, { tone: "muted" });
    const reach = K.link(agent, probe, { tone: "muted", dashed: true, from: "top", to: "left" });
    const ownIn = K.link(agent, rOwn, { tone: "muted" });

    // 1. A fact. The agent's connection leaves through "network" and lands as a row. The file stays inside; "disk" looks in
    //    at it from outside, and its row lands. Hero: the two rows landing, one from what passed, one from what was looked
    //    at. Then a lock on each as the voice says "never changed".
    scene(0);
    K.camera(null, { dur: 0 });
    K.appear(main, { from: "fade", at: V.at(0, 0) });
    K.appear(agent, { from: "pop", at: "<0.2" });
    K.appear([disk, net], { from: "left", stagger: 0.15, at: "<0.3" });
    K.appear(record, { at: "<0.2" });
    tl.fromTo([rConn.icon, rFile.icon], { opacity: 0 }, { opacity: 1, duration: 0.5, stagger: 0.15, ease: "power2.out" }, T(0, V.word(0, 6, "never changed")));
    K.draw(toNet, { at: V.word(0, 1, "opens a connection") - 0.2 });
    const cConn = K.chip(0, 0, "connection", { tone: "ink", icon: "link", size: 14 });
    K.travel(cConn, toNet, { dur: 0.9, at: ">-0.1" });
    K.pulse(net, { tone: "ink", at: V.word(0, 2, "a part of Silo") });
    K.draw(netIn, { at: "<" });
    const cConn2 = K.chip(0, 0, "connection", { tone: "ink", icon: "link", size: 14 });
    K.travel(cConn2, netIn, { land: "absorb", dur: 0.5, at: V.word(0, 2, "outside the computer") });
    K.appear(rConn, { from: "fade", at: ">-0.1" });
    K.appear(file, { from: "pop", at: V.word(0, 3, "writes a file") });
    K.draw(looks, { at: V.word(0, 4, "Silo sees") });
    K.pulse(disk, { tone: "ink", at: V.word(0, 4, "the computer's disk") });
    K.draw(diskIn, { at: "<" });
    const cFile = K.chip(0, 0, "file", { tone: "ink", icon: "file", size: 14 });
    K.travel(cFile, diskIn, { land: "absorb", dur: 0.7, at: ">-0.1" });
    K.appear(rFile, { from: "fade", at: ">-0.1" });
    markPulse(rConn, T(0, V.word(0, 5, "a fact")));
    markPulse(rFile, T(0, V.word(0, 5, "a fact") + 0.15));

    // 2. A report. The connection is done: nothing links the agent to the parts on the edge. Inside, the process, then the
    //    probe, which tells Silo who wrote the file. Hero: that row landing, lighter, marked report. Then the agent's reach
    //    into the probe, and the agent's own account, a report too.
    scene(1);
    K.hide(toNet, { at: 0 });
    K.pulse(file, { tone: "ink", at: V.word(1, 0, "which process wrote") });
    K.appear(proc, { from: "pop", at: V.at(1, 1) });
    K.appear(probe, { from: "pop", at: V.word(1, 2, "the probe") });
    K.draw(probeIn, { at: V.word(1, 2, "tells Silo") - 0.3 });
    const cWrote = K.chip(0, 0, "written by", { tone: "muted", icon: "process", size: 14 });
    K.travel(cWrote, probeIn, { land: "absorb", dur: 0.8, at: ">-0.1" });
    K.appear(rWrote, { from: "fade", at: ">-0.1" });
    K.pulse(rWrote, { tone: "ink", at: V.word(1, 3, "a report") });
    markPulse(rWrote, T(1, V.word(1, 3, "a report")));
    K.draw(reach, { at: V.word(1, 4, "full access") });
    K.pulse(probe, { tone: "ink", at: V.word(1, 4, "only what the inside said") });
    K.draw(ownIn, { at: V.word(1, 5, "its own account") - 0.2 });
    const cOwn = K.chip(0, 0, "own account", { tone: "muted", icon: "agent", size: 14 });
    K.travel(cOwn, ownIn, { land: "absorb", dur: 0.9, at: ">-0.1" });
    K.appear(rOwn, { from: "fade", at: ">-0.1" });
    markPulse(rOwn, T(1, V.word(1, 6, "a report too")));

    // 3. The record alone. The rows light in the order they came, the locks as the voice says nothing changes. Hero: the
    //    report row points to the fact row; the two marks pulse, fact then report.
    scene(2);
    const pointsTo = K.link(rWrote, rFile, { from: "left", to: "left", tone: "ink", bend: 1 });
    const pointsSays = K.text(record.at("left").x - 39, (rFile.at("left").y + rWrote.at("left").y) / 2, "points to it", { size: 15, color: "ink", anchor: "end" });
    K.hide([main, file, proc, agent, probe, disk, net, netIn, looks, diskIn, probeIn, reach, ownIn], { at: 0 });
    K.camera([record, pointsSays], { pad: 50, at: 0.2 });
    K.pulse([rConn, rFile, rWrote, rOwn], { tone: "ink", at: V.word(2, 0, "in the order they came") });
    tl.to([rConn, rFile, rWrote, rOwn].map(r => r.icon), { stroke: K.tone("ink"), duration: 0.3, yoyo: true, repeat: 1, stagger: 0.1 }, T(2, V.word(2, 1, "Nothing in it")));
    K.draw(pointsTo, { at: V.word(2, 2, "never both") });
    K.appear(pointsSays, { from: "fade", at: "<0.3" });
    markPulse(rFile, T(2, V.word(2, 2, "is a fact")));
    markPulse(rWrote, T(2, V.word(2, 2, "is a report")));

    // 4. An opinion, beside the row and never in it. The classifier reads the record through a dashed one-way link. Version 1
    //    says "code"; version 2 says "build output", and the old opinion stays at full strength, muted and struck through.
    //    Hero: the opinion changing while the record holds still. Then Silo asks you, beside you and not from the record;
    //    your answer is an opinion too, beside the file row with "you" under it. The record gains no row.
    scene(3);
    const FY = rFile.at("left").y;
    const files = K.card(925, 150, "files, version 1", { sub: "a function", icon: "code", w: 270 });
    const reads = K.link(record, files, { tone: "muted", dashed: true });
    const [oCode, oCodeWho] = opinion(535, FY, "code", "files v1");
    const [oBuild, oBuildWho] = opinion(535, FY, "build output", "files v2");
    const [oYou, oYouWho] = opinion(345, FY, "build output", "you");
    const codeStrike = strike(oCode, 1.2);
    const you = K.actor(345, 575, "you", "you", { tone: "you" });
    const askCard = K.card(600, 575, "is this a build?", { icon: "message", w: 240 });
    const yes = K.chip(270, 575, "yes", { tone: "you", size: 15 });
    K.dock(yes, you, { side: "left", dur: 0.01, at: 0 });
    K.hide([pointsTo, pointsSays], { at: 0 });
    K.camera(null, { at: 0 });
    K.appear(files, { at: V.at(3, 0) });
    K.appear([oCode, oCodeWho], { from: "right", at: V.word(3, 0, "what something thinks") });
    K.draw(reads, { at: V.word(3, 1, "computed from the record") });
    K.pulse(oCode, { tone: "accent", at: V.word(3, 1, "kept apart") });
    K.pulse(oCodeWho, { tone: "accent", at: V.word(3, 3, "says whose it is") });
    K.relabel(files, "files, version 2", { at: V.word(3, 3, "improves") });
    K.pulse(files, { tone: "accent", at: "<" });
    K.moveTo(oCode, 175, FY, { at: V.word(3, 3, "computed again") - 0.2 });
    K.moveTo(oCodeWho, 175, FY + 32, { at: "<" });
    tl.to(oCode.label, { fill: K.tone("muted"), duration: 0.4, ease: "power2.out" }, ">-0.2");
    tl.fromTo(codeStrike, { scaleX: 0, transformOrigin: "50% 50%" }, { scaleX: 1, duration: 0.4, ease: "power2.out" }, "<");
    K.appear([oBuild, oBuildWho], { from: "right", at: "<0.1" });
    K.pulse(record, { tone: "ink", at: V.word(3, 4, "do not change") });
    K.appear(you, { from: "pop", at: V.at(3, 5) });
    K.appear(askCard, { from: "pop", at: V.word(3, 6, "Silo asks") });
    K.appear(yes, { from: "pop", at: V.word(3, 7, "Your answer") });
    K.appear([oYou, oYouWho], { from: "below", at: V.word(3, 7, "your opinion") });
    K.pulse(oYou, { tone: "accent", at: V.word(3, 8, "as you gave it") });

    // 5. Four classifiers, each with its opinion under it. Hero: the four cards with their opinions.
    scene(4);
    const clsY = 350, opY = 446;
    const cPat = K.card(205, clsY, "*.pem", { sub: "a pattern", icon: "search" });
    const cFun = K.card(495, clsY, "files v2", { sub: "a function", icon: "code" });
    const cMod = K.card(785, clsY, "small model", { icon: "model" });
    const cAg = K.card(1075, clsY, "agent", { sub: "another agent", icon: "agent" });
    const ops = [[205, "credential-like"], [495, "build output"], [785, "risky"], [1075, "needs you"]]
      .map(([x, w]) => K.chip(x, opY, w, { tone: "accent", size: 15 }));
    K.hide([record, reads, files, oCode, oCodeWho, oBuild, oBuildWho, oYou, oYouWho, you, askCard, yes], { at: 0 });
    K.camera([cPat, cFun, cMod, cAg, ...ops], { pad: 70, at: 0 });
    K.appear(cPat, { from: "below", at: V.word(4, 0, "a pattern") });
    K.appear(cFun, { from: "below", at: V.word(4, 0, "a function") });
    K.appear(cMod, { from: "below", at: V.word(4, 0, "a small model") });
    K.appear(cAg, { from: "below", at: V.word(4, 0, "an agent") });
    K.appear(ops, { from: "above", stagger: 0.2, at: V.at(4, 1) });

    // 6. How sure something computed is. Three lines stand faint from the start, each lights as the voice says it, and they stay. Then, with the last
    //    sentence only, the record, small, beside them. Hero: the three lines with the two things that cannot happen: an
    //    opinion as a row, and a report as a fact.
    scene(5);
    const LX = 90;
    const lines = [
      [200, "fact + fact  →  as good as a fact", "spend of main: $4.20"],
      [330, "fact + report  →  unverified", "written by cargo build: 48 MB"],
      [460, "anything + a judgement  →  an opinion", "build output"],
    ].map(([y, rule, ex]) => [K.text(LX, y, rule, { size: 20, mono: true, anchor: "start" }), K.text(LX, y + 32, ex, { size: 16, color: "muted", anchor: "start" })]);
    const small = K.window(870, 320, "The record", { w: 460, icon: "list" });
    const sConn = small.row("connection  crates.io", { mono: true, icon: "lock", meta: "fact · network" });
    const sFile = small.row("file  target/app  +48 MB", { mono: true, icon: "lock", meta: "fact · disk" });
    const sWrote = small.row("written by  cargo build", { mono: true, icon: "lock", muted: true, meta: "report · inside" });
    // A hole: a place of the list that holds nothing, outlined in dashes. The row after it lands in the last scene.
    const sHole = small.row("");
    for (const [k, v] of [["stroke", K.tone("muted")], ["stroke-width", 1.5], ["stroke-dasharray", "6 5"]]) sHole.band.setAttribute(k, v);
    const sOwn = small.row("own account  3 model calls", { mono: true, icon: "lock", muted: true, meta: "report · inside" });
    const notFact = K.chip(small.at("right").x + 50, sWrote.at("left").y, "fact", { tone: "muted", size: 15 });
    strike(notFact, 1.2);
    const notFactSays = K.text(small.at("right").x + 50, sWrote.at("left").y + 32, "never a fact", { size: 15, color: "muted" });
    const notRow = K.chip(870, small.at("bottom").y + 58, "build output", { tone: "muted", size: 15 });
    strike(notRow);
    const notRowSays = K.text(870, small.at("bottom").y + 96, "never a row", { size: 15, color: "muted" });
    K.hide([cPat, cFun, cMod, cAg, ...ops], { at: 0 });
    K.camera(null, { at: 0 });
    // Faint: the words inside each piece stand at a third of their strength until the voice says their line.
    tl.set(lines.flat().map(p => p.text), { opacity: 0.33 }, T(5, 0));
    K.appear(lines.flat(), { from: "fade", stagger: 0.08, at: V.at(5, 0) });
    lines.forEach((l, i) => tl.to(l.map(p => p.text), { opacity: 1, duration: 0.5, stagger: 0.25, ease: "power2.out" }, T(5, V.at(5, i + 1))));
    K.appear(small, { at: V.at(5, 4) - 0.6 });
    K.appear([sConn, sFile, sWrote], { from: "fade", stagger: 0.1, at: "<0.2" });
    K.appear([notFact, notFactSays], { from: "pop", stagger: 0.2, at: V.word(5, 4, "a report") });
    K.appear([notRow, notRowSays], { from: "pop", stagger: 0.2, at: V.word(5, 4, "an opinion") });

    // 7. The record alone, larger. Its fourth row lands one place lower, and the place between holds nothing. Hero: that
    //    place, outlined as the voice says "a hole", with nothing inside.
    scene(6);
    const holeX = small.at("right").x + 26, holeY = sHole.at("left").y;
    const holeSays = K.text(holeX, holeY - 11, "a hole", { size: 18, anchor: "start" });
    const holeStays = K.text(holeX, holeY + 13, "never filled", { size: 15, color: "muted", anchor: "start" });
    K.hide([...lines.flat(), notFact, notFactSays, notRow, notRowSays], { at: 0 });
    K.camera([small, holeSays, holeStays], { pad: 60, at: 0.2 });
    K.appear(sOwn, { from: "fade", at: V.at(6, 0) });
    K.appear(sHole, { from: "fade", at: V.word(6, 0, "a hole") - 0.2 });
    K.appear([holeSays, holeStays], { from: "fade", stagger: 0.2, at: "<0.2" });
    K.pulse(sHole, { tone: "ink", at: V.end(6, 0) });
  },
});
