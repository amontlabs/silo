# The engine's shape and its promises: a second opinion

An agent's review, 24 November 2026, for polarzero to rule on five decisions of the part
`engine`. It changes nothing in `plan/`. Each section gives a recommendation and the evidence
behind it. "Verified" means I read the source, the document or ran the measurement. "Not
verified" means it comes from memory or from a summary I could not check against the original.

| id | Recommendation | Confidence | Reason |
|---|---|---|---|
| `engine-shape` | Take as written | high | The helper process already forces asks and late answers between the engine and the virtual machine, so splitting the core from the drivers adds little cost. It also makes the first build's one hard case testable: restore and checkpoint racing on one disk. |
| `deterministic-core` | Take with an edit | medium-high | Same inputs give the same result only for the same build of the core. Replaying a log with a newer engine is where Temporal and TigerBeetle both broke. The second property also says "calls" where it means inputs. |
| `engine-limits` | Take with an edit | medium | Fixed tables are right. But an action the engine starts on its own, such as an automatic checkpoint, would hit a full table and be refused with no caller to read the refusal. The first build, which has no thinning, hits this within days. |
| `engine-reconciles` | Take with an edit | high | Its liveness property has no number, which goes against the taken `liveness-has-a-number`. Its "or says what it is waiting for" also lets an engine that only ever waits pass the check. Retries have no back-off. |
| `how-effects-work` | Take with an edit | medium-high | The core is right. Three changes are needed. A timeout is "not known", not "failed". The effect's key names what it makes, so a retry cannot make a second checkpoint. And `no-effect-twice` needs a test on the real drivers, because a simulator only sees the fakes. |

Is the cost justified for a first build whose engine drives one virtual machine? Yes for the shape:
the core, the drivers, effects that are answered later, and the core's determinism. A trial on
21 November already built a core of this shape in 322 lines with a simulator of 201
(`core-simulator`, notes). The cost is not in the core. It is in three places that these five
entries pull in. The log replayed through the core at start (`engine-saved-state`) is one: in
practice, it is what makes a newer engine version dangerous. A fake for each driver with a
written fault model (`simulator-faults`) is the second. The third is a fixed limit on every table,
which does not suit a list that grows by itself, such as the checkpoints. Sections below say how
to keep the shape and narrow each of the three.

## `engine-shape`: How is the engine cut into parts?

- **Recommendation:** Take as written. Silo's processes already split this way, the trial showed
  that the core is small, and the first build's riskiest bug only shows up in a test when the
  core and the drivers are separate.
- **Confidence:** high. It rests on the trial's numbers (`core-simulator`), on `processes`, and on
  well-documented prior art (sans-I/O, Elm, TigerBeetle, FoundationDB).
- **What it decides, on one real case:** It is 3 a.m. The engine's clock asks for main's
  automatic checkpoint, so the helper starts an APFS clone of main's disk image. Meanwhile your
  script, run from your Mac, has sent `silo restore main <checkpoint>`. The restore stops main and
  swaps its disk image for a clone of the older checkpoint. If the checkpoint's clone finishes
  after the swap, it holds either the old disk or the new one, under a time that says neither.
  With a core that decides and touches nothing, a simulator plays this order, and every other
  order, with the clone's answer delayed, lost or doubled. The rule that one operation runs at a
  time on each computer is then checked, not hoped for. Written as one ordinary program, this
  shows up only when the timing happens to line up on a real Mac.
- **Options:**
  1. *Core and drivers apart (the answer).* It costs one indirection: every action is an effect
     sent out, and an answer that comes back later as an input. This is the same design as
     sans-I/O protocol libraries (h11, h2), Elm's `Cmd`/`Msg`, Gary Bernhardt's "functional core,
     imperative shell", TigerBeetle's state machine and FoundationDB's Flow actors.
  2. *One ordinary program.* Quicker for the first file, and how Tart, UTM and Lima are written.
     None of those three keeps checkpoints of its own or promises anything about crashes, which
     Silo does (`accepted-call-never-forgotten`, `crash-finishes-or-rolls-back`).
  3. *An actor runtime with supervisors (Erlang/OTP style).* Each computer is a process with its
     own state, and a supervisor restarts what crashes. OTP accepts crashes and restarts. It
     does not make a run replayable. Silo already takes the useful half of it: the helper per
     computer (`processes`), and a restart limit of three stops in ten minutes
     (`computer-stops-by-itself`), which is OTP's `intensity` and `period`.
