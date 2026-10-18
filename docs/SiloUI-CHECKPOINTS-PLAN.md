# Silo checkpoints, restore and forks

Date: 2026-09-25. Product decisions are accepted and the Silo-side checkpoint,
fork, restore, and migration implementation is complete for this qualification
pass, which used MicroSandbox 0.7.2. Later sections record their own revisions
and verification dates. Current runtime pins come from
[`runtime-inputs.json`](../app/SiloUI/runtime-inputs.json); upgrade checks and
qualification limits are recorded in [runtime packaging](SiloUI-RUNTIME-PACKAGING.md).
This document preserves the original evidence and remaining platform qualification;
it is not a release approval.

## Direction and accepted decisions

Keep Silo and adopt supported upstream MicroSandbox snapshots and forks. Retain
its credential proxy. Improve the desktop independently and use full LCU for
computer use. The E2B replacement is no longer the implementation direction;
retain its experiments and failure evidence as references.

The user accepted:

- Restore rewinds the existing computer and first creates a recovery checkpoint.
  Fork creates an independent computer.
- Start with manual checkpoints. The recovery checkpoint before Restore is the
  explicit exception; scheduled and other automatic checkpoints are deferred.
- A new fork is stopped and requires explicit Start. It must never briefly run
  as a side effect of creation.
- A fork inherits the source computer's currently approved secret/GitHub
  assignments, gets a fresh identity, and has independent assignment changes.
  Historical grants cannot override current revocations.

Upgrade existing computer data once into the new runtime format. Keep the original
data recoverable through conversion and verification, then use the new format
in normal application paths. The migration can read the old format; Silo does
not need an ongoing legacy runtime or archive reader. Preserve local/remote
ownership, native access and supported device platforms in the new format.

Portable archives use format v3 and the pinned MicroSandbox snapshot save/load
API. The snapshot carries the root disk and the Owned `/workspace` disk as one
runtime checkpoint; Silo does not duplicate workspace files in its own archive
payload. Export is disk-only and requires guest flush when capturing a running
computer. Import verifies the archive and loaded snapshot group/member before it
publishes a new Silo computer identity. The imported computer stays Stopped until
the user explicitly starts it, at which point current host-side assignments
are applied. Format v2 archives are rejected; no legacy archive reader is
retained. This is a deliberate archive compatibility break and must be called
out in release notes.

## Stopped forks and first start

User-facing state: **Stopped**, with **Start** as the action. The computer row
does not add a captured-session subtitle. No additional Pause/Resume concept is
required merely to create a fork.

The implementation must distinguish an ordinary stopped computer from a stopped
computer whose next start restores a checkpoint. Shutting down a running child
after creation would lose its live execution state and allow unwanted work.
Holding a paused child in RAM would allocate a computer before the requested Start.

The implementation retains an immutable full checkpoint, creates the new Silo
computer identity and persists a pending restore reference. Start activates
the child through upstream restore only when explicitly requested.
For a stopped source, use disk state and identify that the first start boots
normally. Forking a running source captures its state while preserving the source.

