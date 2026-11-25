# Engine internals: a second opinion on eight decisions

An agent's review, 24 November 2026, for polarzero. It changes nothing in `plan/`. Each entry was
read with `plan show`, with the entries it needs and the decisions in force that bind it. Prior art
was read where a link is given; a line marked "not verified" was not read at its source today.

One experiment could not be run: downloading Zig 0.17.1 into this container was refused by the
session's permission check, so the Zig claims below come from Zig's documentation and from reading,
not from compiling. The trial of 21 November (notes of `core-simulator` and `deterministic-core`)
is the only compiled evidence.

| id | Recommendation | Confidence | One-line reason |
|---|---|---|---|
| `core-dependencies` | Take with an edit | high | Right rule, but "only `std.debug.assert`" blocks the checked arithmetic the core needs, and "added without changing the core" cannot be checked |
| `core-knows-no-agents` | Take with an edit | medium | Right, but it names no check, and it must say how the core tells a call from inside main apart without knowing agents |
| `core-state-stays-small` | Replace the answer | high | Its test passes on any fixed-table core, so it checks nothing, and "current rows only" contradicts `call-ids` and `events-until-the-recorder` |
| `engine-inputs` | Take with an edit | high | One queue is right; it needs a bound per source and room kept for driver results, or an agent in main can delay your restore |
| `engine-state` | Take with an edit | medium | Right; add that a row refers to another by an id that is never reused and never a table position |
| `engine-source-checks` | Take with an edit | high | "Wrapping arithmetic on a value that came from an input" cannot be found by reading source; several real ways around the rules are missing |
| `tigerstyle-by-place` | Take with an edit | medium | Leaves out the probe and the Swift code; two of its core rules cannot be checked as written |
| `how-the-engine-is-built` | Take with an edit (polarzero) | medium | Right, but "a person reads line by line" names no one, and polarzero said engine statements are hard to assess |

## The three findings that matter most

1. **The simulator checks the core, but the crash bugs live in the loop around it.** The write
   of each input, the call-id table, the queue, the timeouts and the reconcile at start are all
   outside the core. TigerBeetle and FoundationDB simulate that code too, by giving it its disk,
   network and clock through interfaces. Silo's plan does not say the loop that feeds the core is
   run by the simulator. It should, and the cheapest place to say it is `core-dependencies` or
   `engine-inputs`. Details in "Cross-cutting findings".
2. **Clock ticks and the write before each input.** `engine-inputs` sends the clock through the
   queue, and `engine-saved-state` writes every input to disk before the core applies it. A tick
   every 100 ms means ten forced writes a second while nothing happens. quinn-proto avoids this:
   the core says when it next needs the time (`poll_timeout`). With that, and with "no output
   leaves before its input is written" in place of "written before applied", writes group by
   themselves.
3. **Two checks in these entries cannot fail.** `core-state-stays-small`'s size test passes on
   any core with fixed tables. `engine-source-checks` asks for a dataflow check that reading the
   source cannot do. `check-must-catch-a-fault` (in force) already says such a check does not
   count, so both need new wording before agents take them.

---

## `core-dependencies`: What may the core depend on?

- **Recommendation:** Take with an edit. The rule is right and matches TigerBeetle, but it is
  stricter than the core can live with in one place and promises one thing no check can show.
- **Confidence:** high. It rests on the trial (core imported only `std.debug.assert`), on
  TigerBeetle's own handling of Zig's standard library, and on Zig's documented safety rules.
- **What it decides, on one real case:** the first core adds a checkpoint every N minutes, so it
  computes `last_checkpoint + interval`. Both come from inputs (the interval from your settings, the
  time from the clock). In ReleaseSafe a plain `+` that overflows stops the engine (Zig's
  documentation lists integer overflow as safety-checked illegal behaviour). `inputs-never-stop-the-engine`
  says no input may do that. The core needs `std.math.add`, which returns `error.Overflow`. The
  trial's source check, which "refuses anything but `std.debug.assert`", would refuse it.
