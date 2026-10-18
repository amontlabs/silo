# Dependencies fix-loop review

Scope: `app/SiloUI/src-tauri/src/dependencies.rs`. The deadline fix also updates its guest-image validator and the frontend dependency-store regression tests.

## DEPENDENCIES-1 · P2 · Native probe deadlines outlast the frontend watchdog

- **File:line:** `app/SiloUI/src-tauri/src/dependencies.rs:1198`, serial `collect` (original review: line 1133).
- **Trigger:** Six successful macOS signature probes taking 2.6 seconds each use 15.6 seconds, before host/version probes finish. Each probe stays below its native three-second deadline.
- **Consequence:** The frontend's 15-second watchdog replaces all rows with timeouts and discards the eventual complete report, including successful checks.
- **Suggested fix:** Share a native collection deadline shorter than the frontend watchdog, preserve completed results, and stop unfinished probes.
- **Regression:** Expired deadlines cannot start another subprocess; successive probes share the remaining budget; guest-image validation observes the same deadline; a partial native report arriving before the watchdog retains successful checks and specific recovery text.
- **Status:** Fixed and folded in `2947f602`. Native collection now has a 12-second budget; subprocesses and hashing consume that budget. This is a cooperative deadline around file reads, not a guarantee that a blocked filesystem syscall will return on time.

## DEPENDENCIES-2 · P2 · Manifest reads have no byte or file-type bound

- **File:line:** `app/SiloUI/src-tauri/src/dependencies.rs:239`, `read_json`.
- **Trigger:** Replace a runtime/Git manifest with a very large JSON file, or a FIFO without a writer. The original `fs::read` allocates for the entire file and blocks opening a FIFO before parsing or timeout checks. A directory also becomes an unreadable/permissions result rather than bundle damage.
- **Consequence:** A damaged manifest can exhaust application memory or strand a native dependency-check worker indefinitely. Retrying after the frontend watchdog abandons that worker can start additional blocked workers.
- **Suggested fix:** Open without waiting for FIFO writers, verify the opened descriptor represents a regular file, and bound reading before deserialization.
- **Regression:** A valid JSON object with over 64 KiB of leading whitespace is rejected while a small object remains accepted; directories are classified as malformed; a FIFO without any writer is rejected by a child-process test with a parent-enforced deadline.
- **Status:** Fixed by limiting manifests to 64 KiB plus one overflow byte and checking file type on the nonblocking opened descriptor.

## Verification

The deadline process regressions failed before implementation; the oversized-file and directory regressions also failed before their fix. Exact-source Rust harnesses under `/tmp/silo-codex-target/verification/dependencies/` passed 22 tests, including all dependency-module tests and three guest-image validation tests. These harnesses omit the unchanged Tauri command wrapper and link the shared compiled dependencies; they do not qualify the complete native application build.

The dependency-store Vitest file passed 11 tests. Node 24.11.1 typecheck, touched-file lint, and Cargo formatting checks passed for the first fix. Formatting also passed after the second fix. The focused Cargo invocation used explicit synthetic GitHub configuration and was stopped after remaining queued on the shared artifact-directory lock; no Cargo test result is claimed. The FIFO child filter derives its namespace from the test module and its parent verifies that exactly one child test passed; this also passed with the harness nested under the real dependency-module namespace. All process/file fixtures were disposable; no app, VM, credentials, or production data was used.


## DEPENDENCIES-3 · P2 · Guest-image FIFOs bypass preflight deadlines and block preparation

- **File:line:** `app/SiloUI/src-tauri/src/guest_image.rs:109` and `:136` in the reviewed source, blocking opens of `manifest.json` and `image.tar.gz`; `unpack` reopens the archive at line 212.
- **Trigger:** Replace either bundled guest-image input with a FIFO without a writer. A valid ordinary manifest accompanies the archive case.
- **Consequence:** Preflight never reaches the deadline checks after `File::open`; guest-image preparation also holds `IMPORT_LOCK` while blocked, preventing later preparations from progressing.
- **Fix:** Open bundled inputs nonblocking, validate the opened descriptor as a regular file, and use the same guard when decompression reopens the archive.
- **Regression:** Two parent-bounded child-process tests exercise the actual MicroSandbox check with manifest/archive FIFOs. Both failed with `Timeout` before the fix; each child must reject the input as bundle damage and report exactly one passing test after the fix. Existing valid-image and corruption fixtures still run.
- **Verification:** Rust fixtures use exact dependency/guest-validator source in the shared-target harness. The native Cargo run is queued with synthetic GitHub configuration; no app or VM launch is involved.