- **Evidence:**
  - The extra cost is small because the asynchrony is already there. `processes` puts the
    virtual machine in a helper process, and `macos-vm-driver` puts that code in Swift. So the Zig
    engine talks to the virtual machine over a socket in every design. An answer can be lost, can
    come late, or the helper can die, and the engine has to handle each case. Apple's
    `VZVirtualMachine.start(completionHandler:)` and its delegate's `guestDidStop` and
    `didStopWithError` are asynchronous already. (The process design: verified, plan. The shape of
    Apple's API: not verified here, since Apple's pages render only with JavaScript. It is from
    memory.)
  - Trial: a core of 322 lines, a simulator of 201 and properties of 92. One million histories
    ran in 2.66 s, and two planted faults were found within 3 and 11 histories
    (`core-simulator`, notes; verified as written in the plan, not re-run).
  - Sans-I/O: "contains no code that does any form of network I/O or any form of asynchronous
    flow control". Its claimed results are tests that are never flaky and full branch coverage.
    https://sans-io.readthedocs.io/how-to-sans-io.html (verified).
  - FoundationDB: "All database code is deterministic; multithreaded concurrency is avoided". After
    the team replaced ZooKeeper with a Paxos of its own under simulation, "No production bugs have
    ever been reported since". Summary of the SIGMOD 2021 paper:
    https://muratbuffalo.blogspot.com/2022/03/foundationdb-distributed-unbundled.html (verified on
    the summary, not on the paper).
  - OTP restart intensity: when there are more restarts than `intensity` within `period`, "the
    supervisor terminates all the child processes and then itself".
    https://www.erlang.org/doc/system/sup_princ.html (verified).
- **Risks and what would change this:** The shape fails if the drivers grow their own decisions.
  An example is a Swift helper that retries a clone by itself and reports success: the core's
  model is then wrong, and the simulator still passes. The guard is that a driver holds no rule
  of when or whether to act, only how. That is worth one line in `tigerstyle-by-place` or in the
  check script. What would flip the recommendation: a first build that needs no promise about
  crashes or repeated calls. The plan has several.
- **The words it sets:** "call", "event" and "effect" are fine. "call" sits next to the word
  "tool call" and the "model call" that the spend features use. A record line that says "the
  call failed" will be read three ways. Under `one-name-per-concept`, the word's `not` list (not
  part of polarzero's yes) should name "request" and "command". The record's text should never
  say a bare "call" for a tool call or a model call. The word "engine" says "every tool only talk
  to it". `ssh main` from your editor reaches main without going through the engine. That is fine
  because "tool" here means Silo's own tools, but a reader may stumble on it.
- **For the first build:** It blocks `engine-program-and-clients` and everything built on it.
  Nothing to narrow. The builder needs one more rule from it, written in the notes or in
  `multi-step-operations`: one operation at a time on each computer (checkpoint, restore, stop,
  start), with later ones queued in a table of fixed size. The case above is that rule.

## `deterministic-core`: Is the core deterministic?

- **Recommendation:** Take with an edit. Two changes. Determinism is promised for one build of
  the core, and recovery must not rest on replaying old inputs through new code. The clock reaches
  the core when the core asks for it, not on every tick. Also fix "calls" in the second property.
- **Confidence:** medium-high. It rests on the Jepsen report on TigerBeetle, on TigerBeetle's
  upgrade design, and on Temporal's documentation.
- **What it decides, on one real case:** You update Silo from version 0.3 to 0.4 while main runs.
  Version 0.4 changes how the core counts down to the next automatic checkpoint. The engine
  starts, loads the last save and applies the 200 inputs logged since, as `engine-saved-state`
  says, but with 0.4's core. The checkpoint that 0.3 asked for at input 150 is now asked for at
  input 160 instead. Effect ids and event numbers move. The recorder already wrote events up to
  number 180 from 0.3 (`events-until-the-recorder`). The same number now means a different event.
  Nothing in "same inputs, same result" catches this, because the inputs are the same and the
  code is not.
