## Proposed, not confirmed by the owner

- Each concept states its properties, and a simulator checks them on every change with random sequences of calls, crashes included.

### For the owner to settle

- Which properties does Silo promise in public, such as "dropping a fork leaves nothing behind" or "a computer cannot reach your device", and which stay internal checks?
  - Recommendation: Promise in public only the properties whose check has run and passed, as isolation-tests backs each "cannot" with evidence.
- Is the engine's core first written as a short reference model with no optimisation, which you check, and the Zig core then translated from it?
  - Recommendation: Yes, because it is a description of the engine short enough for you to check, and the simulator can compare the Zig core against it.
- Once more simulation stops paying, is one property proved formally: a computer cannot get more than it has, a fork never has more than its parent, or credentials never come back in a merge?
  - Recommendation: Not before the starter is in daily use, and then only where testing is not enough, since security work for its own sake waits (`security-not-the-focus`).
- Does Silo publish its measurements: the time to start a fork, to compute a diff and for an approval round trip, and how long a computer stayed up?
  - Recommendation: Yes, once the starter is in daily use, each as a dated measurement on a named machine.

### For whoever builds it

- The property list
  - Rule: Each property is one plain sentence with its kind, safety or liveness, and what checks it: the simulator, a hostile test, a live check, assertions or a proof.
  - Rule: Each property is written so that a random sequence of inputs and crashes could break it.
  - Rule: Each liveness property gives "in the end" a number, for example at most two minutes of retries with at least thirty seconds free of faults.
  - Rule: Agents propose invariants for the property list, the simulator tries to break each one, and the owner reviews only those it breaks.
  - Risk: The simulator checks only the properties someone wrote, so a list of missing properties is kept and reviewed.
  - Docs: Simulator results are reported as the seeds and runs covered, never as "proven".
  - The docs say that the simulator checks the core against the fakes only, and that the real drivers and the real boundary have their own hostile tests and live checks.
- Properties to add
  - Property: Fork, diff, merge and checkpoints keep working while GitHub or any other forge is unreachable.
  - Property: Every pending operation ends or times out, even when a driver never answers.
  - Property: After every input, each counted quantity (disk, memory, running computers, quota) equals its previous value plus the changes named in the emitted events.
  - Property: The production event log has, byte for byte, the format the simulator reads, so any real log replays in the simulator.
- The simulator
  - Rule: The same core code runs under the real drivers and under the simulator's fakes, with no conditional code.
  - Test: A reference model, if kept, runs beside the Zig core in debug builds, under fuzzing and in the simulator, and a difference in state after any input stops the run.
  - Detail: The simulator is a discrete event simulation in which every message and event goes into one queue ordered by delivery time.
  - Detail: The simulator decides deterministically when the answer to each effect arrives, and may delay any message without bound, drop it or reorder it.
  - Rule: The simulator checks every property after every step.
  - Detail: On a failure the simulator removes inputs until the smallest failing sequence is left.
  - Detail: The input generator draws values from the constants and limits found in the core and from the full legal range.
  - Detail: One source gives two builds, selected by build options: the release build, and a checked build with full tracing for the simulator.
  - Test: The same seed is run on Apple Silicon and on x86_64 Linux, and the state and the events are identical.
  - Detail: Each simulator run records its cost (time per step, memory, disk), and the trend is looked at daily.
  - To find out: Is the simulator built on Zig's replaceable test runner or as its own program?
  - To find out: Do `std.testing.fuzz` and `zig test --fuzz` run on macOS now or still need Linux, and are they worth using on the engine core?
- Faults and fakes
  - Rule: A fault model, written with the design of the shell stage, says what each fake (hypervisor, guest, disk, network) does wrong on purpose and which inputs each property must survive.
  - Detail: The fakes make a computer die, fill the disk, make a guest lie, never deliver a reply and deliver a reply twice.
  - Detail: The fake GitHub rate-limits, refuses and goes down.
  - Rule: Every real failure becomes a behaviour of a fake.
  - Detail: Recorded exchanges with real drivers and guests, and recorded responses from GitHub and package registries, are replayed in driver tests and used as simulator inputs and seeds.
  - Detail: The simulator pauses fault injection in windows, so that the engine can recover and liveness can be checked.
- Scenarios and seeds
  - Rule: An engine change merges only when a scenario that uses every feature at once and random sequences of inputs with crashes both pass.
  - Rule: Every failing seed becomes a named, fixed regression test in the change that fixes it, with a replay bundle.
  - Test: Each feature has a simulator scenario for an engine restart, a client reconnect, stale ownership, and a desync between the app and the engine.
  - Test: The set of wanted operations changes while an effect is in flight, and the effect's result attaches to its own record or is discarded.
  - Test: A fork's working files are deleted from under it, and the fork recovers from its last checkpoint.
  - Test: The simulator covers two operations on one store, a timer that fires twice and a resume with a missing input, and the properties hold.
  - Test: Property tests in the core check ordering, and that applying an operation then undoing it gives back the starting state.
  - Test: The simulator also runs with the core's limits set very small and very large, and the properties hold.
  - Detail: The transitions and branches that random runs never reach are recorded, and an agent writes input generators aimed at them.
  - Test: Agents run mutation testing on the tests and on the simulator's properties, and each one fails for some mutation.
  - Detail: A debugging command replays a recorded input log and bisects it to the first state in which a given property fails.
  - Detail: A real history can be replayed in the simulator to measure a proposed storage or engine change.
- Beyond the simulator
  - Test: The simulator's scenarios also run against the real drivers behind adapters, and the same properties hold.
  - Test: The time to start a fork, to compute a diff and for an approval round trip is measured, and each stays within its budget.
  - Test: A live check runs a computer for several days, and the computer stays up and reachable throughout.
  - Test: One fixed end-to-end task (fork, change, diff, merge) is rerun on each new model and each release, and its result is kept as Silo's own benchmark.
  - Test: Several real, large projects, still to be chosen with their recipes, run their existing work unchanged inside a Silo computer, and passes are counted per project.
  - To find out: How much wall time does Silo add to a task, against the same agent running outside a computer?
  - Test: Where the old app had a feature, its behaviour, brought back by a dedicated lookup of the old code (`old-code-outside`), is the oracle for the new one.
  - Risk: The engine is new Zig that nobody else has run, without Rust's static and dynamic analysis tools, so assertions and the simulator must catch what those tools would have caught.
  - To find out: Which specification tool is worth trying on the pure core: Specula (code to TLA+), Quint for the state machine, or harmonic.fun?

## From the old code

The old code planned it but did not build it, about 6,500 lines, and it needs reworking.

Lessons:
- Keychain is absent under another HOME, so checks needing saved sign-ins cannot use their own instance.
- Only the real engine on the KVM runner showed several container bugs; keep a real-VM job in CI.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/scripts/live/run`
- `docs/SiloUI-ENGINE.md#L90`
- `.github/workflows/ci.yml`
- `.github/workflows/linux-host.yml`
- `app/SiloUI/scripts/ci-native-changes.py`
- `app/SiloUI/UI-PATTERNS.md#L89`
- `app/SiloUI/src/fixtures`

Calls: `scripts/live/run`.
