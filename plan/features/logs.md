## Proposed

- Rule: Logs have a cap that holds with many agents writing, and reaching it is shown where the logs are read.
- Debug logging is switched on by an environment variable or a command, so it works when the app cannot open.
- A hang detector writes a record even when nothing crashed.
- A computer's log stream also carries its network requests and screenshots, all in one timestamped order.
- A command prints the log and crash-report paths.
- The docs state where the logs and crash reports are kept.
- Test: Scrolling inside a log view or an embedded terminal scrolls and selects no text.

## Open

- Does Silo build its own log search, or emit logs, events and the audit journal as OpenTelemetry for existing tools to search?
  - Suggested: `silo logs` first, as planned, and an OpenTelemetry export after it, because reading a computer's output should not need a collector running.

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