- **Options:**
  1. *The answer as written.* It holds within one build. It is silent across builds.
  2. *Pin replay to the build that wrote the log (TigerBeetle).* TigerBeetle ships several old
     versions inside one binary, so that "a given request will always be executed by the exact
     same binary version… even when the log is replayed". This is heavy machinery for a program
     on one Mac.
  3. *Save at a clean stop, and refuse cross-build replay (proposed).* Before a new version
     starts, the old engine stops cleanly and saves its state with the log empty. A log written by
     another build of the core is not replayed. The engine starts from what the drivers say exists
     and reconciles, which it does already (`engine-reconciles`, `multi-step-operations`). For one
     user on one Mac, upgrades come through Silo's own updater, so a clean stop first is in reach.
     The case left over is a crash just before an upgrade. Reconciling covers it.
  4. *Patch markers in the core (Temporal).* Every change to the core's logic is guarded by a
     version check. Temporal's own guidance now pushes "Worker Versioning", which pins each run to
     one build, because hand-written patch markers kept failing in production.
- **Evidence:**
  - Jepsen's analysis of TigerBeetle 0.16.11 to 0.16.30 found four crashes and panics on
    *upgrades between versions*. One was "panic: checkpoint diverged". Another was "switch on
    corrupt value" on a message type the new code no longer handled. TigerBeetle "lacked fuzz
    tests for cross-version upgrades". TigerBeetle has the strictest deterministic core and
    simulator of any comparable project, and the bugs it shipped were where two versions meet.
    https://jepsen.io/analyses/tigerbeetle-0.16.11 (verified, through a summary of the page).
  - TigerBeetle multiversion binaries: https://tigerbeetle.com/newsletters/2024-08-02-july-in-tigerland
    (verified).
  - Temporal: a change that alters the order or type of commands for the same history cannot be
    deployed against existing histories. Replay testing in CI and Worker Versioning are the fixes.
    https://docs.temporal.io/develop/safe-deployments (verified, through a search summary). Forum
    reports of non-determinism errors after adding versioning:
    https://community.temporal.io/t/non-determinism-after-versioning/7527 (not verified in detail).
  - The clock. The answer makes time an input. `engine-saved-state` writes every accepted input to
    disk before applying it. Together they mean that, if the clock ticks every second, the engine
    forces 86,400 writes a day with F_FULLFSYNC on a Mac (cost per write unmeasured; it is Mac test
    4 of the first-build proposal). Almost none of those writes change anything. TigerBeetle keeps
    clock ticks out of its log: its log holds the client requests, each stamped with a time by the
    primary (from memory, not verified here). Elm gives a program the time only when the program
    asks for it, through a subscription such as `Time.every`. The fix for Silo is that the core
    returns the time it next needs to be woken at. An input that changes nothing is not logged,
    and a call carries the time at which it was accepted.
  - Measured here (ext4 on a virtual disk, Linux, not a Mac). Appending 256 bytes and an fsync
    took 692 µs on average. Rewriting the whole state of 4 KB to 64 KB through a temporary file,
    with fsync, rename and an fsync of the directory, took 2.2 to 2.8 ms. 1 MB took 4.5 ms. The
    first build's state is a few kilobytes: one computer, a few hundred checkpoints, the pending
    effects and the call ids. Saving it whole after each input that changes it costs about 3 to 4
    times an append on this disk, and never needs a replay. That is an option for
    `engine-saved-state` worth timing on a Mac. Script:
    `exp/fs.py` in the project folder `research/2026-11-24-first-build-decisions-scripts/`.
  - The trial of nine ways to read the clock (`deterministic-core`, notes) is good evidence that
    the check holds. Nothing here contradicts it.
- **Risks and what would change this:** If Silo will never update the engine while computers run,
  the cross-build case is smaller, but the recorder's event numbers still span the update. A Mac
  measurement showing that F_FULLFSYNC costs well under a millisecond would make logging every
  tick affordable. It would still be useless.
- **Proposed text:**
  - `answer`: "The same inputs in the same order, applied by the same build of the core, give the
    same state and the same outputs. Time and randomness are inputs: the core says when it next
    needs the time, and is given it then and with each input. The core runs on one thread. Inputs
    saved by one build of the core are never applied by another."
  - `sets`, second property: `{"in":"engine/properties","id":"core-keeps-nothing-between-calls","says":"The core keeps nothing between two inputs outside its state.","kind":"safety","check":["test"]}`.
    The id can stay, or become `core-keeps-nothing-between-inputs` to match the sentence; renaming
    it is better under `one-name-per-concept`. "Call" is the word for what a client sends. A clock
    tick or a driver result is not a call, yet the property must cover them.
- **For the first build:** It blocks `engine-program-and-clients`. The edit narrows
  `engine-saved-state`'s work: no cross-version replay to build, and no log of ticks. The first
  build ships one version at a time, so the clean stop before an upgrade is one step in the
  updater.

