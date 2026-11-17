## From the old code

The old code had it built and driven end to end, about 2,700 lines, and its approach can be reused.

Lessons:
- A one-shot exec is capped at 180 s by the engine; long work needs the streaming form.
- A process still running when the engine died is not tracked after restart; its job reads back as lost.
- Commands travel as argv, never shell source, except one carefully quoted line over SSH for macOS.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/guest.rs`
- `app/SiloUI/src-tauri/silod/src/methods/guest.rs`
- `docs/direction/evidence/T15.txt`
- `docs/product/JOURNEYS.md#L42`
- `app/SiloUI/cli/src/ops.rs`
- `app/SiloUI/cli/tests/exec_stream.rs`
- `docs/direction/evidence/T5a.txt`
- `app/SiloUI/src-tauri/engine/src/guest/exec.rs`
- `app/SiloUI/src-tauri/engine/src/guest/session.rs`
- `app/SiloUI/src-tauri/engine/src/guest/process.rs`

Calls: `guest.exec`, `guest.exec.start`, `guest.exec.get`, `guest.exec.input`, `guest.account`, `silo exec`.
