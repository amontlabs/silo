# Computer action failures

A failed start can leave MicroSandbox in `Stopped`. That state describes the computer,
not the result of the user's request. Silo keeps these two facts separate.

The lifecycle journal retains a filtered runtime explanation alongside the
failure summary. New events include the computer ID so a replacement computer with the
same name does not inherit the previous computer's error. The latest failed lifecycle
event is projected into `lifecycleFailure` in application snapshots. A successful
subsequent lifecycle action clears that projection. Legacy events without a
computer ID remain visible in Activity. Journal retention is capped at 200 events
and 1 MiB; a diagnostic is limited to 8,192 characters.

The desktop bridge retains local action rejections per computer until a successful
lifecycle retry, including rejections before a journal event could be saved.
Refreshes do not discard these messages or disable unrelated computers. Overview shows
the error immediately below the affected computer while preserving its observed
runtime state and retry controls. Activity preserves diagnostic line breaks.

MicroSandbox writes early boot failures atomically to `logs/boot-error.json`.
This is a single JSON document, including when pretty-printed, with `t`, `stage`,
`errno`, and `message` fields. Silo reads it as one Runtime record with its original
timestamp, using the same filtering, search, pagination, context, export, and
sensitive-output filtering as ordinary logs. A replaced boot record invalidates
an older log snapshot rather than mixing two attempts. No guest boot is needed
to query these files.

## Sources

- [MicroSandbox boot-error format](https://github.com/superradcompany/microsandbox/blob/5eca4de8bf233e57f114140f8c076ea8c96f21ab/crates/runtime/lib/boot_error.rs), verified in the pinned local runtime source.
- [Activity journal](../app/SiloUI/src-tauri/engine/src/runtime_activity.rs).
- [Retained log reader](../app/SiloUI/src-tauri/engine/src/runtime_logs.rs).
- [Desktop bridge](../app/SiloUI/src/desktop/production-source.ts).

## Verification

Deterministic Rust tests cover diagnostic preservation and filtering, durable
failure projection, boot timestamps, search, context, and pagination. Frontend
tests cover immediate Overview feedback, refresh persistence, unaffected computers,
and successful retries. These tests use temporary files and fixtures; they do
not prove live computer startup or the signing policy of a packaged app.

On 2026-09-22, the focused frontend run passed 187 tests across the bridge,
Overview, application integration, and Logs. Typecheck and lint passed. The
runtime suite passed 153 tests inside the tool sandbox; its Unix-socket path
test was blocked by that sandbox and passed when rerun outside it. Seven
opt-in tests remained ignored. Native unit tests used explicit synthetic
GitHub configuration. No application bundle or live computer was changed.
