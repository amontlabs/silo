## Proposed

- The engine is its own program, not a library inside the app, so computers keep running when the app closes.
- The core decides and touches nothing: it takes state and a call and returns the new state, events and things to do. Drivers do them (run a VM, set up the network, talk to GitHub).
- Layout: `engine/` (model, core, drivers, server, sim), `cli/`, `app/`, `guest/`, `tools/`.
- Zig pinned to one version (0.17.1 was the latest stable on 2026-11-02), with its docs at hand.

- Running on its own
  - Rule: A broken invariant stops the engine through an assertion, and the engine restarts from saved state; it is never returned as an ordinary error.
  - Test: With computers running, stop the engine and install a new version; every computer keeps running throughout, and only changes wait for the engine.
  - Property: A crash of a computer or of its helper stops neither the engine, nor other computers, nor a program driving Silo from outside.
  - Rule: Each computer has one owner at a time, decided by a version check or a lease, so a stale engine or helper never overwrites newer state.
  - Test: Run two engines, or two operations, on one store; the second is refused or safe, and nothing races on the file.
  - Property: A fork and the agent session in it run for several days across engine restarts without losing work.
  - Test: Quit the app during a long run and open it again; the state and the history it shows are complete.
  - Property: The engine listens at a fixed address the user controls, and a second client or tool on the same device never moves it.
  - Clients reach the engine over a local Unix socket, and the same calls and events also work over a network transport.
  - Rule: The engine uses no message broker such as NATS or Kafka, since it is one program per device; devices talking to each other may reopen this.
  - Property: The engine's idle memory is measured and stated, with a target under 100 MB; a comparable always-on agent gateway uses 247 MB resident.
- State and events
  - Rule: Nothing changes the engine's state except a numbered input handled by the core, and calls, driver results and the clock all enter through one queue.
  - Rule: The core runs on one thread and handles inputs one at a time; only costly verification may run in batches, checked against the input order.
  - Property: The same inputs in the same order give the same state and outputs, since time and randomness enter the core only as inputs.
  - Property: An input that starts or changes something is recorded before any effect that depends on it is issued.
  - Property: Every event names the one input that caused it, and work the engine or a driver starts on its own gets a recorded cause too.
  - Rule: Every change that touches several records is atomic.
  - Rule: Only the engine reads or writes its store, and neither a client nor a driver holds state that the engine does not hold.
  - Property: Any cache the app or the command line keeps can be thrown away and rebuilt from the engine's state and events.
  - Property: The whole engine state can be written out and read back in one operation, and nothing lives only in a side structure.
  - Rule: A table stores a value only when it cannot be computed from other stored values, so counts, statuses and summaries are computed.
  - Rule: Every record and effect has an id independent of its position or order, and ids of repeated instances come from an explicit scope prefix.
  - Rule: Derived views such as history and statistics may lag behind the event log, and the transactional path never waits for them.
  - Rule: No optimisation of the core removes or reorders its numbered events.
  - Rule: Command output, logs and large files go to a log store and stay out of the state records.
  - An event records the alternatives the core weighed and did not take, for example why a merge needed approval.
  - Rule: A format change to the stored state or the calls ships with its migration, under one versioning rule for the tables that needs no file per change.
  - Putting the engine's own records back to an earlier event number truncates the event log after that number, and covers only what the engine holds.
  - Rule: A second storage backend is not announced until the whole test suite has run against it.
