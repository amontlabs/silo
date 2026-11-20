explainer({
  title: "One call through the engine",
  answer: "A click becomes one input. The core turns it into new state, an event and an effect.",
  scenes: [
    { caption: "State is a small table.",
      source: "The engine's state is a few small tables of plain rows. A computer is one row: its number, its name, what is wanted and what was last seen." },
    { caption: "A click becomes a call, through one queue.",
      source: "Nothing changes the state except an input handled by the core, one at a time. A call from the app is an input. So is a driver's answer, and what a driver sees when a machine stops by itself." },
    { caption: "The core decides and touches nothing.",
      source: "The core is a function. It takes the state and an input, and returns the new state, events and effects. It touches no disk and no network." },
    { caption: "A driver does the work and answers later.",
      source: "An effect is a request to a driver. It has no return value. The answer, or a timeout, comes back later as an input." },
    { caption: "What you want and what the engine saw now match.",
      source: "Each row keeps what is wanted and what was last seen. While they differ, the core issues effects to make them match, or says what it is waiting for. What it does when a computer stops by itself is still open." },
    { caption: "The same inputs always give the same result.",
      source: "The same inputs in the same order always give the same state and the same events. Time and randomness are inputs too." },
  ],
  build(K, tl, scene, V) {
    // The world, laid out once. Top: the state and the events. Middle: the way in, the queue, the core.
    // Bottom: the driver and the machine it runs. The camera does the travelling.
    const ROW = (w, s) => `7  main     wanted ${w}  seen ${s}`;
    const table = K.window(470, 140, "computers", { w: 440, icon: "database" });
    const row7 = table.row(ROW("stopped", "stopped"), { mono: true });
    const row8 = table.row("8  try-bun  wanted stopped  seen stopped", { mono: true });

    const zig = K.doc(1030, 300, "computer.zig", { w: 300 });
    const code = ["const Computer = struct {", "    id: u32,", "    name: Name,", "    wanted: State,", "    seen: State,", "};"].map(s => zig.line(s));
    const record = K.link(zig, row7, { tone: "accent" });
    const recordSays = K.text(800, 104, "one row is one struct", { size: 16, color: "accent" });

    const events = K.window(1000, 140, "events", { w: 360, icon: "list" });
    const ev412 = events.row("#412  computer_start_requested(7)", { mono: true });
    const ev413 = events.row("#413  computer_started(7)", { mono: true });

    const you = K.actor(150, 210, "you", "You", { tone: "you" });
    const app = K.card(160, 400, "the app", { icon: "window", w: 200 });
    const start = K.badge(app, "Start", { tone: "you", side: "bottom", icon: "play" });
    const queue = K.window(470, 400, "one queue", { w: 230, icon: "inbox" });
    const waitA = queue.row("driver answer", { icon: "server", muted: true });
    const waitB = queue.row("clock", { icon: "clock", muted: true });
    const slot = queue.row("start_computer(7)", { icon: "send", mono: true, tone: "you" });
    const core = K.card(790, 400, "core", { sub: "a function", icon: "brain", tone: "accent", w: 220 });
    const driver = K.card(790, 610, "VM driver", { sub: "does the real work", icon: "server", w: 220 });
    const vm = K.card(1090, 610, "virtual machine", { sub: "computer 7", icon: "machine", w: 220 });
    const fake = K.card(790, 610, "fake driver", { sub: "fails on purpose", icon: "bug", w: 220, dashed: true });
    const sim = K.actor(160, 610, "refresh", "simulator");
    // The record: what agents do inside a computer, kept apart from the events. Drawn as the next film draws it,
    // its row with its lock and its mark. Nothing links it to the core or to the events.
    const theRecord = K.window(270, 615, "The record", { w: 420, icon: "list" });
    const recRows = [
      theRecord.row("connection  crates.io", { mono: true, icon: "lock", meta: "fact · network" }),
    ];

    const appIn = K.link(app, queue, { tone: "you" });
    const toCore = K.link(queue, core, { tone: "accent" });
    const coreTop = core.at("top");
    const toTable = K.link({ x: coreTop.x - 50, y: coreTop.y - 6 }, table, { tone: "warn" });
    const toEvents = K.link({ x: coreTop.x + 50, y: coreTop.y - 6 }, events, { tone: "accent" });
    const toDriver = K.link(core, driver, { tone: "accent" });
    const toVm = K.link(driver, vm, { tone: "accent" });
    const back = K.link(driver, queue, { tone: "ok", label: "the same queue" });
    const simIn = K.link(sim, queue, { to: "left", tone: "you" });

    // 1. State is a small table. Open close on the table; row 7 is lit as the voice says "each row is one computer",
    //    and the record's fields light one by one as they are named.
    scene(0);
    K.camera([table, zig], { dur: 0, pad: 70 });
    K.appear(table, { at: V.at(0, 0) + 0.2 });
    K.appear([row7, row8], { stagger: 0.3, at: V.at(0, 1) });
    K.spotlight([row7, zig, record, recordSays], { at: V.word(0, 2, "Each row") });
    K.appear(zig, { from: "right", at: "<0.2" });
    K.appear(code, { from: "fade", stagger: 0.06, at: "<0.2" });
    K.draw(record, { at: ">-0.2" });
    K.appear(recordSays, { from: "fade", at: "<0.4" });
    K.recolor(code[1], "accent", { at: V.word(0, 2, "its number") });
    K.recolor(code[2], "accent", { at: V.word(0, 2, "its name") });
    K.recolor(code[3], "you", { at: V.word(0, 2, "what you want") });
    K.recolor(code[4], "ok", { at: V.word(0, 2, "what the engine last saw") });

    // 2. A click becomes a call, through one queue. Hero: the call riding into the queue behind two inputs already
    //    waiting. Then the core takes them one at a time, in order, and the queue moves up: the click is first in line.
    scene(1);
    K.hide([zig, record, recordSays]);
    K.camera([you, app, start, queue, core], { pad: 60, at: "<" });
    K.appear([you, app], { stagger: 0.2, at: V.at(1, 0) });
    K.appear(start, { from: "pop", at: "<0.3" });
    K.pulse(start, { tone: "you", at: V.word(1, 0, "Start") + 0.2 });
    K.appear(queue, { at: V.at(1, 1) + 0.3 });
    K.appear([waitA, waitB], { from: "fade", stagger: 0.2, at: "<0.3" });
    K.draw(appIn, { at: V.at(1, 2) });
    const call = K.chip(0, 0, "start_computer(7)", { tone: "you", mono: true, size: 15 });
    K.travel(call, appIn, { dur: 1.3, at: V.word(1, 2, "start computer") });
    K.appear(slot, { from: "fade", at: ">-0.1" });
    K.appear(core, { at: V.at(1, 3) });
    K.draw(toCore, { at: "<0.3" });
    const SLOT = i => 69 + i * 40;   // where a queue row sits in its window, front first
    const takeA = K.chip(0, 0, "driver answer", { tone: "ink", icon: "server", size: 15 });
    const tA = V.word(1, 3, "through that queue"), tB = Math.max(V.word(1, 3, "one at a time") - 0.2, tA + 1);  // B leaves once A has landed
    K.hide(waitA, { at: tA });
    K.travel(takeA, toCore, { dur: 0.9, at: "<" });
    K.moveTo(waitB, 0, SLOT(0), { dur: 0.5, at: "<0.3" });
    K.moveTo(slot, 0, SLOT(1), { dur: 0.5, at: "<" });
    const takeB = K.chip(0, 0, "clock", { tone: "ink", icon: "clock", size: 15 });
    K.hide(waitB, { at: tB });
    K.travel(takeB, toCore, { dur: 0.9, at: "<" });
    K.moveTo(slot, 0, SLOT(0), { dur: 0.5, at: "<0.3" });
    K.pulse(slot, { tone: "you", at: ">" });

    // 3. The core decides and touches nothing. It takes the call, checks row 7, and returns three things,
    //    each as the voice names it: the new state (row 7 turns amber: wanted and seen differ), an event, an effect.
    scene(2);
    K.camera(null);
    K.appear(events, { at: V.word(2, 2, "an event") - 0.5 });
    K.appear(driver, { at: V.word(2, 2, "an effect") - 0.5 });
    K.pulse(core, { at: V.at(2, 0) });
    const take = K.chip(0, 0, "start_computer(7)", { tone: "you", mono: true, size: 15 });
    K.hide(slot, { at: V.at(2, 1) });
    K.travel(take, toCore, { at: "<" });
    K.pulse(row7, { tone: "accent", at: V.word(2, 1, "checks") });
    const checked = K.badge(core, "7 exists, stopped", { tone: "ok", side: "bottom", icon: "check" });
    K.appear(checked, { from: "pop", at: "<" });
    K.hide(checked, { at: V.at(2, 2) });
    K.draw(toTable, { at: V.word(2, 2, "the new state") });
    K.relabel(row7, ROW("running", "stopped"), { at: ">-0.2" });
    K.recolor(row7, "warn", { at: "<" });
    K.draw(toEvents, { at: V.word(2, 2, "an event") });
    K.appear(ev412, { from: "fade", at: ">-0.2" });
    K.dim(driver, { to: 1, at: V.word(2, 2, "an effect") });
    K.draw(toDriver, { at: "<" });
    const effect = K.chip(0, 0, "boot_vm(7)", { tone: "accent", mono: true, size: 15 });
    K.travel(effect, toDriver, { land: "dock", dur: 0.9, at: ">-0.3" });

    // 4. A driver does the work and answers later. Hero: the answer riding back into the same queue the click used,
    //    empty now: it is first in line.
    scene(3);
    K.camera([queue, core, driver, vm], { pad: 70 });
    K.pulse(effect, { tone: "accent", at: V.at(3, 0) });
    K.moveTo(effect, 790, 610, { dur: 0.6, at: V.at(3, 1) });
    K.hide(effect, { dur: 0.25, at: ">-0.2" });
    K.pulse(driver, { tone: "accent", at: "<" });
    K.draw(toVm, { at: V.word(3, 1, "it starts") });
    K.appear(vm, { at: ">-0.3" });
    const slow = K.badge(vm, "takes time, can fail", { tone: "warn", icon: "clock" });
    K.appear(slow, { from: "pop", at: V.at(3, 2) });
    K.draw(back, { at: V.at(3, 3) });
    const answer = K.chip(0, 0, "vm_booted(7)", { tone: "ok", mono: true, size: 15 });
    K.travel(answer, back, { dur: 1.5, at: V.word(3, 3, "comes back") });
    K.moveTo(waitA, 0, SLOT(0), { dur: 0, at: ">-0.2" });
    K.relabel(waitA, "vm_booted(7)", { color: "ink", dur: 0, at: "<" });
    waitA.label.setAttribute("font-family", "'Geist Mono', ui-monospace, Menlo, monospace");   // shown as code, like the other input
    waitA.label.setAttribute("font-size", 15);
    K.recolor(waitA, "ok", { dur: 0, at: "<" });
    K.appear(waitA, { from: "fade", at: "<" });

    // 5. Wanted and seen agree. The answer is handled like any input. Hero: row 7 turning green.
    scene(4);
    K.camera([table, events, queue, core], { pad: 60 });
    const take2 = K.chip(0, 0, "vm_booted(7)", { tone: "ok", mono: true, size: 15 });
    K.hide([slow, waitA], { at: V.at(4, 0) });
    K.travel(take2, toCore, { at: "<" });
    K.relabel(row7, ROW("running", "running"), { at: V.word(4, 1, "seen running") });
    K.recolor([row7, toTable], "ok", { at: "<" });
    K.appear(ev413, { from: "fade", at: V.word(4, 1, "a second event") });
    K.glow(row7, { tone: "ok", at: V.word(4, 2, "agree") });
    K.dim([toDriver, toVm, back], { to: 0.35, at: "<" });

    // 6. The same inputs always give the same result. The two inputs that went through the queue come back and light
    //    as the voice says "the same inputs": they are what is replayed. Then the events light, with a link to the app
    //    that follows them. Then the simulator feeds random inputs through the same queue while a fake driver fails on purpose.
    scene(5);
    // The link leaves left of the core's arrow into the events, clear of the window's edge, and bends down so it
    // crosses the link into the table near its middle, away from that link's arrowhead.
    const evBottom = events.at("bottom");
    const follow = K.link({ x: evBottom.x - 120, y: evBottom.y + 26 }, app, { to: "top", tone: "accent", bend: -0.2 });
    K.camera(null);
    K.hide([you, start], { at: "<" });
    K.moveTo(waitA, 0, SLOT(1), { dur: 0, at: "<" });
    K.appear([slot, waitA], { from: "fade", stagger: 0.2, at: V.at(5, 0) + 0.3 });
    K.glow(slot, { tone: "you", at: V.word(5, 1, "the same inputs") });
    K.glow(waitA, { tone: "ok", at: "<0.2" });
    K.draw(follow, { at: V.word(5, 2, "follows") });
    K.glow([ev412, ev413], { at: "<" });
    K.appear(theRecord, { from: "fade", at: V.word(5, 3, "the record") - 0.2 });
    K.appear(recRows, { from: "fade", at: "<0.2" });
    K.hide([theRecord, ...recRows], { at: V.at(5, 4) - 0.2 });
    K.hide([vm, toVm, driver], { at: V.at(5, 4) });
    K.dim([toDriver, back], { to: 1, at: "<" });
    K.appear([sim, fake], { at: "<0.4" });
    K.draw(simIn, { at: "<0.2" });
    K.flow([simIn, toCore, toDriver, back], { at: "<0.3" });
    const many = K.badge(sim, "millions of random sequences", { tone: "you", side: "top", icon: "refresh" });
    K.appear(many, { from: "pop", at: V.word(5, 4, "millions") });
    const props = K.badge(core, "properties checked after every step", { tone: "ok", side: "right" });
    K.appear([K.mark(core, "ok"), props], { from: "pop", stagger: 0.2, at: V.word(5, 4, "before it ever") });
  },
});
