## From the old code

The old code had it built, about 6,000 lines, and its approach can be reused.

Lessons:
- Operations and events cover every computer and are not cut per caller, so agents cannot use them.
- Losing the client connection while waiting must never turn into a failed job.
- The UI decides from the operation kind the engine reports, never from label text.
- The gate does not nest, so long operations (rebuild, promote) must hold it once and call un-gated helpers.
- It does not replace the OS file lock between runtime processes.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/operations.rs`
- `app/SiloUI/src-tauri/silod/src/methods/operations.rs`
- `app/SiloUI/src-tauri/engine/src/jobs`
- `app/SiloUI/src-tauri/engine/src/events`
- `app/SiloUI/api/src/jobs.rs`
- `app/SiloUI/src-tauri/engine/src/jobs/registry.rs`
- `app/SiloUI/src-tauri/engine/src/jobs/mod.rs`
- `app/SiloUI/cli/src/engine.rs`
- `app/SiloUI/src/features/application/model/operation-queue.ts`
- `app/SiloUI/src/features/application/components/operation-queue-panel.tsx`
- `app/SiloUI/src/lib/operation-toast.ts`
- `app/SiloUI/src-tauri/engine/src/runtime/operation_gate.rs`

Calls: `operations.list`, `operations.cancel`, `operations.job.get`, `operations.job.acknowledge`, `events.subscribe`, `events.unsubscribe`, `silo job`.