## `engine-limits`: What limits the engine's tables and memory?

- **Recommendation:** Take with an edit. Fixed tables and named errors are right. Two things are
  missing. A refusal of something the engine started on its own needs a reader. A table that grows
  by itself, such as the checkpoints, needs a rule that frees rows before it fills, in the same
  build. Without one, the first build stops taking checkpoints.
- **Confidence:** medium. It rests on arithmetic over the plan's own numbers. That the checkpoint
  list is a table of the core is inferred from `core-state-stays-small` ("the core's saved state
  holds the current rows only"), and not stated anywhere.
- **What it decides, on one real case:** In the first build, main takes a checkpoint every 5
  minutes. That is 288 a day, more with checkpoints at each ask. Say the checkpoint table holds
  1,024 rows. It is full after 3.6 days. The next automatic checkpoint is refused "with a named
  error", but no caller is there to read it. Your agents keep working. A week later you want to go
  back to yesterday afternoon, and there is nothing after day four. The first build left out
  `thinning-old-checkpoints` (see the first-build proposal, "The features"). Thinning by the plan's
  own rule (`thinning-checkpoints`) keeps 24 + 48 + 30 = 102 automatic checkpoints at a 5-minute
  interval. So a table of a few hundred rows is enough once thinning exists. Without thinning, no
  fixed size is enough.
- **Options:**
  1. *The answer as written.* Right for calls. For what the clock starts, it fails without a
     sound.
  2. *Fixed tables, plus a rule for what the engine starts on its own (proposed).* An action the
     engine starts itself that meets a limit is refused in the same way, and the refusal becomes
     a line of the record and an alert. Every table that grows without a call also has a rule
     that frees rows, built with it.
  3. *Grow as needed.* This is the loop of ten thousand forks in the notes. Not recommended.
- **Evidence:**
  - TigerBeetle allocates everything at start and limits its clients: past `clients_max`, the
    oldest client session is evicted, and the evicted client gets an error. Jepsen found that this
    eviction "crashed the whole process" in the Java client (#2484). So the limit was right, and the
    path where the limit is hit was the untested one.
    https://jepsen.io/analyses/tigerbeetle-0.16.11 (verified, through a summary). TigerStyle on
    static allocation: https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/TIGER_STYLE.md
    (not re-read for this review).
  - Temporal caps a workflow's history (51,200 events or 50 MB) and asks a workflow to "continue as
    new" before then. It is a fixed limit with a built-in way out, the same shape as thinning (from
    memory, not verified here).
  - The property `nothing-grows-without-limit` says "Nothing grows without limit." Read alone, as
    the Workshop shows it, it reads as a promise about all of Silo. Yet checkpoints on disk and the
    record do grow, up to limits that `thinning-checkpoints` and the record set elsewhere. Its
    check, `assertion`, can show that a table is never over its size. It cannot show "nothing".
- **Risks and what would change this:** If checkpoints are not rows of the core but files the
  helper lists, the case moves to the helper, which is also bounded (`tigerstyle-by-place`). The
  silent stop is the same. That "raising a limit takes a new version of Silo" (notes) is a price
  the user pays, and under `owners-part-on-engine` it is polarzero's to accept. It is acceptable
  only if the limits are set well above any real use, and the first build will be the first
  measurement of real use.
- **Proposed text:**
  - `answer`: "Limits are part of the model. Tables have a fixed size. Reaching a limit fails with
    a named error. An action the engine starts on its own that meets a limit is refused in the same
    way, and the refusal is recorded and alerts you. A table that grows without a call has a rule
    that frees rows before it is full. No memory is allocated in the core after start. The limits
    are one structure that the core is built from, and a check refuses impossible limits when
    compiling."
  - `sets`, first property: `{"in":"engine/properties","id":"nothing-grows-without-limit","says":"No table or buffer of the engine grows without limit.","kind":"safety","check":["assertion","test"]}`.
    The added `test` is the one the notes already describe: it prints the engine's total memory and
    fails when a row grows.
- **For the first build:** It blocks `engine-program-and-clients`. It also shows a gap in the
  first-build proposal: either `thinning-old-checkpoints` joins the first build, or that build
  holds a smaller, fixed rule (keep the last N automatic checkpoints and remove the oldest). Note
  that `thinning-old-checkpoints` needs `light-checkpoints`, `opinion-never-chooses-what-is-kept`
  and `kinds-of-other-actions`, so taking it in pulls more decisions. The smaller rule is easier.

## `engine-reconciles`: How does the engine bring what is seen to what is wanted?

- **Recommendation:** Take with an edit. The design is the known one, the Kubernetes controller's.
  Three changes are needed. The property needs a number. Its way out has to apply only while
  faults last. Retries need a back-off. An observation older than one already applied must not
  undo it.
- **Confidence:** high. It rests on a taken decision of the plan, and on documented Kubernetes and
  TigerBeetle practice.
- **What it decides, on one real case:** Your Mac restarts overnight for an update. In the morning
  the engine starts and main's row says it was wanted as running. The helper is gone, so main is
  unknown, and the engine starts it. Say main's disk sits on a drive that is now full. The start
  fails, and the engine starts it again on the next tick. With no back-off, that is a start every
  second, each with a line in the record and perhaps an alert. `computer-stops-by-itself` stops
  after three tries in ten minutes, but only for a computer that stopped by itself, and a start
  that fails is not that case. Meanwhile the property as written, "agree in the end, or the
  engine says what it is waiting for", is satisfied: the engine says it is waiting.
- **Options:**
  1. *The answer as written.* Right design. The property it sets can always be passed.
  2. *Level-triggered reconciling with back-off and a bounded property (proposed).* This is what
     Kubernetes does: a work queue with exponential back-off per object, and each reconcile reads
     the current state, not the events. TigerBeetle's simulator runs in two phases for liveness. It
     injects faults, then heals them and checks that the cluster converges (from memory, not
     verified here). That second phase is the one that gives "in the end" a number.
  3. *Trust what was saved.* Not recommended, for the reason in the notes.
- **Evidence:**
  - The taken `liveness-has-a-number` says "Each liveness property gives 'in the end' a number."
    `wanted-and-seen-agree` is a liveness property with no number. As written, the take of
    `engine-reconciles` would bring in a property that breaks a decision in force. (Verified, plan.)
  - Level-triggered controllers: Kubernetes controllers "Reconcile the state of the system at the
    time that Reconcile is run". https://kubectl.docs.k8s.io/guides/introduction/resources_controllers/
    and Tim Hockin, "Edge vs. level triggered logic", https://speakerdeck.com/thockin/edge-vs-level-triggered-logic
    (verified, through a search summary).
  - Stale observations are Kubernetes' standing problem. Its ReplicaSet controller keeps
    "expectations" of the creates and deletes it has not yet seen, and does not act until they are
    met or expire. Kubernetes 1.36 (April 2026) added tracking of each object's resource version
    so that a controller skips a sync while its cache is behind. The post says staleness is often
    found "when a controller in production has already taken incorrect action".
    https://kubernetes.io/blog/2026/04/28/kubernetes-v1-36-staleness-mitigation-for-controllers/
    and KEP-5647 https://www.kubernetes.dev/resources/keps/5647/ (the post's opening verified, its
    mechanism through a search summary). In Silo the same thing happens when a report of main as
    running arrives after the report that it died. `how-effects-work`'s notes already match
    results by "the computer's restore count". The same counter should order observations.
  - NATS 2.15's reconciling, cited in the notes:
    https://www.synadia.com/blog/how-we-move-streams-safely-nats-2-15-desired-state-reconciliation
    (not re-read).