- **Options:**
  - As written: no dependencies, and the trial's check allowing only `std.debug.assert`. Costs the
    core every checked operation of `std.math` and `std.mem`; agents then write their own, worse.
  - No dependencies, and a written list of the parts of Zig's standard library the core may use,
    each of which does no I/O: `std.debug.assert`, `std.math` (checked arithmetic), `std.mem`
    (compare, copy), `std.meta`, `std.hash` (pure hashes). The source check holds the list. This
    is TigerBeetle's practice: its `tidy.zig` bans `std.BoundedArray`, `std.time.Instant`,
    `mem.copy(` and others, and names the replacement in its own `stdx`.
  - Allow pure third-party Zig packages in the core. Nobody serious does this for a deterministic
    core; Zig's own standard library changes under you enough between versions (0.15 to 0.17).
- **Evidence:**
  - TigerBeetle `tidy.zig` at tag 0.16.30 bans named parts of `std` and points to `stdx`
    replacements. Verified, read: https://raw.githubusercontent.com/tigerbeetle/tigerbeetle/0.16.30/src/tidy.zig
  - TigerBeetle's `src/stdx/` holds its own `bounded_array.zig`, `ring_buffer.zig`, `prng.zig`,
    `time_units.zig`, `queue.zig` and a `vendored/` folder. Verified (directory listing):
    https://github.com/tigerbeetle/tigerbeetle/tree/main/src/stdx
  - TIGER_STYLE: "zero dependencies" apart from the Zig toolchain. Verified:
    https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/TIGER_STYLE.md
  - Zig 0.15.2 reference: integer overflow on `+`, `-`, `*` and an out-of-range `@intCast` are
    safety-checked illegal behaviour, which panics in ReleaseSafe. Verified:
    https://ziglang.org/documentation/0.15.2/
  - Zig 0.17.1 was released on 2 November 2026 and 0.18.0-dev is on master. Verified from
    https://ziglang.org/download/index.json
  - Sans-IO libraries keep the same line: quinn-proto "contains no networking code and does not get
    any relevant timestamps from the operating system"; rustls "doesn't make or accept TCP
    connections, or do DNS, or read or write files", and its cryptography is a provider you plug in.
    Verified: https://docs.rs/quinn-proto/latest/quinn_proto/ and https://docs.rs/rustls/latest/rustls/
- **Risks and what would change this:** if the first core never needs checked arithmetic (every
  input-derived value bounded so tightly by validation that overflow is impossible), the
  narrower rule holds. That is unlikely once time and sizes enter.
  "A new virtual machine monitor ... added without changing the core" is true only for one that
  offers the effects the core already has. Apple's framework can save a running macOS computer's
  memory; libkrun's build may not. A capability nobody has yet, such as a memory fork, is a new
  effect and changes the core. `neutral-core` already says differences reach the core as data,
  which is the checkable half.