- Effects and drivers
  - Rule: An effect is a request with an id and no return value, and its answer, or a timeout, reaches the core later as an input.
  - Rule: Each record keeps what was asked and what was last seen, and the core issues effects until they match, never trusting an old answer.
  - Rule: Every effect record states its class, safe to repeat or once only, and the core sets the class, never the driver.
  - Rule: Every effect carries an idempotency key that its driver passes on or checks before acting, so an effect issued again after a crash acts once.
  - Rule: Every multi-step operation is a list of bounded steps known before it starts, and anything open-ended is a step that returns a new plan.
  - Rule: Failure handling, retries and observation live in the drivers and the client, and the core stays plain.
  - Rule: A retry is an explicit, limited effect, and no driver hides retries inside its transport.
  - Rule: When a failure forces a choice, the engine takes longer to reach the wanted state and never loses state or work.
  - Rule: The core receives its drivers and I/O as parameters, never names a concrete driver, and has no input or output path around them.
  - Rule: A new VM runtime or device type is a driver, a fault model, a fake and a configuration object, added without changing the core.
  - Test: Fit a second runtime or host to the driver interface, and check that the core needs no special case for it.
  - Rule: One pattern for driver interfaces in Zig, a hand-built vtable or a type checked at compile time, is chosen before the second driver is written.
  - Rule: Helpers, such as a proxy or the process that hosts a VM, run beside the engine as separate processes, each with a stated interface.
  - Rule: A Rust library a driver needs, such as the VM runtime, stays behind a process or C boundary, with no hand-written Rust bindings.
  - Rule: Driver interfaces stay internal and never appear in anything users or agents write.
  - Rule: Extensions are data, such as recipes and rules, or clients of the calls and events, and never a plugin loaded into the engine process.
  - Rule: The engine's core contains nothing about agents, and agent features are built from its primitives.
  - Rule: Each kind of work is one generic record type with its variant as a field, such as a task with an agent field.
  - Rule: Before a driver is built, rough numbers for its network, storage, memory and compute are sketched, for both bandwidth and latency.
  - The docs list what the core may ask the outside world to do, which driver does each, and which drivers each core operation needs.
  - Docs: Each driver's fault model lists every behaviour that a pinning test established.
- Limits and memory
  - Rule: The core allocates no memory after start, and its large tables are static or allocated once at start, never on the stack.
  - Rule: Every table has a fixed capacity sized from a declared limit, such as the number of computers, a name's length or a queue's length.
  - Property: The limit on computers sits above what the device's memory and disk can run, so `resource-floors` refuses a new computer before the limit does.
  - Rule: One deliberate design pass on the core's memory rules happens before its first code is written.
  - Rule: Limits, table sizes, the set of drivers and the choice of simulated or real drivers are named compile-time parameters of one configuration type, never literals.
  - Rule: Each limit is set just above the worst case measured on real workloads, by a script kept in the repository.
  - Rule: Reaching any limit fails with a named error, reported as a failure to inspect, and nothing stops or crashes silently at a limit.
  - Test: Tests count allocations in the core after start and fail on any.
  - Rule: Each core transition uses one fixed-size arena, copies its results out and resets the arena at the end.
  - Rule: Records refer to each other by id or index in a few flat tables of plain records, never by a pointer into a growable container.
  - Rule: Memory layout is not tuned for speed, since the engine tracks dozens of computers.
  - The docs record how each engine table is read, by id, by scan or by parent, and the table's layout follows from that.
  - Rule: Ids, counters and lengths are exact-width integers sized from the limits, with units in their names, and every conversion between widths is explicit and asserted.
  - Rule: The core holds no floating-point value in its state or its decisions.
  - Rule: Every loop in the core has an explicit bound and fails an assertion beyond it.
  - Rule: Hash maps keyed by data from inside a computer use a flood-resistant hasher, and only maps keyed by internal ids may use a fast one.
  - Rule: The engine and the client hold no hidden caches and no global singletons.