- **Risks and what would change this:** A number that is too tight fails on a real Mac: macOS
  takes tens of seconds to boot in a virtual machine, and a restore copies a disk. So the number is
  given in simulated time and comes from the engine's own limits, not from wall-clock hopes.
- **Proposed text:**
  - `answer`: "Each row keeps what was asked and what was last observed. While they differ, the
    core issues effects to make them match, or says what it is waiting for. What was never observed
    is unknown, and unknown never counts as matching. An observation older than the last one
    applied to the same row changes nothing. A try that fails is tried again after a delay that
    grows each time, up to a fixed longest delay. A virtual machine can die by itself and the
    device can reboot, so the saved state is never assumed to describe the world."
  - `sets`: `{"in":"engine/properties","id":"wanted-and-seen-agree","says":"Once faults stop, what is wanted and what is seen agree within the longest retry delay plus the longest effect timeout. Until then the engine says what it is waiting for.","kind":"liveness","check":["simulator"]}`
- **For the first build:** It blocks `engine-matches-wanted`, which the first build needs so that
  main comes back after a restart. The edit costs a few lines in the core and one phase in the
  simulator.

## `how-effects-work`: How does the core ask a driver to act?

- **Recommendation:** Take with an edit. Asking and hearing back later is right. Three changes. A
  timeout is reported as "not known yet", not as a failure. The once-only key names what the
  effect makes, so a retry after a timeout is the same effect. The property's check adds a test of
  each real driver against the same cases as its fake.