This is Silo lifecycle integration around upstream immutable snapshots, not a
new snapshot format or memory implementation. The 0.7.2 source reviewed for the initial qualification has no
public full-state restore mode that leaves a stopped sandbox. `msb restore`
calls `RestoreBuilder::restore_with_progress`, awaits the sandbox, and detaches
it; its options are RAM-preserving `--cow-mem` (`--forked` before 0.7.6) and disk-only cold boot. The
`RestoreBuilder` API likewise exposes `forked()` and `disk_only()` but no
deferred-activation or paused-result option. Internally the VM restore starts
paused during construction, then the relay activates the restored guest before
it publishes readiness. That construction barrier is not a user-selectable
stopped state. See that revision's [`restore` command options and flow](https://github.com/superradcompany/microsandbox/blob/60d4dc8a436fb9365491567ec21d073e924e3c6d/crates/cli/lib/commands/restore.rs#L19-L43),
[`RestoreBuilder` API](https://github.com/superradcompany/microsandbox/blob/60d4dc8a436fb9365491567ec21d073e924e3c6d/sdk/rust/lib/sandbox/restore_builder.rs#L56-L79),
[builder restore methods](https://github.com/superradcompany/microsandbox/blob/60d4dc8a436fb9365491567ec21d073e924e3c6d/sdk/rust/lib/sandbox/restore_builder.rs#L166-L195),
and [restore activation](https://github.com/superradcompany/microsandbox/blob/60d4dc8a436fb9365491567ec21d073e924e3c6d/crates/runtime/lib/runner/relay.rs#L1460-L1518).

Represent the stopped computer state with a durable Silo reference to an
immutable native checkpoint member. Silo checkpoint IDs remain the public
identity; persist their mapping to a backend-resolvable native snapshot
reference, and resolve existing records through their saved group/member data.
Restore changes that selected reference, not guest memory: when a computer is
already pending, retain its previous selected reference as the recovery point
and atomically select the requested member. A current-state Fork from a pending
computer points its new stopped child at the same immutable member. Neither
operation starts a computer or allocates guest RAM. For a live source, capture a
checkpoint first; for a pending source, reuse its saved reference. Only explicit
Start calls upstream full restore (`--cow-mem`) or disk restore. Apply current
host network and credential policy before that Start activates the guest.

Upstream accepts snapshot group/member selectors and stable backend snapshot
IDs. Its snapshot archive copy API is for disk snapshot archives; its own source
documentation says full-state snapshots use `save_to`, so `copy_to` is not a
local full-memory clone or alias mechanism. Keep aliases in Silo metadata and
retain the referenced native member while current state, checkpoint history,
recovery, or a pending fork depends on it. See [snapshot references and copy
scope](https://github.com/superradcompany/microsandbox/blob/60d4dc8a436fb9365491567ec21d073e924e3c6d/sdk/rust/lib/snapshot/api.rs#L37-L46),
[`Snapshot::copy_to`](https://github.com/superradcompany/microsandbox/blob/60d4dc8a436fb9365491567ec21d073e924e3c6d/sdk/rust/lib/snapshot/api.rs#L249-L257),
and the [`SnapshotCopyBuilder`](https://github.com/superradcompany/microsandbox/blob/60d4dc8a436fb9365491567ec21d073e924e3c6d/sdk/rust/lib/snapshot/copy.rs#L13-L56).

Required lifecycle rules:

- The first-start requirement survives app/owner restart and is enforced by the
  owner, including when a remote controller requests operations.
- Do not inherit automatic-start selection. Launch preferences, desktop opening,
  Files refresh, terminal/editor helpers and implicit `exec` must not activate a
  pending fork. Route a user's explicit Start through the common lifecycle path.
- Pin the checkpoint and its dependencies while a pending fork needs them.
  Removing the source or a history entry must not break an existing fork. Block
  deletion until references are resolved if independent retention is not proven.
- Apply current host-side network and credential policy before guest execution
  can use external services. Updating policy after resume is not sufficient.
- Consume the pending restore only after durable successful activation. A failed
  first start remains retryable and must not silently cold-boot or double-start.
- Ordinary Stop/Restart behaviour remains unchanged once the fork is active.
  Resource edits that invalidate a saved memory state require an explicit
  disk-only start decision; never discard memory implicitly.

Current seams: [computer lifecycle](../app/SiloUI/src-tauri/engine/src/runtime/mod.rs),
[frontend source contract](../app/SiloUI/src/features/application/model/application-source.ts),
[computer presentation](../app/SiloUI/src/features/computers/model/computer-presentation.ts),
and [lifecycle recovery](../app/SiloUI/src-tauri/engine/src/runtime/lifecycle_recovery.rs).
Current code includes automatic startup and temporary boot through helpers;
these require explicit pending-restore guards.

## Product flows

| Action | Proposed behaviour |
| --- | --- |
| Create checkpoint | Name an immutable point in this computer's history. Running computers capture memory and both owned disks; stopped computers capture disks. Show the scope clearly. Preserve the source's lifecycle state. |
| Fork current state | Capture a live source or reuse a pending immutable reference, choose a new name, and create a stopped sibling computer. No child guest execution until Start. |
| Fork checkpoint | Same stopped-sibling flow using the selected point. Copy current source assignments, never historical grants. |
| Start pending fork | Resolve current policy, validate compatibility, restore the selected state and open the normal computer. Reconnect desktop/terminal/LCU sessions as available. |
| Restore checkpoint | Secure the current state as recovery, then atomically select the target immutable reference. Keep the computer stopped; explicit Start performs full-memory restore or disk boot under current device policy. |
| Export/import | Keep a portable recovery path and existing archive import. Clearly distinguish disk recovery from memory continuation. Use upstream archive mechanisms wherever compatible. |
| Delete checkpoint/fork | Explain retained dependencies and actual reclaimable space when known. Preserve artifacts still required by other computers. |

Put checkpoint history in the computer UI, with Create checkpoint, Restore and
Fork actions. Keep Export/Import discoverable separately from local checkpoint
history. Show actual operation stages, errors and recovery actions; do not imply
that a progress percentage establishes a usable checkpoint.

### Checkpoint action feedback (2026-09-27)

The computer row shows an indeterminate progress bar as soon as Create,
Restore, or Fork is submitted and keeps it visible until the request settles.
The computer's Checkpoints menu expands the panel inside its existing card;
the panel itself has no redundant outer rounded border or duplicate progress.
Restore uses the shared inline Cancel/Confirm pattern; its tooltip explains the
recovery checkpoint and stopped result. Fork names are entered in a small
popover with Enter-to-submit. The Overview current-state fork dialog and computer
row also show progress while a request is pending. The production source keeps
pending operations visible across refreshes and page changes and rejects duplicate
requests for the same computer, including remote targets.

Checkpoint commands use the existing five-second lifecycle lock wait. This
avoids failing immediately when brief background work owns the mutation lock;
sustained contention still returns Busy. The causal sources are
[`read_application_state`](../app/SiloUI/src-tauri/engine/src/runtime/mod.rs), which may
inspect stopped-computer log retention under that lock, and
[`ssh_access::start_monitor`](../app/SiloUI/src-tauri/engine/src/ssh_access.rs), which
holds it during reconciliation. The
[`checkpoint commands`](../app/SiloUI/src-tauri/engine/src/runtime/checkpoints.rs)
previously used an immediate `try_lock`.

Runtime snapshots intentionally reject observations during a mutation, and the
snapshot runner does not expose byte progress for checkpoint capture. The UI
therefore reports the pending action without inventing a completion percentage.
The row's ellipsis and reorder handle stay mounted but disabled during an
operation, preserving the positions of the other controls.

Remote status reads use the same snapshot guard. Its
`SILO_SANDBOX_UPDATE_IN_PROGRESS` sentinel means that a consistent read was
unavailable: a mutation lock was held before or after the read, metadata changed
during it, or a durable configuration recovery was pending. The background SSH
monitor also uses that lock, so the sentinel does not establish that computer settings
are changing. The controller preserves the previous snapshot and marks that
device's rows stale until a successful refresh; its ten-second polling interval
can turn a brief collision into a much longer visible busy state. See
[`refreshDevices`](../app/SiloUI/src/desktop/production-source.ts) and
[`read_application_snapshot`](../app/SiloUI/src-tauri/engine/src/runtime/mod.rs).

The owner now retries one complete read after 100 ms for transient lock or
metadata collisions. It discards the first observation, preserves the immediate
defer for durable recovery, and returns real errors without retrying them. The
controller labels deferred reads “Refreshing status” and keeps known Start,
Stop, Restart, and checkpoint progress visible ahead of that fallback. The retry
requires an updated owner build; the corrected label also works with older
owners. This diagnosis follows the code paths and deterministic regressions;
it does not measure collision frequency on a user's remote device.

The progress/layout follow-up passed 185 frontend tests across the checkpoint
panel, current-state fork dialog, Overview fork/failure behavior, production
source, application shell, and machine list. Typecheck and lint passed. Four
focused native snapshot tests cover discarded observations, bounded contention,
durable recovery, and unretried runtime errors. The broader runtime suite passed
91 tests; its Unix-socket test passed separately with socket permission.
The final `desktop:build` passed signing and bundle verification at
`app/SiloUI/src-tauri/target/release/bundle/macos/Silo.app`. This bundle was not
launched; the running app in the separate `checkpoint-state-build-fu327l8s`
directory and its computers were left untouched.

After Restore, the old runtime instance is absent and the selected snapshot
reference becomes the computer's authoritative stopped state. Restore can be
repeated without Start: preserve the current reference as recovery, then commit
the new reference in one journaled metadata transition. Current-state Fork from
this state uses that same selected reference. A pending computer cannot create
a new captured memory point without activating a computer; it may create another
logical checkpoint alias to the selected immutable member. Do not report an
alias as newly captured data or delete its native member while another Silo
reference still uses it. A live computer may capture before retargeting; a
pending computer uses the saved immutable reference for recovery.

Create, Restore, and Fork remain available while the selected snapshot is
stopped. Capture code avoids inspecting an absent runtime and verifies the
selected member before naming it. Each Restore adds a distinct logical recovery
checkpoint and commits it with the new selection in one atomic record save.
Older checkpoint IDs remain valid native selectors when no explicit `nativeId`
is stored. Local and remote user Start actions now use the same
`explicit_computer_action_with` path; background startup retains its guard.
Current credentials, network policy, and secret assignments apply before guest
execution. Archive import keeps its new-computer meaning.

Verification for this change used deterministic data, not live computer operations:

- `npm --prefix app/SiloUI test -- src/features/application/components/checkpoint-panel.test.tsx src/features/application/components/fork-state-dialog.test.tsx src/features/application/pages/overview-fork.test.tsx src/desktop/production-source.test.ts`: 85 passed.
- `cargo test --manifest-path app/SiloUI/src-tauri/Cargo.toml runtime::checkpoints::tests`: 15 passed, using explicit synthetic GitHub configuration. This covers full-state aliases, repeated Restore and recovery selection, native-ID resolution at explicit Start, failed Start preservation, current-state Fork without activation, runtime collisions, and legacy group migration.
- `cargo test --manifest-path app/SiloUI/src-tauri/Cargo.toml runtime::tests::`: 88 passed and one Unix-socket test was denied by the command sandbox. That exact test passed when rerun with socket permission. This run also used synthetic GitHub configuration.
- Frontend `build` (including TypeScript), `lint`, and `git diff --check` passed.

A read-only review found no blocker in identity resolution, recovery, policy
application, or the explicit Start boundary. The optimized macOS app was built
with `desktop:build` under the separate absolute `CARGO_TARGET_DIR`
`app/SiloUI/src-tauri/target/checkpoint-state-build-fu327l8s` in this checkout.
The resulting `release/bundle/macos/Silo.app` passed the build wrapper's signing
and bundle verification. It was not launched; the existing running release
bundle and user computers were left untouched. Tests establish deterministic behavior
and packaging, not live session continuity or remote-device deployment.

For v1, retain checkpoints until explicit deletion; show measured usage and fail
cleanly when storage is insufficient. Do not add an unrequested automatic
deletion policy. A snapshot can contain browser cookies, application tokens and
unsaved user data already present in the guest, even though brokered provider
credentials stay on the host. Apply private storage/export handling accordingly.

### Checkpoint usage survey scope

The Checkpoints panel needs byte sizes, saved references, native children, and
live lineage positions to explain Delete availability. Empty histories return
zero bytes without surveying other computers. When all selected native members
are missing, saved references still determine blockers, but live lineage cannot
block removal of absent native data and is not surveyed.

Existing members require the complete dependency survey: an unconfigured computer
can build on a selected member, and a child can belong to another group. Do not
restrict this survey to configured computers or the selected lineage group.
Deletion and reclamation retain their complete, fail-closed surveys. No shared
survey cache is introduced, so capture, Restore, fork, import, and deletion are
observed on the next read without an invalidation protocol.

Storage's byte-only totals use the record's lineage group, falling back to the
computer name for legacy records. The pinned runtime supports
[`snapshot list --group`](https://github.com/superradcompany/microsandbox/blob/09df3d4b9d832adaede1fb9a198cfc660bfab8cd/crates/cli/lib/commands/snapshot.rs).
This limits the JSON returned to Silo; upstream still enumerates snapshots before
filtering. The pinned [`list` JSON](https://github.com/superradcompany/microsandbox/blob/09df3d4b9d832adaede1fb9a198cfc660bfab8cd/crates/cli/lib/commands/list.rs)
does not contain desired or active lineage, so it cannot replace individual
inspections for existing members.

The 2026-10-02 regression run compared the complete survey with usage reads for
1, 10, and 100 listed runtime sandboxes. These are single samples from an
in-process `RuntimeRunner` fixture, including JSON parsing and temporary-file
reads, with one managed record and an unrelated native member. They measure
fixture execution, not native process startup or live computer latency. Call counts
are assertions; elapsed times are diagnostic output, not timing thresholds.

| Sandboxes | Full survey calls | Empty history calls | Empty time: survey → usage (µs) | Missing-member calls | Missing time: survey → usage (µs) |
| --- | --- | --- | --- | --- | --- |
| 1 | 3 | 0 | 315 → 54 | 1 | 254 → 146 |
| 10 | 12 | 0 | 539 → 53 | 1 | 530 → 132 |
| 100 | 102 | 0 | 3735 → 110 | 1 | 3459 → 200 |

Four regressions also cover unreadable unrelated histories for an empty owner,
saved dependencies for missing members, inherited and legacy storage groups,
unconfigured lineage owners, and children in other groups. The focused
`cargo +1.94.0 test --manifest-path app/SiloUI/src-tauri/Cargo.toml --locked checkpoint_ -- --nocapture`
run passed 28 tests with three live tests ignored, using the shared target and
[synthetic unit-test GitHub configuration](SiloUI-RELEASES.md#local-setup).
Typecheck, lint, and Rust formatting passed. No bundle was inspected or launched.

## Implementation order and evidence gates

### 1. Qualify the upstream runtime in isolation

Start from the released 0.7.2 candidate established in the
[research](research/checkpoints-desktop-direction-2026-09-24.md), inspect relevant
subsequent fixes, and select an exact release/commit with matching runtime,
guest kernel and build inputs. Do not adopt a moving branch. Use disposable
runtime homes and synthetic credentials; do not upgrade the user's data to test.

Inventory each Silo patch: upstreamed, still needed, or replaced by a supported
API. Preserve behaviours for stopped creation, archives, selective TLS, active
revocation, GitHub scopes, port/SSH publication, storage and progress. Keep narrow
remaining patches until their upstream replacements pass; do not mechanically
apply the old patch or build a generic runtime abstraction for one backend.

Create a one-command probe with Silo's actual root and separate workspace disk.
Establish whether the workspace volume is captured as owned storage by the new
runtime. It must become a private child disk; binding the same ext4 file to both
computers is not a fork. If ownership adoption is needed, qualify it on a copy first.

Pass criteria: memory-only marker and unsaved editor buffer survive; both disk
markers survive; source and child diverge independently; a saved checkpoint
restores after owner and device restart; a pending fork remains inactive until
Start; current grants apply before its first external request. Record guest pause
time, capture/restore latency, physical storage and resident-memory growth, and
compare with the current backup baseline. No performance target is claimed met
before measurement.

Exercise capture/restore interruption and full-disk failure using the
[E2B preservation evidence](research/e2b-qualification-gates-2026-09-23.md) as test
design input. An upstream failure must leave identified recoverable state.
Unsupported behaviour becomes a minimal upstream reproduction, not a custom
hypervisor implementation or a weaker product promise.

### 2. Integrate the upgrade and one complete checkpoint flow

Update [runtime inputs](../app/SiloUI/runtime-inputs.json),
[preparation](../app/SiloUI/scripts/prepare-microsandbox-runtime.mjs),
[runtime packaging](SiloUI-RUNTIME-PACKAGING.md) and the retained patch set using
the qualified candidate. Recheck source hashes, licenses, signing and device floors.
Implement a one-time, versioned, resumable migration on disposable copies first.
The upgrade opens into a full-app progress overlay before normal computer recovery or
automatic startup. Show live stages and safe logs. Convert each old computer to the
qualified new storage format, verify its root and separate workspace data and
identity, and retain originals until conversion is proven. New application
paths then read only the new format. Do not assume downgrading the binary
reverses a data-format upgrade.

On failure, keep the overlay and journal so Retry resumes safely. Offer logs and
an issue-report action, but never submit an issue or upload logs without an
explicit user action. The user can back up their work manually and choose to
continue after acknowledging that failed computers were not migrated. Continue must
enter a usable new-runtime state without presenting failed computers as migrated or
deleting the originals. These are accepted product choices; the exact runtime
conversion commands and archive handling depend on the qualification probe.

Implement one end-to-end manual checkpoint: owner operation, durable metadata,
progress/error reporting, frontend source and computer history. Keep runtime
capture/storage upstream; Silo owns identity, policy, journaling and presentation.
Add meaningful behaviour tests as each flow is implemented, starting at this
seam, then a live test of actual state capture. Build the fixture preview at this
stage; do not defer all UI work until the backend is finished.

### 3. Add stopped forks and explicit first start

Implement the pending-restore lifecycle and durable artifact retention. Inherit
assignment references at fork time, not secret values. Before Start, resolve
those references against the current device store and grants. Source-only later
assignment changes remain independent; global deletion/provider revocation must
still deny access. Assign fresh computer/runtime/viewer identities and endpoints.

Test app restart, owner disconnect, repeated Start, source deletion, checkpoint
deletion, current revocation, cross-computer access and implicit-start paths.
Use an independent recording origin to prove zero child requests before Start.
Memory and files must continue independently after explicit activation.

### 4. Add recovery-protected in-place restore

Keep the stable Silo computer identity while changing its runtime generation.
Serialize against lifecycle/configuration and backup operations. Preflight the
selected artifact, capture the recovery point, stage replacement state, and use
a durable operation journal to commit routing/metadata consistently. Retain the
old generation until recovery and replacement state are secured.

For a running target, establish a quiesced boundary that spans recovery capture
and replacement. Do not let the old guest resume and accumulate unprotected
writes between those steps. Record its prior lifecycle state so cancellation or
failure can restore it deliberately. Prove this sequence through supported
upstream pause/capture semantics rather than guest-side timing assumptions.

Recovery-point failure must prevent replacement. A crash at any phase must
identify the original or replacement generation and allow a deterministic retry
without deleting user state. Preserve current assignments, rotate access sessions
and invalidate stale viewer/editor endpoints. Test external mounts explicitly;
do not claim rollback of shared device files or external API effects.

Failed full captures must settle the source independently of snapshot cleanup.
At pinned MicroSandbox revision
`09df3d4b9d832adaede1fb9a198cfc660bfab8cd`, the
[checkpoint coordinator](https://github.com/superradcompany/microsandbox/blob/09df3d4b9d832adaede1fb9a198cfc660bfab8cd/crates/runtime/lib/checkpoint/coordinator.rs#L950-L1018)
retains suspension after failed source resume or workload thaw. The
[executor](https://github.com/superradcompany/microsandbox/blob/09df3d4b9d832adaede1fb9a198cfc660bfab8cd/crates/runtime/lib/runner/control/executor.rs#L254-L270)
rejects ordinary resume for recovery-owned suspension. Silo therefore inspects
the source with cancellation masked, attempts resume, verifies the resulting
state, and uses its existing Restore force-stop policy only if the source remains
Paused. The
[CLI force-stop command](https://github.com/superradcompany/microsandbox/blob/09df3d4b9d832adaede1fb9a198cfc660bfab8cd/crates/cli/lib/commands/stop.rs#L76-L88)
kills the selected runtime; it preserves disks and runtime logs but loses RAM and
cannot guarantee a clean guest shutdown. Do not release the upstream recovery
pause directly or delete the computer to recover it.

If inspection, resume and stop cannot establish Running, Stopped, Created or
Crashed, retain the existing in-flight full-capture journal and report the recovery
failure alongside the capture failure. Ordinary Start/Stop, capture retry and
launch recovery can then retry settlement for that journal's managed computer identity.
Fixture tests cover errors, timeouts, cancellation, verification failure, failed
resume, a misleading successful resume response, and failed settlement followed
by ordinary recovery. These tests do not qualify live guest health; disposable
Silo Dev computer failure/relaunch verification remains a separate live check.

### 5. Finish retention, portable recovery and lifecycle integration

Qualify upstream reference retention, deletion, compaction and integrity checks.
Do not write another CoW store or deduplication engine. Handle storage exhaustion
and partial reclamation honestly. Migrate old archive data once where supported,
then prove a complete new-format export/import on a clean runtime home without
the original cache.

Integrate operation recovery with Quit, update and remote-owner behaviour.
Validate compatible runtime updates and make incompatible memory states explicit,
including the separately tested disk-only recovery path. Full-state cross-CPU
migration is not promised. Remote forks initially stay on their source execution
device; portable transfer remains an export/import operation subject to compatibility.

Use the existing [backup controller](../app/SiloUI/src-tauri/engine/src/backup_controller.rs),
[backup recovery](../app/SiloUI/src-tauri/engine/src/backup_controller/recovery.rs),
[storage](../app/SiloUI/src-tauri/engine/src/runtime/storage.rs),
[update recovery](../app/SiloUI/src-tauri/engine/src/runtime/update_recovery.rs),
[shutdown](../app/SiloUI/src-tauri/engine/src/runtime/shutdown.rs) and
[remote operations](../app/SiloUI/src-tauri/engine/src/runtime/remote_ops.rs) as integration
seams. Refactor only where the new behaviours require it.

### 6. Verify the supported product and document the release

Run focused frontend/native behaviour tests during implementation, followed by
typecheck, lint, frontend tests, appropriate native tests and release-tooling
checks for changed packaging. Build and inspect an exact packaged bundle.
Record fixture and live results separately.

The live matrix includes supported Apple Silicon macOS and Linux ARM64/x86-64,
local and remote ownership, actual desktop state, SSH/editor access, Git/LFS and
credential revocation. Reuse existing tests where they exercise the contract.
Unrun platforms remain explicitly unqualified. Do not equate compilation with a
checkpoint durability pass or silently raise supported platform requirements.

Add the user-facing minor changeset and update application help and implementation
docs. Do not bump versions, consume changesets, tag or publish as part of this plan.

## Desktop and LCU workstream

Start the display/LCU compatibility investigation early using the existing guest
as the control. It can proceed alongside runtime qualification, but does not
justify replacing the distribution, viewer and VM engine in one experiment.
The subsequent deliverable is a prepared desktop image and a measured viewer
choice using the [desktop selection criteria](SiloUI-DESKTOP-SELECTION.md).

Keep LCU's integration seam while it is refined. Qualify its shared desktop
session, input/capture coordinates, accessibility, human takeover and reconnection
against the selected version. Do not implement a second computer-use stack. New
distribution selection, automatic checkpoints and general hibernation are separate
follow-ups after the core checkpoint/fork flows have passed.

## First action completed

The isolated upstream probe verified full-memory restore, independent root and
Owned `/workspace` disks, stopped creation, current host policy before activation,
and migration of copied old workspace data. The live evidence used disposable
runtime homes and synthetic credentials; it did not exercise a packaged Silo app,
host restart, or the supported Linux platform matrix.

## MicroSandbox 0.7.2 security parity checkpoint

The standalone 0.7.2 network patch (`microsandbox-silo-network-0.7.2.patch`,
replaced by the per-feature patches in the 0.7.4 rebase) applied to the
official tag. It adds selective TLS interception for currently
allowed secret destinations, closes existing proxy connections when secret
policy changes, and carries managed SSH, `exec --no-start`, and SFTP login-home
behaviour. Its GitHub profile selector substitutes a scoped token only in an
exact HTTP/1 Authorization header; a synthetic test proves the JSON profile
does not leave in that request. The user authorized bounded read-only owner
discovery for ambiguous multi-owner GraphQL requests. The port sends routing
probes only to `https://api.github.com/graphql`, using the first configured
owner's read token for schema inspection and each configured owner's read token
for node ownership. It holds fragmented and pipelined guest bytes until owner
selection succeeds, then forwards the original request once with the selected
token. Malformed, ambiguous, partial-error, and failed lookup results block the
request. The resolver never sends the guest mutation as a discovery probe.

The 0.7.2 patch passed `git apply --check` against the official source
extraction. All 13 focused network GitHub tests passed, including read-token
selection, failed and ambiguous lookup, and unsent request buffering. The full
network library suite first reported 549 passes and 25 failures because the
sandbox denied local socket binding with `Operation not permitted`. A rerun
with local socket access passed all 574 tests. These were synthetic tests: no
live GitHub token, repository, computer, or guest traffic was exercised. CLI
compilation must be rerun on the combined source. The SSH feature test did not
run because the isolated build exhausted local disk space.

The synthetic passthrough test used task-local worktree baseline
`f9d72d200cf81cb46bc0186d2ded76d205909b35`, whose workspace manifest
identifies MicroSandbox 0.7.2. This is a local baseline commit, not an upstream
Git commit. The official source revision used for the final patch apply check
is `60d4dc8a436fb9365491567ec21d073e924e3c6d`.

The old Silo proxy let public `$MSB_*` placeholders pass unchanged outside
assigned secret destinations while still substituting real values only for
allowed hosts. This permits an agent tool result containing a placeholder to
reach a model API without terminating the connection. The new patch retains
upstream's restrictive default. Silo now expresses its documented public
placeholder policy through MicroSandbox's supported per-secret
`passthrough=*` option on Silo-managed Env-source secret references. Their
existing exact allowed-host lists still control real-value substitution; the
host keeps the raw credential and the guest receives only the opaque reference.
The internal `SILO_GITHUB` profile reference uses the same per-secret setting.
No global `SecretsHandle::new` override is used. A synthetic upstream handler
regression covers allowed-host substitution and unchanged forwarding to an
unrelated host. General user secrets outside Silo's configured Env-source slots
and upstream defaults remain restrictive.

## Integration verification record

On Node 24.11.1, frontend typecheck and lint passed; Vitest passed all 941 tests
across 104 files. Native tests used explicit synthetic GitHub build values. The
final `cargo test --quiet` run passed 492 tests, failed none, and ignored 12
opt-in live tests. Its preserved output is
`app/SiloUI/src-tauri/target/verification/integration-validation-final.txt` (untracked local evidence).
Focused migration, checkpoint, archive, owned-storage and startup tests also
passed during integration.

One earlier parallel native run failed the owned-storage history test while
acquiring the per-runtime command lock. A bounded regression reproduced the
shared-lock lifetime error before the fix; the corrected lifecycle passed the
four focused lock tests, the history regression, and the final full native run.

The isolated optimized macOS package was rebuilt after the migration and GitHub
restore-policy fixes. The migration journal now persists `complete` on the first
verified selected-generation startup. A later fork may change the computer count
without being mistaken for a failed conversion; an uncompleted conversion still
has to match the original migrated count. The focused migration module passed
7/7, including converted-fork and clean-generation-after-create restart cases.

Packaged macOS UI and guest checks passed with a disposable, one-computer legacy
fixture. Retry converted 1/1 after the copied fixture's mount path was corrected
to its isolated app-data directory; the original fixture workspace hash remained
unchanged. The normal overview showed the migrated source Stopped. We explicitly
started it, created and forked a full/manual checkpoint, and explicitly started
the stopped fork. Source and fork had separate writable qcow layers; source-only
and fork-only files stayed absent from the other guest, and their sentinel
changes remained independent. Restore created a full “Before restore” recovery
point. After graceful app Quit and relaunch, the app opened without the migration
gate even though the selected converted metadata contained both source and fork.
The source stayed stopped until explicit Start, which restored the checkpoint
sentinel and removed both post-checkpoint files. The fork remained stopped.
Evidence, exact IDs and hashes are in
`app/SiloUI/src-tauri/target/verification/packaged-macos-checkpoint-fork-restore-20260925/RESULTS.txt` (untracked local evidence).

The final isolated bundle compiled the GitHub current-target profile selection
and was verified by the local macOS bundle verifier and `codesign --verify`.
Its identifier and executable/runtime hashes are in
`app/SiloUI/src-tauri/target/verification/packaged-macos-checkpoint-fork-restore-20260925/final-combined-package.txt` (untracked local evidence).
The target-specific selector regression and synthetic signed-runtime restore
test each passed 1/1. An authorized live GitHub acceptance test passed 1/1 in
117.47 seconds: the source had write access; before first child execution its
current assignment was read-only; clone and read APIs succeeded while issue
mutation and Git push failed. No real token entered the guest. Test issue and
branches were cleaned up and child tokens were revoked. No unrelated repository
was accessed.

Linux qualification completed on a disposable Ubuntu 24.04 x86-64
container hosted by Ubuntu 26. KVM API 12 and actual VM-handle creation passed.
The ordinary AppImage smoke passed 10/10. A genuine predecessor fixture was
created with installed Silo 0.6.3 / MicroSandbox 0.6.17 and the Ubuntu 24.04 v2
guest, then provisioned using the HEAD-patched 0.6.17 account migration utility.
The guest migration passed: UID 1001 `silo`, required SFTP server, preserved
workspace marker and ownership, and the stopped computer's verified working-account
label were checked. Its root snapshot backup remains verified. The first
attempt's apt failure was caused by the fixture's disabled network; only the
disposable computer was recreated from its verified snapshot with the supported
`public` network profile before retry. Packaged Silo conversion passed 1/1. The
production UI showed the source stopped; explicit Start, full checkpoint,
stopped fork, RAM-marker and guest-process survival, independent source/fork
workspace writes, recovery-point restore and stopped pending-restore across app
restart all passed. A recovery-fork bug while the source was pending restore was
fixed; its real-UI retry successfully created and started that fork while
preserving the pre-restore source state. The latest expanded run passed 11
lifecycle assertions through post-restart rollback of disk, RAM marker and
guest process. The Linux qualification also exposed and corrected backup-history
re-quarantine after completed migration, missing 4 GiB defaults and 0.7 config
canonicalization, temporary snapshot-ancestry retention, snapshot-index reads
truncated above 32 KiB, archive head selection in multi-member groups, snapshot
selectors incorrectly constrained by computer-name limits, and the disk-only start
flag for a disk snapshot. The production UI exported a v3 archive and its
native head/integrity checks passed; the imported computer explicitly started and
restored workspace bytes. An earlier repeat export failed because Silo did not
persist a stable snapshot group. Silo now persists lineage groups across
source, import, fork, backup, restore, and relaunch. Focused regressions and
the final ARM64 and x86-64 same-home matrices passed on their separate
authentic-predecessor fixtures. Do not attribute
the earlier failure to MicroSandbox alone. The source-only archive export UI
passed with the native GTK folder chooser; its selected destination and
archive are captured in [Linux verification](SiloUI-LINUX-VERIFICATION.md). The final x86 runtime-8 AppImage payload, manifest, tool versions, protocol probes, and dependency checks passed, as did its live port-control proof; exact hashes and evidence are recorded in [Linux verification](SiloUI-LINUX-VERIFICATION.md). This qualification used nested KVM under Ubuntu 26, not bare-metal Linux.
The larger lifecycle/archive
path also used production Tauri IPC with a preseeded isolated destination.
Backup-history startup and imported deny-all
re-export eligibility fixes have focused regressions; the latter's final
x86-64 end-to-end matrix passed. The later runtime-7 x86 AppImage
(SHA-256 `f741e941c7d215835282ab7159b7ae32cef50bfba51311aceb5593ed9a4b1189`)
passed the authentic migration and same-home export matrix; see the Linux
acceptance record for its seven verified exports and resource checks. The
earlier fresh-home x86 archive import failed at Start because its VMDK pointed
into the retired source `MSB_HOME` cache. The pinned portable-image-cache fix
then passed the production import/Start path with the source cache absent; its
restored VMDK and 40 GiB capacity were verified in a separate fresh XDG/MSB_HOME.
The compact result is recorded in the Linux acceptance research note. A separate
live current-0.7.2 utility apply on a
v3 guest passed account, workspace, descriptor-discovery and snapshot
verification checks; interrupted `--resume` is covered by the focused 11-test
utility suite, not a live interrupted run. Compact evidence is
`app/SiloUI/src-tauri/target/verification/linux-account-migration-072-20260925.txt` (untracked local evidence).
The separate earlier 0.7.2 computer staged under the old runtime directory was
synthetic and is not evidence of legacy migration compatibility. A later
Ubuntu 24.04.4 ARM64 Lima VM on Apple Silicon passed nested-KVM API and
VM-creation checks. Its stopped Ubuntu 24.04 v2 guest was created by
MicroSandbox 0.6.17 and converted through the production migration gate. The
terminal journal, stopped source overview, and post-migration Start were
verified. Production UI lifecycle passed 14/14 assertions, including a full
checkpoint fork that restored a tmpfs marker and guest process, independent
source/fork disk writes, recovery checkpoint restore, and stopped state across
app restart. A stale fork-name collision in an earlier run had caused the UI
to reject a duplicate while the harness accepted an old Start control; the
final helper requires a unique child and verifies its pending checkpoint
before Start. The final ARM AppImage passed native WebKit smoke with its live
APPDIR resolving the packaged tools and all three dependency rows checked. Its
six managed ELF payloads matched their prepared SHA-256 values. The final
lineage matrix passed one same-home archive import, explicit Start, fork and
relaunch, followed by two valid exports each for source, imported computer, and fork.
The source's two exports were validated before the deny-all eligibility-only
change; the resumed run on the rebuilt DEB reused the existing import/fork and
created no additional imports (`newImportsCreated: 0`). A separate cold-cache
check then exported one 1 CPU / 1 GiB / 10 GiB guest through production IPC,
imported it once into a fresh app data/home, and explicitly started the
imported guest with both root and workspace markers intact. Before the final
Start, the stopped source's original `MSB_HOME` alias and canonical
`runtime/microsandbox` backing directory were both absent; the 323,035,136-byte
allocated backing store remains preserved in task evidence. The imported
guest's VMDK base, raw managed root, and qcow2 restore overlay all resided in
the destination cache; `qemu-img` reported 10 GiB virtual root capacity.
Evidence is in `app/SiloUI/src-tauri/target/verification/arm64-cold-cache-qualification-2026-09-26/` (untracked local evidence).
Saved records, native inspection, and
physical raw-image sizes agreed at 1 CPU, 1024 MiB RAM, and 4096 MiB root disk
for all three computers. The first matrix attempt exposed that the importer
intentionally preserves deny-all networking while backup eligibility only
accepted the default policy. Backup validation now accepts exactly the default
profile or its deny-all policy; custom rule sets remain rejected. Focused
regressions passed before the final package build. Evidence is in
`app/SiloUI/src-tauri/target/verification/arm64-final-qualification-2026-09-26/` (untracked local evidence)
and the separate
[Linux acceptance record](research/silo-linux-acceptance-2026-09-25.md).
This guest test does not claim bare-metal Linux ARM64 coverage. The later x86
production desktop-session run preserved an unsaved Mousepad buffer through
checkpoint, stopped-fork Start, and source restore; see the September 27
section of the Linux acceptance record for the runtime/package and evidence
paths. Release signing and
distribution are publication steps outside this implementation qualification.

The installed `/Applications/Silo.app` was restored and its exact executable
was observed on the normal overview with `dev` and `hermes` stopped. No
production computer was started. The updated sanitized staged-inspection message has
focused native coverage; the corrected message was not separately inspected in
the UI.

### Imported full checkpoints: restore mode, 2026-10-02

Import requests disk restoration even when the native artifact contains full
execution state. Start retains the runtime inventory's native scope separately
from that desired mode and sends `--disk-only` only for full artifacts. Actual
disk captures cold boot without that flag; full checkpoint forks retain
`--cow-mem`. The pinned [restore CLI](https://github.com/superradcompany/microsandbox/blob/09df3d4b9d832adaede1fb9a198cfc660bfab8cd/crates/cli/lib/commands/restore.rs#L193-L198)
selects disk restoration explicitly. The virtio-fs device-state budgeting patch
changes capture limits, not this selection contract.

A deterministic production Start test covers all three combinations. The
opt-in full-checkpoint export/import test also checks a changed boot ID and
absence of a captured process and RAM-only file; it remains unexecuted here.

### Checkpoint environment defaults, 2026-10-02

Every checkpoint Start sends `GH_TOKEN=$MSB_SILO_GITHUB`, matching fresh
creation. The [bundled restore-policy patch](../app/SiloUI/patches/microsandbox-restore-policy-0.7.6.patch)
supports destination `--env` arguments for new exec commands; resumed processes
retain their captured environments. The placeholder resolves through the
current target's host-side GitHub profile, including the disabled profile.
Neither the restore arguments nor the imported pending record carry an old
`GH_TOKEN` value.

Other environment entries accepted by backup validation are portable key/value
defaults. Import persists them before committing computer settings, checks their
shape and NUL-free values, and passes them as individual `--env` arguments on
Start. Forks retain those imported defaults. Current GitHub identity
reconciliation still owns author/committer settings. Arbitrary environment
values remain archive contents, so export warnings about sensitive data apply.

Deterministic tests exercise disk/full Start with denied and synthetic authorized
profiles, imported environment persistence and argument boundaries, and invalid
entries. They do not prove a live GitHub CLI request.
