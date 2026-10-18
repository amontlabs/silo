# Silo computer-use review pass 3, 2026-10-02

This pass confirms four P2 findings: a restore topology regression in the device-inventory cap, an upstream LCU publication failure that blocks upgrade retries, a manual-setup approval race, and an unsynchronized attempt-marker rename. The manual race is corrected with a focused regression. The runtime and installer findings need upstream boundary fixes; marker durability needs filesystem failure qualification.

Initial revision: `f9421925`. Findings rechecked after folding the fix and synchronizing with integration at `aa085336`; CU-01, CU-02 and CU-04 remain open there and were rechecked at `9bbb1558` after the final focused Cargo run. Scope includes commits returned by `git log --since=2026-10-01`, with particular attention to `57d8b380`, `4d58e654`, `dd2b6d80`, and `b6354934`. Findings use CU-01 onward. The two existing reports were read from the main checkout; this report does not reproduce their ledgers. R-11 and R-15 are excluded because another agent owns their fixes.

Evidence distinguishes source confirmation from executed deterministic fixtures. No app launch, live VM, production state, runtime preparation, or network-heavy verification is authorized by the shared task instructions. Native tests use the shared `/tmp/silo-codex-target` and explicit synthetic GitHub configuration only.

## Findings

| ID | Priority | Status | Finding |
| --- | --- | --- | --- |
| CU-01 | P2 | Open | Current mount counts omit unavailable captured transports |
| CU-02 | P2 | Open, upstream | Interrupted LCU publication blocks later upgrade attempts |
| CU-03 | P2 | Fixed and folded, `e5f00188` | Manual setup fails to converge when its follower disappears |
| CU-04 | P2 | Open, source-confirmed | Attempt-marker rename lacks directory synchronization |

### CU-01. The checkpoint inventory cap omits unavailable captured transports

**P2, open; extracted predicate reproduced, complete restore source-confirmed.** In [the checkpoint patch](../app/SiloUI/patches/microsandbox-checkpoint-fs-state-0.7.6.patch), `max_virtio_fs_devices` counts current launch mounts, file mounts, owned volumes and backends. It passes that bound into `PreparedCheckpointRestore::open` before building the VM. A RAM checkpoint can also contain a captured external transport that has no current host mapping. The pinned runtime deliberately constructs `UnavailableFs` for that slot, preserving topology without granting access. The new cap omits that device.

**Trigger.** Restore a full checkpoint whose captured virtio-fs count exceeds the current-input bound because one or more captured resources are unavailable. The extracted production functions reject four captured devices against a bound of three (two internal devices plus one current bind mount). This fixture models an unavailable external file slot; it does not execute Silo or restore RAM. Redundant owned-volume counting can mask smaller differences in other configurations.