- **Confidence:** medium-high. It rests on the Jepsen report on TigerBeetle, on Dropbox's account
  of its sync tests, and on how `clonefile` behaves.
- **What it decides, on one real case:** The engine asks main's helper for checkpoint 412. The
  APFS clone takes 40 seconds because the disk is in many pieces (draft PR #347 saw 11 to 23 s
  stalls on XFS). The effect's timeout is 30 s. At the timeout, the core marks the checkpoint as
  timed out. Consider two cases.
  - *Key named by attempt.* The next tick asks for checkpoint 413. The first clone then finishes:
    two clones of the same moment exist, and one of them no row names. Kubernetes `generateName`
    makes duplicate pods on retries the same way.
  - *Key named by result.* The retry is the same effect: "clone to `checkpoints/412`". The clone
    is made under a temporary name and renamed into place, so if `checkpoints/412` exists, it is
    whole. A second try that finds it reports done.

  And the app, for those 10 seconds, should say "checkpoint 412: not known yet", not "failed".
- **Options:**
  1. *The answer as written.* Sound, but it leaves open what a key names and what a timeout reads
     as. Its notes accept "an occasional wrong first report".
  2. *Answer with the three edits (proposed).*
  3. *Wait for the answer.* Not recommended: one slow clone would freeze the engine, which
     `core-never-waits` forbids in spirit.