- Code rules
  - Rule: What counts as core, held to TigerStyle in full, is written down; drivers are the stated assumed environment and follow it where it fits.
  - Rule: Each style rule has a mechanical check from the first commit, by lint, compile-time check, test or assertion, so code that breaks one cannot merge.
  - Rule: Where possible, a dangerous operation is a named construct that a lint or the compiler can forbid, so enforcing the rule needs no judgement.
  - Risk: Coding agents resist TigerStyle, and ported code can pass the tests without following it, so reviews of engine code judge its design and are budgeted for.
  - Rule: Assertions stay on in release builds, unchecked fast paths included.
  - Rule: Every input to the core passes one validation function at the boundary, which caps every field and checks the engine's rules at run time, cheapest first.
  - Rule: Code past the boundary repeats no checks, and the checked variants of its functions are kept for tests.
  - Rule: Each core function has at least two assertions, and each property is asserted in two places.
  - Rule: Core code has plain control flow and no recursion.
  - Rule: Core functions stay under 70 lines and lines under 100 columns, and a lint checks both, along with nesting depth.
  - Rule: Names have no abbreviations, put qualifiers last and carry their units, as in `latency_ms_max`.
  - Rule: The core does periodic work at fixed intervals, in batches, and does not react to each stimulus as it arrives.
  - Rule: The engine uses Zig's standard library unless a benchmark shows Silo needs its own version, and a lint or review flags hand-rolled replacements.
  - Rule: The core does not use Zig's new I/O or concurrency interfaces until they are in a stable release, and I/O stays in drivers.
  - Rule: The repository's rules list the concurrency primitives the engine may use.
  - Rule: Logic needed both at compile time and at run time is one function, never two code paths.
  - Rule: Compile-time code (comptime) stays small, and a person reviews every change to it.
  - Rule: The engine's parts (tables, event log, drivers, Zig client) are built as primitives usable alone, each with its own tests.
  - Rule: Each engine component has a bound on its number of lines, and every addition states its reason.
  - Rule: Before an engine path is tuned, whether it is judged on latency or on throughput is written down.
  - Rule: No part tuned for batches goes into an interactive path, since a path is as slow as its worst-suited part.
  - Test: Every abstraction meant to cost nothing at run time has a benchmark or an assertion that shows it.
  - Rule: A failure the event log and simulator cannot explain first gets the event or check that would have explained it, then its fix.
  - The docs explain the engine as an event that updates the state, after which the plan is computed again from the state.
  - Docs: Wherever the docs say 'state machine', they name which part is the state and which part is the transitions.

## Open

- When the engine crashes or is updated, do running computers keep running while changes wait, or may they pause and resume as `updates-keep-running` plans?
  - Suggested: Keep running, so that an assertion stopping the engine costs no running work, at the price of one helper process per computer.
- Does the engine save its state plus the inputs since the last save, or keep every event forever and rewrite the whole log at each upgrade?
  - Suggested: State plus recent inputs, since disk stays bounded and new code never reads old inputs, with history and the audit log kept as their own records.
- To find out: Can a restarted engine find and take back one helper process per computer, under Apple's virtualization framework and MicroSandbox's process model?
- To find out: What does recording every event cost on the busiest path, and can full recording stay on at all times?
- To find out: Where else does recording everything as an event not fit, and what is used there?
- To find out: If the store uses SQLite, which journal mode avoids the SQLITE_IOERR_VNODE errors reported in WAL mode on macOS?
- To find out: Does any planned feature need two devices holding the same state to compare their stores byte for byte, which needs a deterministic on-disk layout?
- To find out: Does any planned operation need a third class of effect, one that can be undone, beside safe to repeat and once only?
- To find out: Can approvals and telemetry run as observers of the event stream, outside each operation's code, without writing state or weakening the trusted path for approvals?

## From the old code

The old code had it partly built, about 10,400 lines, and it needs reworking.

Lessons:
- The app did not start its own Dev engine (launchd needs LWCR update with ad-hoc signing); first launch fell on a stale error screen.
- Quit stops Silo-owned local computers but never computers on other devices.
- launchd refuses ad-hoc-signed Dev engine builds ('needs LWCR update'), so live checks start silod directly.
- SSH starts the bridge through a ~/.local/bin link where macOS finds no bundle; the process must re-exec from its real path.
- The runtime socket path must stay under 104 bytes, which caps the HOME length at 28 bytes.
- Linux computers are detached runtime processes, so an engine exit need not kill them; systemd KillMode=process is required to keep that true.
- A session end that leaves a computer running must exit non-zero so the supervisor records it.
- Ctrl-C in a terminal reached msb children too; start them in their own process group.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/engine.rs`
- `app/SiloUI/api/src/transport.rs`
- `docs/SiloUI-ENGINE.md`
- `docs/direction/evidence/F.txt`
- `docs/direction/evidence/V.txt`
- `docs/SiloUI-ENGINE.md#L40`
- `app/SiloUI/src-tauri/silod/src/agent.rs`
- `app/SiloUI/src-tauri/silod/src/ownership.rs`
- `app/SiloUI/src-tauri/silod/src/app.rs`
- `docs/product/JOURNEYS.md#L19`
- `app/SiloUI/src-tauri/src/engine_client/mod.rs`
- `app/SiloUI/src/desktop/engine-connection.ts`

Calls: `engine.handshake`, `engine.state`, `engine.disconnect`, `engine.shutdown`, `silod --agent`, `silod --service`.