**Evidence.** Exact pinned upstream revision `09df3d4b9d832adaede1fb9a198cfc660bfab8cd`: `sdk/rust/lib/sandbox/external_mounts.rs` marks a resource unavailable when no mapping is selected; [runtime transport construction](https://github.com/superradcompany/microsandbox/blob/09df3d4b9d832adaede1fb9a198cfc660bfab8cd/crates/runtime/lib/runner/vm.rs#L2044-L2098) builds file inputs from captured bindings and creates an unavailable filesystem for absent mappings. The corresponding bind-mount path retains missing slots too. The local fixture extracts both new functions directly from the shipped patch. Output: `checkpoint names 4 virtio-fs devices but this sandbox has at most 3`.

**Consequence.** The new guard rejects a supported restore topology before the runtime can construct its denied-access placeholder. This is a regression in the bundled runtime restore contract. No ordinary Silo UI sequence or live restore was exercised; the exact trigger requires unavailable captured filesystem resources.

**Correction.** Compute the upper bound from the topology that `build_vm` actually constructs, including validated captured external slots. Keep the per-object and 64 MiB aggregate limits. Validate captured binding IDs/counts before using them as a bound; do not replace the guard with the untrusted device-reference count.

**Acceptance.** Restore a checkpoint with an intentionally unmapped captured bind/file resource: its slot remains and access fails with EIO, while valid mapped resources restore. Reject extra device references not backed by that validated topology. Include current owned volumes and backends, strict/relaxed external policy, and the aggregate budget. This correction changes a runtime boundary and needs upstream tests, so it is not a small Silo-only fix.

### CU-02. Interrupted LCU publication leaves subsequent upgrades stuck

**P2, open; actual upstream publication function reproduced with synthetic collaborators.** Silo's [guest installer](../app/SiloUI/src-tauri/guest/silo-computer-use.py) invokes the pinned LCU installer for the 0.8.1-to-0.8.2 upgrade. [LCU 0.8.2 `select_release`](https://github.com/0xpolarzero/lcu/blob/v0.8.2/scripts/install.py#L85-L114) creates `prefix/.next`, then replaces `prefix/current`. Its exception handler removes the new release but leaves `.next`; later attempts refuse any existing or dangling `.next`.

**Trigger and evidence.** Inject a publication error at `os.replace` after `.next` is created. The exact upstream function preserves the old `current`, removes the new release and leaves a dangling `.next`. An unchanged retry raises `Unexpected .next path; inspect the installation before retrying`. The diagnostic runs only `select_release` from the fetched v0.8.2 source; bundle verification and runtime validation are synthetic. It uses temporary directories and no app or guest. Killing the installer in the same publication window also skips exception cleanup; that cancellation case is source-confirmed, not executed.

**Consequence.** Boot and Set up computer use keep failing after the original storage error is gone. The old release can remain usable, but a VM cannot complete the pinned upgrade or approval setup through Silo's retry. Restoring Ask does not run `lcu setup` because installation fails first.

**Correction.** Fix LCU's publication transaction upstream: clean only the temporary link owned by a failed attempt, and recover an interrupted publication under the existing installer lock after validating the candidate and current links. Preserve the prior release. Silo should consume a verified release containing that fix; do not remove arbitrary guest paths to bypass the upstream refusal.

**Acceptance.** Inject failure and terminate an owned fixture installer between link creation and publication; retry must either safely recover or publish a fresh validated release. Keep the old `current` until success and reject unrelated `.next` paths. Test successful upgrade and rollback separately. This is an upstream installer defect, so no unverified Silo cleanup workaround was added.

### CU-03. Manual setup can leave a newer approval choice without an executor

**P2; fixed and folded in `e5f00188`; native regression passed.** [Manual setup](../app/SiloUI/src-tauri/engine/src/computer_use.rs) reads one mode and runs one attempt. The background apply has a convergence loop; manual setup did not.

**Trigger.** Change approval while manual Set up computer use runs. The follow-up background thread has a ten-minute admission deadline (`GATE_WAIT`), while the manual guest run may last fifteen minutes plus host allowance (`APPLY_TIMEOUT`/`APPLY_GRACE`). If that thread expires before the manual turn finishes, the manual helper completes the earlier mode and leaves the new choice pending until another boot or app start. A narrower scheduling window also exists between reading the mode and saving the unfinished marker: returning to a previously completed mode there schedules no worker.

**Evidence.** The unchanged production `setup_with` was extracted, compiled with Rust 1.94.0 and real `serde_json`, and run with a synthetic runner that saves Ask during an Auto attempt and supplies no surviving follower. It failed the convergence assertion: actual `[Auto]`, expected `[Auto, Ask]`. This fixture establishes manual-turn behavior; the ten-minute admission expiration and pre-marker scheduling window are source-confirmed rather than elapsed-time reproductions. A committed native behavior regression uses the actual policy files, operation turn and barrier-controlled guest runner.

**Consequence.** The chosen Ask can remain unapplied while agents retain Auto. The UI correctly says pending, but no worker remains to finish. This is distinct from historical R-21: that background-turn defect is already fixed; this finding concerns the manual turn.

**Correction.** Re-read the chosen mode after each manual attempt and converge inside the existing turn. Stop on cancellation or an unavailable app. Subsequent convergence attempts use ordinary idempotent setup. The extracted production function now passes convergence and cancellation assertions. No new scheduler or guest approval record was introduced.

**Acceptance.** Save a new choice during a blocked manual helper, then release it with no surviving queued follower. The turn must finish with desired and applied Ask, no unfinished marker or pending slot, and one helper at a time. Keep Stop, Restart, Delete and shutdown cancellation tests passing. The fixture does not qualify live LCU behavior or a fifteen-minute real guest installation.

### CU-04. Policy and unfinished-marker publication omit directory synchronization

**P2, open; source-confirmed durability gap, no power-loss reproduction.** [Policy persistence](../app/SiloUI/src-tauri/engine/src/computer_use.rs) in `write_atomic` synchronizes the temporary file, renames it through `NamedTempFile::persist`, and returns success without synchronizing the containing directory. `begin_attempt` then allows the guest helper to mutate agent configuration. The plan explicitly promises recovery after power loss, not only an ordinary process crash.

**Trigger and evidence.** Auto was recorded durably. The user selects Ask; `set_approval` returns after replacing that policy file, and an apply publishes its unfinished marker through the same writer. Power loss occurs before those directory updates reach durable storage. The prior Auto policy can remain authoritative after reboot. Linux's [fsync contract](https://man7.org/linux/man-pages/man2/fsync.2.html) requires separate directory synchronization to guarantee the directory entry; syncing file contents alone does not establish it. The checked Silo function has no directory sync at all. Pinned [tempfile 3.27.0](https://docs.rs/tempfile/3.27.0/tempfile/struct.NamedTempFile.html#method.persist) also documents that `persist` does not synchronize the directory. Checkpoint record persistence elsewhere already treats directory synchronization as a fallible step. The crash/reboot outcome is inferred from that documented contract; it was not executed against a disposable filesystem.

**Consequence.** An acknowledged Ask choice can revert to Auto after a host crash. A host power failure stops the local VMs, and the next boot always schedules setup; that boot then applies the surviving old Auto policy. The new marker also lacks its documented persistence guarantee, but loss of the marker alone does not establish a skipped boot apply. File write and rename can both return success while their names are not durable.

**Correction.** Synchronize the containing directory after publication before reporting marker success, and persist newly created policy-directory entries in their parent too. Propagate synchronization failure so the helper cannot start under an unconfirmed marker. Qualify the supported Linux and macOS filesystem guarantees rather than assuming one syscall proves every storage platform.

**Acceptance.** A disposable filesystem or fault-injection fixture must cut persistence between file sync, rename and directory sync, then reboot/reopen from durable state. An acknowledged Ask choice must survive reopening from durable state, including a choice saved while the VM is stopped. Guest mutation starts only after the marker's name and bytes are durable. Inject directory-sync failure after rename and assert no helper runs; cover first directory creation and replacing an existing completed policy. No storage crash test was executed in this pass, so this boundary remains open rather than receiving an unqualified durability claim.

## Verification and review boundaries

All evidence is under ignored `app/SiloUI/src-tauri/target/verification/pass3-computer-use/`. No packaged bundle was inspected or launched. Data was temporary or synthetic; no live VM, production HOME, Keychain, or remote computer was accessed. LCU research fetched only its small pinned installer source, not an archive or runtime.

| Check | Result and boundary |
| --- | --- |
| CU-01 inventory seam, `rustc +1.94.0` | Reproduced rejection of four modeled captured devices against three current devices |
| CU-02 exact LCU 0.8.2 publication function | Reproduced dangling `.next` after failed replace and blocked retry; old release survives |
| CU-04 persistence source and Linux fsync contract | Confirmed missing directory synchronization; no crash/reboot execution |
| CU-03 extracted production manual turn | Failed before correction with `[Auto]`; passed after correction with `[Auto, Ask]`; cancellation stops after one attempt |
| Native CU-03 behavior regression through Cargo | 1 passed; actual policy files, operation gate and barrier-controlled runner |
| Native focused computer-use group through Cargo | 66 passed, 0 failed, including the new regression and existing cancellation/identity/marker cases; final source at `9bbb1558` |
| `python3 -m unittest discover -s app/SiloUI/scripts -p test_computer_use.py` | 58 passed at the final cutoff; earlier run: 56 passed before subsequent guest-persistence tests arrived through integration. Includes the separately owned R-11 correction |
| `npm --prefix app/SiloUI test -- src/test/microsandbox-runtime.test.ts --maxWorkers=1` | 13 passed, Node 24.11.1; cached runtime/protocol staging fixtures |
| `npm --prefix app/SiloUI run typecheck` | Passed before subsequent integration merges |
| `npm --prefix app/SiloUI run lint` | Passed before subsequent integration merges, with two existing warnings in Status bar and Storage |
| `cargo +1.94.0 fmt --manifest-path app/SiloUI/src-tauri/Cargo.toml --check`; `git diff --check` | Passed after formatting the new native regression |

Native Cargo commands used `CARGO_TARGET_DIR=/tmp/silo-codex-target`, the release guide's synthetic `SILO_GITHUB_APP_SLUG=silo-ci-test`, `SILO_GITHUB_CLIENT_ID=test-client`, `SILO_GITHUB_CLIENT_SECRET=test-secret`, and test-only `TAURI_CONFIG='{"bundle":{"externalBin":[],"resources":[],"macOS":{"frameworks":[]}}}'`. This excludes absent generated packaging inputs without preparing runtime resources or changing tracked configuration. Two initial attempts used an empty resources map, which merges with existing entries rather than clearing them; both failed on absent generated resources before tests. Their logs remain. A direct shared-executable group run was subsequently found to be another worktree's binary (the new regression was absent) and is excluded from branch validation. Use Cargo to keep the artifact lock through execution.

The LCU diagnostic initially compared a canonical temporary path with its `/var` spelling on macOS; correcting that fixture comparison produced the reported reproduction. The failed assertion and initial native packaging errors are fixture/build setup evidence, not product findings.

Review covered the helper's pin matching, download/hash checks, install/setup/readiness phases, host attempt marker and failure persistence, approval convergence, operation-gate preemption, guest transport cleanup, runtime instance restoration and probes, checkpoint integrity/restore limits, and changed opt-in live-test inputs. Counter-evidence matters: the pinned agent's relay disconnect cleanup explicitly signals the guest process group, so killing `msb exec` was not promoted to an orphan-process finding. Identity checks revalidate both machine label and runtime instance after admission; existing native cases cover replaced instances and missing identity. The cap's per-type and aggregate limits remain useful; CU-01 concerns its topology bound.

The upgrade unit fixture substitutes installer behavior and therefore does not qualify a real 0.8.1-to-0.8.2 installation or rollback. Concurrent integration added bounded host policy reads, forward-compatible attempt outcomes and directory synchronization for guest receipts; the final Cargo group includes those changes. The host policy writer in CU-04 still omits directory synchronization. No live upgrade/rollback, Linux checkpoint, full device-state stress, or cancellation-to-guest-exit timing was run. R-11 and R-15 remain owned elsewhere and are not counted here.

Next action: add CU-04's directory-sync failure and crash-recovery regression, then make marker publication durable before the guest helper starts.