- **Evidence:**
  - Definite and indefinite errors. Jepsen's main open criticism of TigerBeetle is that its client
    turns definite failures into indefinite ones by retrying forever. Jepsen recommends "a
    first-class representation of definite and indefinite errors". A timed-out effect is the
    indefinite case. The plan's own `absent-failed-empty` (a value, nothing, or a named failure)
    and "unknown never counts as matching" both say to show it as not known, not as failed.
    https://jepsen.io/analyses/tigerbeetle-0.16.11 (verified, through a summary).
  - `clonefile(2)`: the calls are "expected to be atomic, i.e. … all new objects being created
    successfully or no new objects will be created", and the call fails with `EEXIST` if the
    destination exists. Apple engineers advise cloning to a temporary name and then `rename()`.
    https://keith.github.io/xcode-man-pages/clonefile.2.html and
    https://developer.apple.com/forums/thread/831575 (verified, through a search summary; not
    tested on a Mac). So a clone keyed by its final path can be retried safely.
  - Fakes drift from the real thing. Dropbox's Trinity runs its sync engine against an in-memory
    filesystem, and adds a slower "native" mode because the mock misses "OS-specific logic to
    manage permissions, modify extended file attributes" and cannot reboot the machine to check
    fsync. A separate suite, Heirloom, catches drift between mocked and real servers.
    https://dropbox.tech/infrastructure/-testing-our-new-sync-engine (verified). TigerBeetle's
    simulator missed a bitflip bug because it "corrupted entire sectors, rather than single bits",
    and missed the query bug because its workload never took the code path
    (https://tigerbeetle.com/blog/2025-06-06-fuzzer-blind-spots-meet-jepsen/, verified). In both
    cases the simulator's core model was right and its picture of the outside was not.
    `no-effect-twice` is about what the drivers do, and a simulator only sees fakes.
  - Timeouts on a laptop. On macOS, `mach_absolute_time` stops while the Mac sleeps and
    `mach_continuous_time` does not (from memory; Apple's page did not render). The virtual
    machine does not run while the Mac sleeps. If effect timeouts use the clock that keeps
    counting, closing the lid for an hour times out every pending effect at wake. The notes of
    `deterministic-core` already say that "which clock a hold's limit uses is still to name". For
    effect timeouts, it should be the one that stops.
- **Risks and what would change this:** If the real drivers turn out to make each effect
  idempotent at the helper, for example by keeping a table of effect ids, the key rule matters
  less. The contract test is still the only check that the helper does so.
- **Proposed text:**
  - `answer`: "An effect asks a driver, and is not a call. It has an id and no return value. Its
    answer, or a timeout, comes back later as an input. Every effect is safe to issue twice, or is
    marked once only and guarded by a key that names what it makes, so that asking again after a
    timeout is the same effect. A result or a timeout completes its pending effect once. A
    timeout leaves what the effect did unknown, never failed, until the engine has looked. A result
    that matches no pending effect, or comes after its timeout, completes nothing, and what it
    reports is checked by observation. Timeouts count only the time the device is awake."
  - `sets`: `{"in":"engine/properties","id":"no-effect-twice","says":"No effect happens twice: one issued again changes nothing.","kind":"safety","check":["simulator","test"]}`
    and `effect-completed-once` as written. The `test` is a driver contract test: the same
    cases run against each fake and against its real driver (for the first build, the Swift
    driver of Apple's virtualization framework and the APFS clone). `check` is not part of
    polarzero's yes, so this can also be added later in the list. It is written here so that it is
    not forgotten.
- **For the first build:** It blocks `engine-matches-wanted`. The first build has four effects
  that matter: start, stop or pause, clone for a checkpoint, and swap the disk for a restore. Each
  can be keyed by its result (the computer and its restore count, or the checkpoint id). That is
  enough to build on.

## Cross-cutting findings

1. **The danger is where two versions meet, not inside the core.** The five entries describe a
   core that a simulator checks well. The evidence of what such systems ship anyway points
   elsewhere. In TigerBeetle, four of Jepsen's findings were upgrade panics, one was client
   eviction, and two were blind spots in the workload or the faults. In Dropbox's Trinity, the gaps
   were the native filesystem and fsync. In Temporal, it is replaying old histories with new code.
   Silo's equivalents have no owner in the plan yet:
   - an engine upgrade with a log and a recorder that span it (see `deterministic-core`);
   - the real drivers against their fakes (see `how-effects-work`);
   - the APFS and F_FULLFSYNC behaviour on a real Mac (Mac tests 1 and 4 of the first-build
     proposal).

   A decision worth adding: "Every driver has a contract test run against its fake and against
   itself, and every release is tested on an upgrade from the one before, with computers running."
2. **The clock and the log pull against each other.** Making the clock an input
   (`deterministic-core`) and logging every input before it applies (`engine-saved-state`)
   together mean writing ticks to disk. Combined with `events-until-the-recorder`'s rule that
   applying the saved inputs again gives every event back under its number, the log becomes a
   contract between versions. For a state of a few kilobytes, saving the whole state after each
   input that changes it is a real option. Measured on Linux, it costs about 3 to 4 times an
   append. It needs no replay and has no version problem. It should be timed on a Mac before
   `engine-saved-state` is taken.
3. **A conflict with a taken decision.** `wanted-and-seen-agree` (in `engine-reconciles`) breaks
   `liveness-has-a-number`, which is taken. So does `crash-finishes-or-rolls-back` (in
   `multi-step-operations`, not reviewed here, same pattern: "in the end" with no number and a way
   out of "keeps trying and says what it is waiting for"). Both should get the "once faults stop,
   within N" form.
4. **The first build has a gap.** It has automatic checkpoints and no thinning, and with fixed
   tables that gap is silent (see `engine-limits`). Add a small fixed rule to the first build (keep
   the last N automatic checkpoints), or bring `thinning-old-checkpoints` in with its three extra
   decisions.
5. **A rule the plan implies but never states: one operation at a time per computer.** Checkpoint,
   restore, stop and start all touch the same disk image. `multi-step-operations` makes each one
   crash-safe, but nothing says that two of them do not overlap on one computer. It is the first
   real case the simulator should be pointed at (see `engine-shape`).
6. **Challenge to a decision in force: `core-simulator` (taken by agents).** It says the simulator
   checks "every property after every step". A liveness property cannot be checked after every
   step. It needs the two phases (faults, then a quiet period) described under `engine-reconciles`.
   I suggest a decision that replaces it with: "…checks every safety property after every step,
   and every liveness property after its faults stop, within its number." It is about how Silo is
   built, so agents can take it with a judge.
7. **Not every finding here was verified.** Apple's documentation pages render only with
   JavaScript, so the facts on the Virtualization delegate and on the Mach clocks come from
   memory. They should be confirmed on a Mac along with the eight tests of the first-build
   proposal. TigerBeetle not logging its clock ticks, and its simulator's two phases for liveness,
   are also from memory. Read `src/vopr.zig` before citing them in the plan.
