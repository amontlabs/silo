## Proposed, not confirmed by the owner

### For whoever builds it

- Property: Each run starts from a clean environment and inherits nothing from the session that launched it.
- Rule: Prompts and large payloads enter a computer through a file or standard input with an explicit size limit, and never as process arguments.
- Detail: Long-running work is detached by default.
- Detail: The user can watch any fork's run live, and watching does not block the conversation with the agent.
- Detail: Every command run in a computer through the engine can carry a deadline.

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