## DEPENDENCIES-4 · P2 · Linux integrity hashing blocks before checking its deadline

- **File:line:** `app/SiloUI/src-tauri/src/dependencies.rs:301` in the reviewed source, `sha256_file`'s blocking `File::open`.
- **Trigger:** Replace a bundled Git executable/helper with a FIFO without a writer. Unlike the runtime preflight, the Git hash loop reaches the hasher without a preceding readable-file check.
- **Consequence:** The native Linux dependency check waits indefinitely before its first deadline check. The frontend abandons the request while its blocking worker remains alive.
- **Fix:** Share the opened-descriptor regular-file guard across manifests, readable-file checks, and hashing. Its nonblocking open rejects FIFO inputs, and descriptor-based validation also removes the old readability check's metadata/open race.
- **Regression:** Parent-bounded FIFO hashing and directory classification tests failed before implementation; the hasher must also accept an ordinary file and return the SHA-256 `abc` test vector. The pure file hasher is now compiled in unit tests on macOS as well as Linux production builds.
- **Verification:** Exact-source Rust fixtures cover the file boundary locally. This is not a Linux package or host execution result.


## DEPENDENCIES-5 · P2 · Output limits are checked only after the probe has written to disk

- **File:line:** `app/SiloUI/src-tauri/src/dependencies.rs`, `run_bounded_with_timeout`: temporary-file capture and the post-exit `MAX_OUTPUT` check.
- **Trigger:** A probe writes more than 8 KiB to stdout or stderr before exiting. The existing `take(MAX_OUTPUT + 1)` limits only the parent's later read, not the child's writes.
- **Consequence:** A noisy probe can use arbitrary temporary-file space throughout its timeout window and affect other applications before its output is rejected.
- **Fix:** Set the child's `RLIMIT_FSIZE` to the existing output bound plus one overflow byte before exec, retain stricter inherited limits, and disable core dumps. The overflow byte preserves the specific malformed-output result. This reuses the existing resource-limit policy in [host_push.rs](../../../app/SiloUI/src-tauri/engine/src/host_push.rs), rather than adding a custom output transport.
- **Regression:** A shell fixture writes 64 KiB, then writes a separate completion marker. Before the fix it completed the oversized write and marker despite the eventual malformed-output result; afterward both stdout and stderr cases stop before the marker and still report malformed output.


## DEPENDENCIES-6 · P3 · Nested guest-image test guards discard the caller's pinned version

- **File:line:** `app/SiloUI/src-tauri/src/guest_image.rs:58–69`, `pin_test_version` and `TestVersionGuard::drop`.
- **Trigger:** Pin v4 in an outer test scope and temporarily pin another image version in a nested scope. Dropping the nested guard unconditionally resets the thread-local slot to `None`.
- **Consequence:** The still-active outer fixture silently reads the default v3 cohort. The disposable desktop creation helper also installs a temporary pin, so nested fixture use can change what later lifecycle assertions exercise.
- **Fix:** Store the previous thread-local value in each guard and restore it on drop. This changes test support only.
- **Regression:** The nested v4/v3 fixture test failed with v3 after the inner guard dropped; it must preserve v4 and restore the original value after the outer guard drops.


## Second fix-loop verification

Each new regression failed before implementation. The final exact-source Rust harness passed 30 tests (all dependency-module tests plus four guest-image validation tests); the dependency-store Vitest file passed 13 tests. Node 24.11.1 typecheck and lint passed, and Cargo formatting checks passed after each source change. Tests used disposable files and owned subprocesses; no packaged app, VM, or production state was exercised.

The ordinary focused Cargo command eventually reached compilation but failed in `settings.rs:1604–1608`: three references to `tauri::test` were gated out, followed by a closure type-inference error. These failures are outside the changed probe/validator code. Diagnostics remain in `/tmp/silo-dependencies-native-loop2.log`. A retry with the supported `tauri/test` feature uses the same shared target and synthetic GitHub configuration; its outcome is reported separately. Source-harness success does not qualify a complete native build or Linux execution.
