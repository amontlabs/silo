## From the old code

The old code had it built, about 4,700 lines, and it needs reworking.

Lessons:
- Follow must continue the previous snapshot, not start a new search every 3 seconds.
- One malformed or 1 MiB record used to break every query for a computer.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/logs.rs`
- `app/SiloUI/src-tauri/engine/src/runtime_logs.rs`
- `app/SiloUI/src-tauri/engine/src/log_export.rs`
- `docs/SiloUI-LOGS.md`
- `app/SiloUI/src/features/application/pages/logs-page.tsx`
- `app/SiloUI/src/features/application/components/logs-table.tsx`
- `app/SiloUI/src/features/application/model/use-log-history.ts`
- `app/SiloUI/src-tauri/src/commands/log_export.rs`

Calls: `logs.query`, `logs.export`, `runtime.logs`.