- **Proposed text:**
  > No dependencies in the core. It uses only the parts of Zig's standard library on one list,
  > each of which does no I/O, and the check that reads the source holds the list. Established
  > tools sit behind drivers. A new virtual machine monitor (the program that runs computers, as
  > libkrun and Apple's Virtualization framework do) or a new device type is a driver, with a fault
  > model and a fake. When it offers only effects the core already has, it is added without changing
  > the core. The loop that feeds the core reaches the disk, the clock and the drivers only through
  > interfaces it is given at start, so the simulator runs that same loop over fakes.
- **For the first build:** blocks `engine-program-and-clients`. Agents can take it with a judge;
  the check is the source check of the trial plus the list. The builder needs the list on day one.

## `core-knows-no-agents`: What does the core know about agents?

- **Recommendation:** Take with an edit: add the check, and say what the core does know about
  where a call comes from.
- **Confidence:** medium. The rule is sound and nothing in the first build needs agents in the core;
  the doubt is only whether a check can be written that is not just a word list.
- **What it decides, on one real case:** Claude Code in main runs `silo restore main <checkpoint>`.
  `what-an-agent-inside-may-do` (taken) lets it, scoped to main, and `who-can-call-the-engine` says
  the call carries where it came from. The core must decide on "a call from computer main", which
  it can do without knowing that an agent, Claude Code or a run sent it. Without that sentence a
  builder can read "holds nothing about agents" as "holds nothing about callers inside a computer",
  and give such calls the rights of your Mac.
- **Options:**
  - As written. Costs the check: `owners-part-on-engine` lets agents take only what a check can show.
  - With a check, like `core-knows-no-system` (proposed in `neutral-core`): no table, field or kind
    of input of the core names an agent, an agent program, a run or a conversation, and the source
    check refuses those names in the core's folder. The caller is known by the way the call arrived:
    your device, or one computer.
- **Evidence:** the plan's own record-path design (`record-path-and-core`) sends conversations, runs
  and model calls down the record path, never through the core. `run` and `conversation` are words
  in force (`plan/words.json`). No outside prior art is needed here.
- **Risks and what would change this:** a budget on a run that must stop a request at once would
  pull runs into the core. The entry's notes already make that a precaution, outside the core. If
  polarzero later wants a hard spend stop enforced by the core, this changes.
- **Proposed text:**
  > The core holds nothing about agents. No table, field or kind of input of the core names an agent,
  > an agent program, a run or a conversation, and the check that reads the source refuses those
  > names in the core. A call is known only by where it arrived from: this device, or one computer.
- **For the first build:** blocks `engine-program-and-clients`, and is small. Agents can take it.

## `core-state-stays-small`: Where is the history of a computer kept?

- **Recommendation:** Replace the answer. The idea is right; the check cannot fail and one sentence
  contradicts two other entries.
- **Confidence:** high. It rests on reading the entries together.
- **What it decides, on one real case:** a builder adds a field to main's row, `checkpoints: [64]CheckpointId`,
  to show the last checkpoints in `silo checkpoints main`. That is history in the core. The proposed
  test, "the saved state's size stays the same however many inputs are applied", passes: the array
  has a fixed size whether it is empty or full. Every table in the core has a fixed size by
  `engine-limits`, so the test passes for every core that compiles. By `check-must-catch-a-fault`
  it does not count.
- **Contradiction:** "the core's saved state holds the current rows only". But `call-ids` keeps a
  fixed table of recent call ids with their results, and `events-until-the-recorder` keeps the
  events the recorder has not acknowledged in the core's state. Both are recent past, held in the
  saved state. The sentence is false as soon as those two are built.
- **Options:**
  - As written. Costs a check that checks nothing, and a sentence that the next two features break.
  - A check that can fail: after a long history that ends with the same computers, checkpoints and
    rules as a short one, the two saved states are equal, apart from counters named in one list
    (the last event number, the last id handed out). The `checkpoints: [64]` field above makes it
    fail. This is the same equality the simulator already uses to compare states byte for byte.
  - Move the question into `engine-limits`, which already fixes table sizes. Costs nothing but leaves
    the "history lives in the record" sentence without a home.
- **Evidence:** the plan's entries named above. TigerBeetle keeps a bounded client table (one entry
  per client session, with its last reply) in its replica state; that is the same kind of "recent
  past by design" as `call-ids`. Not verified line by line here; the session table is described in
  TigerBeetle's docs and `call-ids` already cites it.
- **Proposed text:**
  > In the record, and the past of a computer's disk in its checkpoints. The core's saved state
  > holds the current rows and three tables of recent work, each of fixed size: the recent call ids
  > (`call-ids`), the events the recorder has not acknowledged (`events-until-the-recorder`) and the
  > effects under way. The loop that feeds the core keeps its rings of recent inputs and effects
  > (`assertions-stay-on`) outside the saved state. A test applies two histories that end with the
  > same computers, checkpoints and rules, one short and one long, and checks that the two saved
  > states are equal except for the counters on one list.
- **For the first build:** needed by `engine-saved-state` and `engine-restart-after-crash`. Agents
  can take the replacement; the test is written with the simulator.

## `engine-inputs`: How does anything change the engine's state?

- **Recommendation:** Take with an edit: bound each source, keep room for driver results, and let
  the core ask for the clock instead of being ticked.
- **Confidence:** high on the bound and the room; medium on the clock, which depends on the cost of
  a forced write on a Mac (not measured, `engine-saved-state`).
- **What it decides, on one real case:** a script of Codex's in main goes wrong and calls
  `silo checkpoint main` in a loop. Each call is valid. With one first-in-first-out queue, your
  `silo restore main` from your Mac waits behind thousands of them, and the driver's answer that a
  checkpoint finished waits too. If the queue is full, that driver answer cannot be refused
  (`engine-saved-state` says a driver result or a tick cannot be refused), so the engine stops.
