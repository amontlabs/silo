# Native test support

Native unit tests use [shared fixtures](../app/SiloUI/src-tauri/engine/src/test_support.rs)
instead of acquiring the production operation gate to serialize unrelated tests.

## Persisted configuration compatibility

The runtime metadata fixture pins the schema-1 computer and SSH field names used before
desktop configuration was added in `7e7fb3e4`. Deserializing and serializing that
fixture must preserve its JSON fields and omit an absent desktop. Desktop fixtures
also pin `startWithComputer`'s default of true and `builtIn`'s default of false,
including omission of the false built-in flag. These tests exercise Serde's
[missing-field defaults and serialization rules](https://serde.rs/field-attrs.html)
without the runtime or process-wide state.

Update preferences retain unknown JSON fields through load/save using Serde's
[flattened map](https://serde.rs/attr-flatten.html), while the automatic-check flag
still requires a boolean. Temporary-file regressions exercise both choices,
additive metadata, save/reload, and explicit repair of malformed preferences.

The migration journal and generation marker were introduced together in
`d654e4bc`. A completed migration requires that marker at startup and whenever
normal runtime storage is resolved. Temporary-file regressions remove the marker,
verify that both generations and the journal remain untouched, and restore it to
verify recovery. An installation that needs no migration still uses `runtime/`
without a marker.

Export-folder preferences also preserve additive JSON fields through an explicit
folder change using a flattened map. Their reader retains the 1 MiB limit and
schema-version check; malformed destinations or archive arrays remain unreadable.
The temporary-file tests verify that reads leave the saved bytes untouched.

Connections settings retain unknown top-level preferences when their known
fields change. Temporary-file round trips verify the saved identity and device list,
while malformed or absent required fields still fail to load. Existing read/write
size-limit and directory-sync regressions exercise the same reader and writer.

GitHub settings preserve additive top-level fields when known choices change.
Round-trip fixtures pin legacy defaults for access, account, grants and computer
policies, and retain the existing rejection of unsafe policy revisions. The
isolated serialization harness supplies no HTTP retry floors; native tests take
the shared state guard because the production writer collects those floors.

## Process-wide state

`test_support::global_state()` guards tests in modules that reach the global
operation gate, shutdown admission, GitHub caches, SSH listeners or secret caches.
It releases shutdown admission on drop, including assertion unwind. Join all
workers before dropping the guard. A worker must not acquire the test lock itself.
Tests with independent gates and state instances run in parallel. Keep the guard
when their helpers still reach global shutdown admission or caches. The remaining
serial group consists of tests whose helpers reach that process-wide state;
`test_support::global_state()` call sites identify its current membership.
This conservative isolation permits concurrency within each owning test and
serializes the guarded tests inside the full parallel suite. Ordinary checks use `cargo test --locked`
with Cargo's default test thread count. Opt-in live checks retain their documented
serial commands and require separate authorization.

The storage quit regression now changes shutdown admission in the same process
under this guard; it no longer recursively launches its test executable.

The failed-Quit retry regression completes the admission change synchronously
under its guard. It no longer leaves a sleeping, unjoined worker that can change
the next test's shutdown generation. Remote operation registry tests own their
gates, so unrelated device operations cannot split their two-computer barrier or
cancel their queued requests.

## File descriptor isolation

The seven ordinary `host_push_cache` tests each execute once in a private child
of the current test binary, selected with `--exact`. The parent verifies the child
exit status and that exactly one test passed. These children can run concurrently.
An unrelated test worker's fork can retain a sweep lock after its owning test
closes the descriptor. A mutex around only cache tests cannot exclude forks from
other modules. Shared lock references are documented in
[Apple's flock(2) manual](https://developer.apple.com/library/archive/documentation/System/Conceptual/ManPages_iPhoneOS/man2/flock.2.html)
and the [Linux flock(2) manual](https://man7.org/linux/man-pages/man2/flock.2.html).
A pipe-coordinated fork reproduction confirmed that parent close left the lock
busy until child exit. Separate test processes isolate the file descriptor table
while preserving the cache's real locking and intentional inherited-child test.

`configuration_recovery` tests fork the whole test process on purpose, which copies
every descriptor open at that moment, including the worker-lock files of the
`backup` command tests. A forked copy kept `cancel_asks_the_runtime_to_stop_before_killing_it`
failing in nearly every full parallel run on Linux (its final zero-wait lock check
saw the lock still busy) while it passed alone. The lock-releasing `backup` tests
now take the shared isolation guard, which the forking tests also hold, so the two
groups never overlap.

## Live test temporary directories

Live tests create their temporary directories under `/tmp` because the runtime's
control socket needs a short absolute path (104 bytes on macOS). `/tmp` is a small
tmpfs on many Linux devices, so set `SILO_TEST_TMP` to a short directory on a larger
file system (for example `/var/tmp/silo-t`) before running them:

```sh
SILO_TEST_TMP=/var/tmp/silo-t SILO_LIVE_TEST_CONFIRM=disposable-test-fixtures \
  cargo test --locked <live test> -- --ignored --nocapture
```

The helper is `test_support::live::temp_root`. Live computer-use tests pin the v4
guest image themselves (`guest_image::pin_test_version`); ordinary tests default to v3.

Keep environment overrides on child `Command` instances, filesystem state in
owned temporary directories, and listener ports allocated by the OS. A test's
workers must finish before its isolation guard or temporary directory drops.

## Full-suite qualification (K-18)

On 2026-09-30, Rust 1.94.0 on `aarch64-apple-darwin`, with 16 logical CPUs,
passed ten consecutive full default-thread runs after all isolation fixes. Every
run passed 1,033 tests and ignored 13 opt-in or subprocess-helper tests. Commands
ran from `app/SiloUI/src-tauri`, prefixed with `nice -n 10`, using the release
guide's synthetic GitHub values (`silo-test`, `test-client`, `test-secret`).
`RUST_TEST_THREADS` was unset. No suites overlapped during the timing comparison.

| Check | Full runs | Wall time |
| --- | --- | --- |
| `cargo test --locked` qualification | 10 consecutive passes | Median 65.68 s; range 62.00–67.65 s |
| Final serial comparison, `cargo test --locked -- --test-threads=1` | 1 pass | 133.06 s |
| Final `cargo test --locked` | 1 pass | 62.30 s |

The parallel median was 50.6% lower than the serial wall time on this
shared host. Both final runs passed the same 1,033 tests with 13 ignored. Earlier
exploration included one cache-lock failure; its complete output was preserved
before the file descriptor isolation fix and qualification restarted.

Logs and timing JSON remain under the ignored
`app/SiloUI/src-tauri/target/verification/k18/` directory (`parallel-11.log` for
the failure, `qualified-01.log` through `qualified-10.log`, and `final-*.log`).
The [ordinary CI workflow](../.github/workflows/ci.yml) uses the same default-thread
command. The historical Linux-host verification workflow was removed when Linux
host support ended. The [release platform workflow](../.github/workflows/release-platform.yml)
uses `--test-threads=1`. Linux execution was outside this local qualification.
These fixture checks do not establish live computer or packaged-app behavior.

## Runtime fixtures

`test_support::paths(directory)` creates the standard disposable runtime layout.
The compatibility wrappers taking `TempDir` delegate to this one implementation.

[ScriptedRunner](../app/SiloUI/src-tauri/engine/src/test_support/runner.rs) takes ordered
`ExpectedCommand` values. Each expectation specifies exact argv and a typed
output or error, with an optional exact timeout. Unexpected reads and mutations
panic immediately. `assert_finished()` and normal drop reject unused expectations.
Use it for fixed command traces. Keep stateful fakes for behaviors such as changing
runtime identity, filesystem side effects or cancellation while a process runs.
`write_shell_script` centralizes executable installation for those process-boundary
tests; the caller still owns the temporary directory and explicit script behavior.

The desktop, checkpoint readiness/failure and application-read failure tests use
this runner. Other stateful and output-queue runners remain; the new helper does
not silently accept commands on their behalf.

## Concurrent assertions

Queue tests wait for a callback from an actual queued worker before checking
absence of its result. Backup cancellation is verified while the contending
operation still holds its turn. GitHub sleep/intent tests expose the point before
the atomic condition-variable wait, and the proxy test exposes header consumption
before allowing the next fragment. Channel timeouts bound failures; they do not
provide synchronization through elapsed time. Remote authorization tests assert
the specific rejection instead of any error. Bridge error-code migration belongs
to K-23.

## Live tests and temporary directories

Every live Rust test and the browser live-test launcher require
`SILO_LIVE_TEST_CONFIRM=disposable-test-fixtures` before external side effects.
Existing GitHub-specific authorization remains required as well. The editor
fixture home must resolve to an existing absolute directory different from
`HOME`, including symlink aliases. The nested GitHub guest regression launches
the current test executable directly, avoiding recursive Cargo target locks.

Retention fixtures remove their temporary directories on normal return and
unwind. Short `/tmp` roots remain where runtime Unix sockets require them, with
comments explaining the macOS 104-byte limit. The retention module is also
embedded in the MicroSandbox patch, so its test-only cleanup change updates that
patch and its pinned SHA. This changes the runtime preparation cache key, without
changing production retention policy.

The built-in desktop and computer-use live tests share `test_support::computer_use_live`
(one disposable `/tmp` home per test, `e2e-*` computers, a registered published ChatGPT
folder). Inputs: `SILO_LIVE_TEST_CONFIRM`, a signed `msb` and libkrunfw (`SILO_TEST_MSB`,
`SILO_TEST_LIBKRUNFW`), the v4 image directory (`SILO_TEST_GUEST_IMAGE`: manifest.json and
image.tar.gz) and a published ChatGPT folder (`SILO_TEST_PUBLISHED`; `SILO_LIVE_CHATGPT_ROOT`
keeps the folder `chatgpt_app::tests::live_download_of_the_pinned_arm64_package` publishes).
`SILO_LIVE_KEEP=1` leaves a failed test's stopped home for inspection;
`SILO_LIVE_EVIDENCE` names a directory for the drive test's screenshots. The tests:

| Test | Proves |
| --- | --- |
| `live_lcu_drives_the_desktop_without_a_model` | Create to ready, `lcu status`/`doctor`, read-only mount, a bare MCP client with no `_meta` reaches X11 through the `js` tool (LCU 0.8.2), LCU's own MCP client drives GNOME Text Editor (typeText, paste, Save As, then window-targeted ctrl+a/BackSpace/per-key typing and ctrl+s, LCU 0.8.3) and a terminal (per-key), with the files verified from outside; also records memory, disk and times |
| `live_approval_switch_edits_only_the_installed_harnesses` | `auto` adds and `ask` removes exactly LCU's approval entries in Codex and Claude Code (installed from npm; `SILO_LIVE_SKIP_HARNESS_INSTALL=1` skips that phase) |
| `live_built_in_lifecycle_keeps_the_desktop_and_computer_use` | Restart, stop/start, checkpoint of the running computer, fork and in-place restore each end with the session running, computer use ready, the folder read-only and `lcu doctor` passing |
| `live_pre_v4_computer_gets_no_mount_no_desktop_and_keeps_its_flows` | A computer from the v3 image (`SILO_TEST_V3_GUEST_IMAGE`) has no mount, no desktop and no helper, and its lifecycle flows work |
| `live_built_in_computer_use_sets_up_and_survives_export_and_import` | Export and import into a second home with its own folder; the imported computer takes the destination's `ask` |
| `live_built_in_desktop_boots_repeatedly` | `SILO_BOOT_LOOP_ROUNDS` (default 3) restarts and imports with no desktop failure |

These opt-in tests exercise real disposable computers with temporary data when run
with the required live inputs. Their source and fixture checks alone do not prove
those workflows passed. A successful live run qualifies only the tested runtime,
image and scenario; it does not launch the packaged Silo app or establish
installed-app or release readiness.

## Blocked test-speed experiment (2026-10-02)

The two SSH listener-startup regressions still use 2.5/1-second child delays
and 600/500 ms parent sleeps. A disposable prototype replaced those delays
with a loopback TCP readiness handshake and scoped reconcile workers, keeping
the existing assertions. Three standalone fixture-child checks verified held
readiness, release, TCP echo and owner-EOF exit. Those checks do not exercise
the Rust reconciliation regressions. The prototype was reverted; no Rust
before/after timing or speedup is claimed.

The first `cargo +1.94.0 test --locked --no-run --message-format=json` used
`CARGO_TARGET_DIR=/tmp/silo-codex-target` and synthetic GitHub values. It waited
roughly 24 minutes for the shared lock, then exited 101 because
`binaries/msb-aarch64-apple-darwin` was absent. The documented test-only override
from the [computer-use review](SiloUI-CODE-REVIEW-PASS-3-COMPUTER-USE-2026-10-02.md)
clears generated inputs without preparing the runtime:

```sh
TAURI_CONFIG='{"bundle":{"externalBin":[],"resources":[],"macOS":{"frameworks":[]}}}'
```

The retry stayed queued and was cancelled after verifying its owned Cargo
process, worktree, redirected files and absence of compiler children. Do not
interrupt other builds or time an unidentified shared test executable. Resume
native measurement after the shared build queue clears, with that unit-test
override and synthetic GitHub configuration. Ignored evidence under
`app/SiloUI/src-tauri/target/verification/test-speed/` includes
`rust-build.{jsonl,log}`, `rust-unit-build.{jsonl,log}`,
`handshake-prototype.log`, and the unverified `ssh-readiness-candidate.patch`.
