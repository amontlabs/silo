# Logging and retention audit

Historical findings before the retained-history implementation. See
[Retained logs](SiloUI-LOGS.md) for the implemented policy and controls.

Source inspection, 2026-09-18. This records implemented behavior and proposed
improvements, not an approved retention policy. No live logs or installed app
were inspected, and no runtime behavior was changed.

## Implemented behavior

- [Runtime log adapter](../app/SiloUI/src-tauri/engine/src/runtime_activity.rs): requests
  `logs NAME --tail 200 --source all --json` for each VM, including stopped VMs.
  Reads share a three-second budget. Each returned record retains text and time;
  source and session identity are discarded. Each text line is capped at 4,096
  characters. Binary records become a placeholder. Marker-based redaction runs
  here, after runtime persistence. Read failures become ordinary-looking log
  records with the current time.
- [Logs UI](../app/SiloUI/src/features/application/pages/computers-page.tsx):
  selected sandboxes, newest first, local time, message, sandbox badge, text
  search and per-record/bulk copy. Search covers the loaded slice only. Copy
  exports message text without structured timestamps or sandbox identity.
  There is no older-history pagination, retention notice, source/time filter,
  follow/pause control, or file export in this component.
- The same file renders Activity separately: category filters, status, detail,
  full date/time, sandbox and progress. Setup output uses the shared
  [collapsible output component](../app/SiloUI/src/components/log-disclosure.tsx).
- Lifecycle history persists the latest 200 start/stop/restart events in
  `sandbox-activity.json`. [Setup history](../app/SiloUI/src-tauri/engine/src/runtime/mod.rs)
  persists at most 512 records for the current/latest setup attempt in
  `setup-activity.json`; starting a new journal begins with an empty event list.
  [Secret activity](../app/SiloUI/src-tauri/engine/src/secrets.rs) retains 100 events.
  The local application snapshot merges these sources and returns at most 200.
  These are count limits, not expiration periods.

## Pinned runtime storage

[Runtime inputs](../app/SiloUI/runtime-inputs.json) pin MicroSandbox commit
`5eca4de8bf233e57f114140f8c076ea8c96f21ab`. Its extracted source was inspected in
the ignored verification tree, together with Silo's checked-in patch. Relevant
upstream files are `crates/runtime/lib/{logging,exec_log,vm}.rs`,
`sdk/rust/lib/logs/mod.rs`, and `crates/cli/lib/commands/logs.rs`.

- Per-sandbox logs live in the sandbox's `logs` directory. `exec.log` captures
  execution output and lifecycle markers; `runtime.log` captures runtime stderr.
  Both use a 10 MiB rotation threshold. No age expiration appears in these writers.
- The rotation implementation shifts `.3` to `.4` without deleting `.4`, then
  shifts the remaining files and creates a new current file. It therefore keeps
  four backups plus the current file, roughly 50 MiB per stream for ordinary
  bounded writes, despite comments promising three backups and 40 MiB. Large
  individual writes also prevent treating the threshold as a strict byte cap.
- `kernel.log` receives console output directly. This path does not use the
  rotating writer; a total per-sandbox storage ceiling is not established.
- Runtime capture writes raw output. UI redaction is neither disk redaction nor
  a guarantee that arbitrary secrets are removed.
- The existing CLI already supports time windows, source filters, regex search,
  follow mode, JSON output and session IDs. Silo exposes only a small subset.

## Recommended next change

Make retention enforceable before adding presentation polish: correct upstream
rotation, bound every stream, define age and total-byte limits, and test expiry
and rotation deterministically. Choose numerical defaults using measured daily
log volume and the required incident investigation window; neither was measured
in this audit.

Then expose retained history through pagination and backend search. Display
coverage, truncation and read failures explicitly. Preserve computer, sandbox,
timestamp, source and session identity through display and export. Connect an
Activity failure to the relevant log window. Keep normal Activity readable and
put raw diagnostics behind details. Retain the existing CLI as the immediate
workaround for deeper investigation.

Acceptance scenario: after a failed start, restart Silo, find that operation,
open its surrounding diagnostics and export context without losing identity;
when evidence has expired or could not be read, state that explicitly.