- **Options:**
  - As written: one queue, one validation. Costs the case above.
  - One queue into the core, fed from a bounded place per source: your device, each computer, the
    drivers, the clock. The loop takes from each in turn, so a source delays the others by at most
    its share. A call from a source whose place is full is refused with a named error. Room for
    the result of every effect under way is kept when the effect is issued, so a driver result
    always fits. TigerBeetle bounds each client to one request in flight and refuses past its
    client count; quinn-proto's endpoint drops datagrams it has no room for and never blocks.
  - Priorities (your calls before an agent's). Costs a rule about who is more important, which the
    first build does not need; fairness is enough.
- **The clock:** quinn-proto takes the time only in two calls, `handle_timeout(now)` and
  `poll_transmit(now, ...)`, and tells its caller when it next needs it: `poll_timeout() -> Option<Instant>`.
  TigerBeetle's replica counts logical ticks (`tick()`), and its replica code is not replayed from
  its log; only the state machine is. Silo's core does both jobs, and every input, ticks included,
  is written before it is applied. If the core says when it next needs the time, and every other
  input carries the time as a field (as the trial already does), the loop sends a tick only when a
  checkpoint or a timeout is due.
- **Evidence:**
  - quinn-proto `Connection` signatures. Verified: https://docs.rs/quinn-proto/latest/quinn_proto/struct.Connection.html
  - TigerBeetle `ReplicaType(StateMachine, MessageBus, Storage, Time, AOF)` and "Time is measured in
    logical ticks that are incremented on every call to tick()". Verified:
    https://raw.githubusercontent.com/tigerbeetle/tigerbeetle/0.16.30/src/vsr/replica.zig
  - TIGER_STYLE: "all loops and all queues must have a fixed upper bound". Verified, link above.
  - Zig has no private fields (proposal closed as not planned), so "a type only validation builds"
    rests on the source check. Verified: https://github.com/ziglang/zig/issues/9909
  - Not run: whether Zig lets another file build such a type through `.{ ... }`, `undefined`,
    `std.mem.zeroes` or `@bitCast` without naming it. By Zig's result-location typing, an anonymous
    literal does not need the type's name, so a non-public type does not stop it. The source check
    must refuse those forms too (see `engine-source-checks`).
- **Risks and what would change this:** if a forced write on a Mac costs well under a millisecond
  with grouping, ticks cost little and the clock part can wait. If the core turns out to need a
  periodic tick for many timers, it can still return the earliest deadline.
- **Proposed text:**
  > Nothing changes state except an input handled by the core. Calls, driver results and the clock
  > all enter the same way, through one queue. The queue is fed from a bounded place for each
  > source (this device, each computer, the drivers, the clock), taken in turn, and a call whose
  > place is full is refused with a named error. Room for the result of every effect under way is
  > kept when the effect is issued. Every input carries the time; the core says when it next needs
  > the clock, and the clock enters only then. Every input passes one validation on the way in,
  > which caps every field. Validation returns a type that only it builds, and the core accepts
  > only that type.
- **For the first build:** blocks `engine-program-and-clients` and `deterministic-core`. Agents
  can take it. The first build has calls from inside main (an agent running `silo`), so the bound
  per source is needed from the first day.

## `engine-state`: What form does the engine's state take?

- **Recommendation:** Take with an edit: say what a reference is.
- **Confidence:** medium. Flat tables with typed ids are the standard data-oriented practice; the
  edit closes one real bug class.
- **What it decides, on one real case:** main's checkpoint 41 is removed and its slot in the table
  is reused for checkpoint 57. A pending restore that held "slot 12" now restores the wrong
  checkpoint. Typed ids stop you from passing a computer id where a checkpoint id is expected; they
  do not stop a stale reference to a reused slot.
- **Options:**
  - As written. Leaves slot reuse open.
  - Rows refer to each other by id; an id is handed out from a counter that only grows and is never
    a position in a table. With `u64` counters the ids do not run out in practice (at a million a
    second, 2^64 lasts about 580,000 years), so "ids ran out" is a named error that is never
    reached. A lookup by id is a bounded scan or a fixed-size index. This matches `how-effects-work`
    ("ids are never reused").
  - Slot plus generation, as many game engines and the notes of `engine-client` say for handles.
    Costs two ways of naming one thing; the plan would then have both.
- **Evidence:** TIGER_STYLE asks for explicitly sized types ("avoid architecture-specific
  `usize`"). Verified, link above. The Zig compiler's own internals use `enum(u32) { _ }` typed
  indices; not verified here. The "different spaces, different types" lesson in the entry's notes
  is from Guillotine and is cited there.
- **Risks and what would change this:** a table that must be indexed by position for speed. The
  core tracks dozens of computers (`engine-shape` notes), so a scan is enough.
- **Proposed text:**
  > State is a small set of flat tables of plain rows that refer to each other by id. Each kind of id
  > has its own type, so two kinds cannot be mixed. An id comes from a counter that only grows, is
  > never handed out twice, and is never a position in a table.
- **For the first build:** blocks `engine-limits`, `engine-reconciles` and `engine-matches-wanted`.
  Agents can take it; the check is the compiler plus a simulator property that a removed row's id
  never resolves again.

## `engine-source-checks`: What does the check refuse in the engine's code?

- **Recommendation:** Take with an edit: replace the one rule that cannot be checked, add the ways
  around the others, and require a planted fault for each rule.
- **Confidence:** high on what cannot be checked; medium on the exact list.
- **What it decides, on one real case:** a builder writes, in the core,
  `const end = start +% length;` where `length` came from a call. The rule "refuses wrapping
  arithmetic on a value that came from an input" needs to know where `length` came from. A test
  that reads the source sees names, not where a value flowed from. To find it, the check would need
  taint analysis, which nobody has for Zig. What the check can do is refuse `+%`, `-%`, `*%`
  everywhere in the core except in one helper file. Plain `+` already stops on overflow in
  ReleaseSafe.
- **Missing ways around the rules:**
  - `@setRuntimeSafety(false)` turns off every safety check in its scope, in ReleaseSafe too
    (Zig reference). One line undoes `assertions-stay-on` locally.
  - `@truncate` drops bits with no check; it is the unchecked narrowing the entry means.
  - `@ptrCast`, `@bitCast` into a row type, `undefined` and `std.mem.zeroes` of a validated type:
    ways to build a type outside its file without a literal.
  - `@import` of anything outside the list of `core-dependencies`.
  - A `while` with no bound (see `tigerstyle-by-place`).
- **What TigerBeetle actually checks.** Its `tidy.zig` (tag 0.16.30) refuses: listed strings of
  `std` with their replacement, `FIXME` (not `TODO`), `dbg(`, `Self = @This()`, lines over 100
  columns, tabs and `\r`, non-public declarations used only once (dead code), `.zig` files nobody
  imports (dead files), generic functions not named `...Type`, large blobs in Git, and unknown file
  types. Function length is a ratchet: `function_line_count_max = 459`, and the test fails if the
  constant is larger than the longest function, so it can only go down. TigerStyle's 70-line rule
  is written in the style guide, not enforced by this check. Verified, link above.
- **Options:**
  - As written. One rule cannot be built; three ways around the others stay open.
  - As written, with the edit below. Adds dead code and dead files, which agents leave behind more
    than people do, and a fixture per rule, which `check-must-catch-a-fault` asks for anyway.
  - Take TigerBeetle's `tidy.zig` as a start. Costs little; it is Apache 2.0. Its banned strings are
    for its own `stdx`, so the list is Silo's to write.
- **Evidence:** Zig 0.15.2 reference on `@setRuntimeSafety`, `@intCast`, `@truncate`, build modes.
  Verified: https://ziglang.org/documentation/0.15.2/ . TigerBeetle `tidy.zig`, verified, link
  above. The trial's source check read Zig's parse tree for the variable kept outside the state
  (notes of `deterministic-core`): an AST-based check exists already.
- **Risks and what would change this:** a text check misses spellings nobody thought of
  (`const s = std;` in the trial). The trial answered that with the build for no operating system,
  linked. Keep both.
- **Proposed text:**
  > In the core, the check that reads the source refuses: a bare `unreachable`; `@setRuntimeSafety`;
  > `@truncate`, `@ptrCast` and `@bitCast`, and a narrowing `@intCast`, outside one checked helper;
  > the wrapping operators `+%`, `-%` and `*%` outside that helper; `@import` of anything outside the
  > list of `core-dependencies`; a print or a log; `TODO`, `FIXME` and `@panic` with no named reason;
  > and building a type outside the one file allowed to build it, such as a validated input, whether
  > by a literal, `undefined`, `std.mem.zeroes` or a cast. In all engine code it refuses code that
  > `zig fmt` would change, a declaration nothing uses and a file nothing imports. Each refused form
  > has a file that holds it, and the check must refuse that file.
- **For the first build:** blocks `engine-program-and-clients`. Agents can take it. This is the one
  entry the builder writes before any feature code, so the edit is worth making first.

## `tigerstyle-by-place`: Which rules of TigerStyle hold where?

- **Recommendation:** Take with an edit: add the places it leaves out, and word the two core rules
  that cannot be checked as checks that can.
- **Confidence:** medium. It rests on TIGER_STYLE, on TigerBeetle's own enforcement, and on the
  taken `macos-vm-driver`.
- **What it decides, on one real case:** the first build's helper for main runs a macOS virtual
  machine. `macos-vm-driver` (taken) says the code that drives Apple's framework is Swift. So the
  helper for main is, at least in part, Swift. The entry says "the helper keeps the safe build, a
  memory cap fixed at start, named failures and fuzzing" and "the check script holds each list for
  its directory". None of those is said for Swift: there is no ReleaseSafe in Swift, the check
  script reads Zig, and Swift's error model is not Zig's.
- **Places missing:**
  - The Swift code of the helper. Its safe build is `-O`, never `-Ounchecked`, which removes overflow
    checks and does not evaluate `precondition` (Swift docs and forums; see below). Its check needs
    a Swift reader (for example `swift-format lint` plus a script), not the Zig one.
  - The probe, which runs inside the computer, where an agent with `sudo` can hand it anything. It
    is Silo code facing hostile input, and it is not on any list.
  - The simulator, the fakes and the checks themselves. TigerBeetle runs its own tidy over its test
    code; a fake that swallows a failure makes the simulator check a fiction.
- **Two core rules that cannot be checked as written:** "every loop bounded" and "no recursion".
  Reading the source cannot prove a `while` ends, and Zig does not refuse recursion. TigerBeetle does
  not check either mechanically. A check that can: in the core, a loop is a `for` over a table or a
  slice, which ends by construction, and a `while` is refused except on one list, each with its
  bound in a comment; recursion is refused where a function calls itself by name (the indirect case
  stays a review point).
- **The quotas:** dropping "two assertions per function" and "seventy lines" is consistent with what
  TigerBeetle itself enforces: no assertion count, and a function-length ratchet at 459 lines
  rather than 70. The notes say the quotas are "the one point put to polarzero". That contradicts
  listing the entry for agents. polarzero asked to keep only opinionated decisions
  (`owners-part-on-engine` notes); a quota is engineering practice. Suggest the notes drop that line,
  and the check adopt TigerBeetle's ratchet for function length.
- **Evidence:**
  - TIGER_STYLE quotes (70 lines, two assertions, static allocation, bounded loops, no recursion,
    100 columns, `u32` over `usize`). Verified, link above.
  - TigerBeetle `tidy.zig`: function length is a ratchet at 459, no assertion count. Verified, link
    above.
  - TigerBeetle `static_allocator.zig`: allocation allowed in `init`, refused by assertion in
    `static`. That is how "no allocation after start" is checked at run time. Verified:
    https://raw.githubusercontent.com/tigerbeetle/tigerbeetle/main/src/static_allocator.zig
  - Swift `-Ounchecked` removes overflow checks and does not evaluate `precondition`. Verified from
    search results quoting Apple's `precondition` reference and the Swift forums thread
    https://forums.swift.org/t/retain-overflow-checks-when-compiled-with-ounchecked/54450 ; the
    Apple page itself was not opened.
  - Apple DTS, on the helper's life: "the app that invokes Virtualization framework is responsible
    for any VMs it creates. If the app terminates unexpectedly, the system should clean up its
    resources, and that includes killing any VMs". This agrees with `helper-on-broken-assumption`.
    Verified: https://developer.apple.com/forums/thread/848542?answerId=907172022
  - Not verified: whether Xcode's Swift toolchain ships libFuzzer for macOS; swift.org toolchains do
    on Linux. "Fuzzing" for the Swift helper may need the swift.org toolchain or a Zig harness.
- **Risks and what would change this:** if the helper turns out to be Zig with a thin Swift layer
  called through C, most of the helper's list holds as written and only that layer needs Swift
  rules. Which it is follows `macos-vm-driver` and is the builder's first choice.
- **Proposed text:**
  > In the core, without exception: the shipped build is the safe build and every stop is a defined
  > stop; no allocation after start, and fixed tables; a loop is a `for` over a table or a slice,
  > and any other loop is on one list with its bound; no function calls itself; no dependency, clock
  > or thread; checked arithmetic and narrowing; every failure named and handled. TigerStyle's quotas,
  > two assertions and at most seventy lines per function, are not held; function length is held by
  > a limit that only goes down. The helper, the recorder and the probe keep the safe build, a limit
  > on everything an agent can grow, a memory cap fixed at start, named failures and fuzzing; they
  > may allocate under that cap, run threads and depend on pinned libraries. Swift code is built
  > with `-O` and never `-Ounchecked`. The client, the command line, tools, the simulator and the
  > fakes may allocate and depend on libraries, and never swallow a failure. The check script holds
  > each list for its directory, in each language used there.
- **For the first build:** blocks `engine-program-and-clients`. Agents can take it with the edit.
  The builder needs the Swift line before writing the helper for main.

## `how-the-engine-is-built`: How is the engine built?

- **Recommendation:** Take with an edit. polarzero takes it: the notes hold polarzero's own four
  lines.
- **Confidence:** medium. The direction is backed by Guillotine's evidence in the notes (rules in a
  prompt, 26 `catch {}` in the code). The doubt is the reader.
- **What it decides, on one real case:** an agent writes the first ten files: the core's first call,
  the validation file, the simulator, the source check, the Swift helper that starts main, and its
  fake. "A person reads those line by line." On 18 November polarzero said agreeing with engine
  statements polarzero cannot assess brings nothing (`owners-part-on-engine` notes). Reading Zig and
  Swift line by line is the same problem, larger. If no engineer is named, either the first build
  waits for one, or the reading becomes a quick look that the plan then counts as a review.
- **Options:**
  - As written. Costs an open question at the start of the build: who.
  - Name the reader before the build starts: an engineer polarzero trusts, recorded with the other
    material for polarzero alone (in Linear's "Insights", as `AGENTS.md` asks), and paid for an
    agreed number of files. polarzero reads too, with an agent beside each file that says what each
    part does and which check holds it. This is the outside look the notes ask for, made concrete.
  - Agents read the first files. The notes already weigh it: an agent accepts another agent's style.
- **Two smaller points:**
  - "Coding conventions are mechanical checks, not text in a prompt." Some conventions cannot be a
    check: a name that says what a thing is, a comment that says why a limit has its value. Read
    strictly, they may then not be written down at all. Suggested: a convention a check can hold is
    a check; one that cannot is written once in a guide and read in review.
  - "The simulator and the checks before the features." The first build's features are mostly
    drivers (the macOS virtual machine, checkpoints, SSH), which the simulator does not check
    (`what-checks-the-real-drivers`, taken). Read as "before the first feature", the Swift helper
    waits for a simulator that does not test it. Suggested: each check before the code it checks.
  - "A property's statement is written by a person" is already in `plan/README.md` ("a property's
    sentence is written by a person"). It is polarzero's line, so it stays; the duplication is
    noted only.
- **Evidence:** the Guillotine counts in the notes of `engine-source-checks` (not re-verified here);
  TigerBeetle's tidy as the model of conventions held by a check (verified above).
- **Risks and what would change this:** if polarzero names an engineer, the first option needs no
  edit beyond the name.
- **Proposed text:**
  > Verification comes first: each check exists before the code it checks. The first engine files
  > become the pattern every later agent copies, so a person reads those line by line: an engineer
  > polarzero names before the build starts, and polarzero with an agent that explains each file.
  > Coding conventions that a check can hold are mechanical checks, not text in a prompt. A property's
  > statement is written by a person.
- **For the first build:** blocks `engine-source-checks` and `engine-program-and-clients`. It is the
  one entry of the eight that needs polarzero, and the reader is the one thing the builder cannot
  supply.

---

## Cross-cutting findings

### The loop that feeds the core is outside the simulator

Silo's core is sans-IO, like quinn-proto, h11 and rustls: a pure function from state and input to
state, events and effects. That makes the core easy to check, and the trial showed it: a million
histories in 2.66 s.

TigerBeetle and FoundationDB draw the line further out. TigerBeetle's replica takes its storage, its
message bus and its clock as parameters (`ReplicaType(StateMachine, MessageBus, Storage, Time, AOF)`),
and the simulator passes fakes, so the code that writes the log and handles timeouts is the code
that is simulated. FoundationDB runs the whole cluster in one thread with its network and disks
swapped for simulated ones (Net2 for Sim2 behind one network interface, by third-party accounts),
and `BUGGIFY` adds faults only in simulation.

In Silo, the code most likely to lose a call after a crash is not in the core: writing each input
before it takes effect (`engine-saved-state`), the call-id table on restart (`call-ids`), sending
events again to the recorder (`events-until-the-recorder`), reconciling at start
(`multi-step-operations`). The properties `accepted-call-never-forgotten` and `crash-finishes-or-rolls-back`
name the simulator as their check. The trial's simulator crashed "after the core answered but
before its answer was kept", so it modelled the keeping, but with its own code, not the engine's.

Proposed: one sentence, put in `core-dependencies` (above) or in `engine-inputs`: the loop that
feeds the core reaches the disk, the clock and the drivers only through interfaces given at start,
so the simulator runs that same loop over fakes. It sets nothing and costs one interface per kind
of outside thing, which the fakes need anyway (`fakes-share-contract-tests`, taken).

Sources: TigerBeetle replica, verified (link above); FoundationDB testing page, verified for "an
entire FoundationDB cluster within a single-threaded process": https://apple.github.io/foundationdb/testing.html ;
Net2/Sim2 and BUGGIFY, not verified at source, from https://pierrez.github.io/fdb-book/meet_fdb/correctess.html
and https://www.slideshare.net/slideshow/deterministic-simulation-testing/39333008 ; sans-IO
definition, verified: https://sans-io.readthedocs.io/how-to-sans-io.html

### "Written before applied" is stronger than it needs to be

`engine-saved-state` (not one of these eight, but every one of them stands on it) says an input is
written to disk before the core applies it. What the promises need is weaker: no event, effect or
answer leaves the engine before the input it came from is written. The core is deterministic, so
applying an input that is then lost is harmless as long as nothing it produced left. With that
wording the loop can apply twenty inputs, write them in one forced write, then release their
outputs. That is how databases group commits, and it answers the "whether writes have to be
grouped" question of `engine-saved-state` and `events-until-the-recorder` without a separate design.
On a Mac, `fsync` does not reach the drive and `F_FULLFSYNC` does, at a cost of milliseconds (from
the first-build proposal; not measured here). Suggest the agent on `engine-saved-state` weigh this.

### Checks that cannot fail, and `check-must-catch-a-fault`

Two of these eight name a check that cannot catch the fault it is for (`core-state-stays-small`,
`engine-source-checks`). `owners-part-on-engine` lets agents take an entry only when "a check can
show it holds". Suggest the judge for every agent-taken entry ask one question first: which planted
fault does this check refuse? An entry with no answer is not ready.

### Ids: one way, not two

`how-effects-work` notes say ids are never reused; the notes of `engine-client` (taken by agents)
say handles are "ids with a generation, not pointers". With `u64` ids from a counter that only
grows, the generation is never needed. Suggest the client's handles are the engine's ids, and the
generation is dropped from those notes, so one way of naming a row exists (`one-name-per-concept`).

### Swift is part of the engine, and most of the plan's engine rules are written for Zig

`macos-vm-driver` (taken) puts Swift on the engine side. `engine-states-and-failures`,
`engine-source-checks`, `assertions-stay-on` and `tigerstyle-by-place` are worded in Zig terms
(`switch` with no `else`, ReleaseSafe, `zig fmt`). The first build's most important process, the
helper for main, is where Swift runs. Before the first Swift file, one entry should say which of
those rules hold in Swift and how they are checked. Swift has exhaustive `switch` over enums without
`default`, typed `throws` since Swift 6, and `-O` keeps overflow traps, so most translate. Not
verified here: typed throws' exact release; it is from memory.

### A challenge to a decision in force

`engine-shape` and `deterministic-core` are not taken yet, but `core-simulator` (taken by agents)
says the simulator "can drive [the core] with millions of random sequences of inputs, crashes
included". The core cannot crash in a way that matters: it holds no disk. Crashes matter in the
loop. So `core-simulator`, as worded, places the crash checking in the one part that cannot lose
anything. The finding above fixes it without replacing `core-simulator`: once the loop runs in the
simulator over fakes, "crashes included" means the loop's crashes. If polarzero or the judge
prefers it said where it belongs, a decision that replaces `core-simulator` with "The core and the
loop that feeds it touch nothing but interfaces given at start, so a simulator can drive them..."
would do it; it is about how Silo is built, so agents may take it.

### Order

All eight are needs of `engine-program-and-clients`, the first feature after `access-model-checked`.
Seven can be taken by agents with a judge once edited; only `how-the-engine-is-built` needs
polarzero, and only for one thing: who reads the first files.
