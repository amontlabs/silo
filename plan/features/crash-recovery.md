## Proposed

- How operations are built
  - Rule: Every multi-step operation (fork, merge, restore, recipe run) advances through explicit steps, each safe to stop after.
  - Rule: Each operation stores its current step as a record that can be read without a running process.
  - Rule: At every start each driver reports what really exists and the engine reconciles, so anything missed while it was down is picked up.
  - Rule: What must survive a crash is the record of what was done and the guarantee that no effect is repeated; recovery need not return to an exact past state.
- What must hold
  - Property: After any restart the engine works out what to do next from stored state alone; nothing about pending work lives only in memory.
  - Property: Recovery resumes from the last good step and keeps the partial work done so far; it never restarts the whole operation.
  - Property: After the engine, a computer's VM process or the device's power fails mid-task, every computer and fork is in a known state that can be resumed or dropped cleanly.
- Tests and measures
  - Test: The simulator stops and restarts the engine after every step of every long operation; the plan recomputed from saved state equals the plan before the stop, and the outcome equals an uninterrupted run.
  - Test: The simulator fails any run in which a once-only effect is issued twice.
  - Test: For every driver, a test crashes the engine exactly between sending an effect and recording its result; after the restart the effect is not carried out again.
  - Test: A test resumes an operation from any saved point with no extra arguments.
  - Test: A live check on a real Mac runs the engine's store for hours with several writers and crashes, and records any I/O error.
  - Recovery time, how long the engine takes to be useful again after a crash, is measured and kept over time.

## From the old code

The old code had it partly built, about 3,300 lines, and it needs reworking.

Lessons:
- A saved configuration journal must block snapshots only while an attempt is live, or one failed attempt freezes the computer list until relaunch.
- Detached runtime children own MicroSandbox's lifecycle guards, not Silo's command lock.
- The lock descriptor was inherited by every process it started, including the VM, so it outlived the engine.
- Test parsers against real launchctl output, not the format a bug report described.
- Holding a global mutex across remote calls deadlocked the app.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/src-tauri/engine/src/runtime/lifecycle_recovery.rs`
- `app/SiloUI/src-tauri/engine/src/runtime/configuration_recovery.rs`
- `docs/direction/evidence/R7.txt`
- `docs/direction/evidence/R10.txt`
- `docs/direction/evidence/F.txt`
- `docs/SiloUI-REVIEW-REMEDIATION-PLAN.md`

Calls: `engine.shutdown`.
