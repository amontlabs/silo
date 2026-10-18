# Lifecycle review, 2026-10-02

Requested scope: local and owner-routed remote Start, Stop, Restart and deletion; graceful Quit;
durable lifecycle intents and retirement; startup crash recovery; admission of
operations on the same and different sandboxes. The initial integration baseline
was `e783170e`. Findings below have deterministic regressions against production
code seams; source-only suspicions are not findings. The resume instruction limited
completion to the in-progress LC-01 fix; the remaining review scope is unfinished.

All verification uses temporary fixture state and fake or scripted runtimes.
Native tests use Rust 1.94.0, the shared `/tmp/silo-codex-target`, synthetic GitHub
configuration (`silo-test`, `test-client`, `test-secret`), and a test-only Tauri
resource override that omits bundled runtime resources. No application, real VM,
production state, Keychain, remote computer or packaged bundle was exercised.
These tests do not establish live VM health or release readiness.

## LC-01: A lifecycle retry can undo a newer Stop

Priority: P1. Status: fixed and verified.

Location: [local lifecycle adapter](../app/SiloUI/src-tauri/engine/src/runtime/mod.rs),
[remote lifecycle adapter](../app/SiloUI/src-tauri/engine/src/runtime/remote_ops.rs), and
[operation gate](../app/SiloUI/src-tauri/engine/src/runtime/operation_gate.rs).

Trigger: Start encounters a transient launch failure and releases its VM lane for
retry backoff. A newer Stop runs and retires the saved Start. The older Start then
reacquires the lane and creates a new intent, starting a VM the user just stopped.
The existing shutdown-generation check protects Quit, but an ordinary Stop does
not change that generation.

Evidence: `lifecycle_retry_does_not_undo_a_newer_stop_on_the_same_vm` failed at the
baseline. The fixture injects one launch failure, then executes a real lifecycle
Stop between attempts without sleep-based ordering. The regression also requires
an action on a different VM to leave the original retry free to continue.

Correction: retain the latest lifecycle request ID per VM in the gate. Local and
remote retries provide their previous request ID; check it under the admission
lock before recording or queuing the retry. A rejected old retry must not itself
supersede a newer request's retries.

Acceptance: the newer Stop leaves the VM stopped, the old Start returns Cancelled,
no extra Start executes, and no Start intent remains for recovery. Another VM's
lifecycle action permits retry. `rejected_old_lifecycle_retry_does_not_supersede_the_newer_request`
checks that refusing an old retry leaves the newer request eligible to retry.

Verification: the original regression failed before the fix and passed afterward.
The focused Cargo run covering `runtime::lifecycle_recovery::`,
`runtime::operation_gate::`, `runtime::remote_ops::`, `remote::operations::`,
`auto_retry`, and `retry_sequence` passed 77 tests, with 2 ignored. Both new
regressions ran in that Cargo invocation. Rust formatting, frontend typecheck,
frontend lint, and `git diff --check` passed. No full native suite was run.
