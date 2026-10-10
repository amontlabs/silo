# macOS computer checkpoints: how existing tools do it

Research, 2026-10-10. Source review of Lume and cua-vmm (local clone of
`trycua/cua`), Tart and UTM (GitHub, read through the API), and the installed
VMPal 0.42 (bundle strings only; its MCP tools were used read-only and it has
no VMs, so no live behaviour was observed). Follows
[macOS guest engine](macos-guest-engine-2026-10-09.md).

## Requirement

Checkpoints for macOS computers on Virtualization.framework: memory plus disk
when running, disk only when stopped, restore, fork (new computer from a
checkpoint) and delete.

## Summary and decision

- No tool offers all of it. Lume has no memory save at all; Tart has a single
  suspend slot, not named checkpoints; UTM (macOS 27+) and VMPal have named
  checkpoints with memory.
- Every tool that saves memory does the same framework sequence: validate,
  `pause`, `saveMachineStateTo`; restore on a freshly built stopped VM with an
  identical configuration, then `resume`. Silo uses that sequence directly through
  `objc2-virtualization`; there is nothing to embed.
- Reuse as design, not code: Tart's file name and "state file marks Suspended"
  rule, UTM's per-checkpoint layout (disk layer + auxiliary storage copy + state
  copy, restore is all-or-nothing, state invalidated when disk changes) and
  VMPal's restore-failure behaviour (cold boot with a notice).
- Silo adds the parts nobody does well: a recorded fingerprint (configuration,
  host build, hardware model) per checkpoint so an unusable state is detected
  before use, and a disk-only fallback that keeps the checkpoint valid.
- Disk copies: use APFS clones (`clonefile`) of `disk.img`, `aux` storage and
  `hardwaremodel`/`machineidentifier`. UTM's ASIF layers need macOS 27 and
  DiskImageKit; not a baseline.
- Tart is read for insight only; no code copied.

## Comparison

| Product | License | Snapshot mechanism | Running VM | Restore failure | Verdict |
| --- | --- | --- | --- | --- | --- |
| Lume 0.6.1 | MIT | None. No `saveMachineState` in `lume/src`; `clone` only (CoW copy of a stopped VM) | Must stop first | n/a | Reference for stopped-disk clone only |
| cua-vmm (Cua) | MIT | `Runtime::checkpoint`/`fork`/`delete_checkpoint` over Lume: stop, clone into a new stopped VM named for the checkpoint, restart | Stops the VM, so no memory; `suspend` is just `stop`, `resume` is boot | n/a | Good API shape (checkpoint = a stopped VM; fork = clone of it); no memory |
| Tart | FSL-1.1-ALv2 (insight only) | One `state.vzvmsave` per VM in the VM directory. `tart suspend` signals the `tart run` process, which pauses and saves, then exits | Pause, save, exit; next `tart run` restores | Error exits the run; state file is deleted only after a successful restore, so it stays and the next run fails the same way | Proves the flow; single slot, weak failure story |
| UTM 4.x (Apple backend) | Apache-2.0 | Named snapshots on macOS 27+: ASIF disk layers per snapshot, copies of aux storage and of the saved state. One `suspend` slot otherwise | Pauses, saves state, copies it into the snapshot | Restore validates all layers first; error leaves VM stopped | Best layout reference; needs macOS 27 for layers |
| VMPal 0.42 | Proprietary | Named snapshots, independent of each other, each costing only the changes after it plus memory if running | Pauses "a few seconds"; snapshot records it was taken running or stopped | Falls back to a cold boot and says the saved session could not be restored | Best behaviour reference; closed source |

## Lume and cua-vmm

- Lume: `grep` for `saveMachineState`, `restoreMachineState`,
  `validateSaveRestoreSupport`, `suspend` finds nothing in
  [`libs/lume/src`](https://github.com/trycua/cua/tree/main/libs/lume/src).
  Only `Errors.swift` declares an unused `snapshotFailed`. Its other
  `snapshot` hits are VNC framebuffer captures and clipboard copies.
- cua-vmm [`runtime.rs`](https://github.com/trycua/cua/blob/main/libs/cua/crates/cua-vmm/src/runtime.rs)
  defines `suspend`, `resume`, `fork`, `checkpoint`, `delete_checkpoint`.
  [`lume/mod.rs`](https://github.com/trycua/cua/blob/main/libs/cua/crates/cua-vmm/src/lume/mod.rs):
  `suspend` calls `stop` ("Lume has no in-memory pause"), `fork` refuses a
  running source ("stop it before cloning"), and `checkpoint` is stop, fork,
  restart, documented as "clean, stopped-state clone, the same contract as
  cloud snapshots". Checkpoints are tagged `Checkpoint` in `lume/owned.rs` and
  deleted like any VM.
- Takeaway: a checkpoint modelled as a stopped sibling VM makes fork and delete
  trivial, at the price of a full stop on every checkpoint. Silo can keep the
  model but must save memory instead of stopping.

## Tart (read for insight only)

Sources: [`Suspend.swift`](https://github.com/openai/tart/blob/main/Sources/tart/Commands/Suspend.swift),
[`Run.swift`](https://github.com/openai/tart/blob/main/Sources/tart/Commands/Run.swift),
[`VM.swift`](https://github.com/openai/tart/blob/main/Sources/tart/VM.swift),
[`VMDirectory.swift`](https://github.com/openai/tart/blob/main/Sources/tart/VMDirectory.swift).

- State file: `state.vzvmsave` in the VM directory. `VMDirectory.state()` is
  Running if locked, Suspended if that file exists, else Stopped. The file is
  copied by `clone` (best effort) and removed by initialize/delete.
- Suspend: `tart suspend` sends SIGUSR1 to the `tart run` process. The handler
  calls `validateSaveRestoreSupport`, `pause`, `saveMachineStateTo`, then
  cancels the run task. Any error prints and exits 1; the VM is left as it was.
- Constraints: `--suspendable` flag. macOS guests only ("You can only suspend
  macOS VMs"); audio and entropy devices are dropped and only Mac keyboard and
  pointing devices are used; incompatible with `--no-trackpad`, `--no-keyboard`,
  `--no-pointer`; USB accessories optional (`--no-usb-accessories`). macOS 14+
  host, Apple Silicon only. A Suspended VM forces `suspendable = true` on the
  next run.
- Restore: in `run`, if the state file exists, `restoreMachineStateFrom`, then
  delete the file, then `start(resume: true)`. The delete comes after the
  restore, so a failed restore keeps the file and there is no cold-boot
  fallback in the code read: the user must delete the VM or its state by hand.
- Takeaway: the suspendable device set is a boot-time decision. Silo should
  build every macOS computer with a save-compatible configuration, since a VM
  started without it cannot be checkpointed while running.

## UTM (Apache-2.0)

Sources: [`UTMAppleVirtualMachine.swift`](https://github.com/utmapp/UTM/blob/main/Services/UTMAppleVirtualMachine.swift),
[`UTMAppleSnapshotBackend.swift`](https://github.com/utmapp/UTM/blob/main/Services/UTMAppleSnapshotBackend.swift),
[`UTMAppleDiskImage.swift`](https://github.com/utmapp/UTM/blob/main/Services/UTMAppleDiskImage.swift).

- Suspend (macOS 14, arm64): `validateSaveRestoreSupport` at VM creation; the
  error is kept in `snapshotUnsupportedError` and raised only when a save is
  requested. Save pauses, then `saveMachineStateTo`; restore requires a stopped
  or starting VM, sets state `restoring`, calls `restoreMachineStateFrom`, then
  resumes and deletes the saved state. On error the state returns to stopped.
- Named snapshots (macOS 27+ only): each disk gets a DiskImageKit ASIF overlay
  layer (`<image>.<name>.asif`); auxiliary storage (EFI variables or Mac aux)
  and the saved state are file copies suffixed with the snapshot id. A manifest
  tracks them; orphans are cleaned. Restore checks every disk first ("every
  drive is restored or none is"), swaps aux storage through `replaceItemAt` so
  a failure cannot lose the original, and removes the live saved state because
  it no longer matches the disks. Create and restore are refused while running;
  delete is allowed.
- Takeaway: disk, aux storage and memory state are one unit; restoring a disk
  must discard any memory state taken against another disk.

## VMPal 0.42 (proprietary, bundle strings only)

French strings in `/Applications/VMPal.app/Contents/Resources/fr.lproj/Localizable.strings`.
No VMs exist on this Mac, so `vmpal_snapshots` was not exercised.

- A snapshot keeps the disk and, when running, what is open. Snapshots are
  independent, each uses only the space of what changed after it plus the
  memory of a running VM. The panel shows exclusive size, which deleting frees.
- Taking one on a running VM pauses it "a few seconds". Each snapshot says
  whether it was taken running (revert resumes from that moment) or stopped
  (revert leaves it stopped). Reverting asks for confirmation and offers
  "take a snapshot and revert".
- Restore failure: "the saved session could not be restored, so the VM started
  from scratch"; also refuses to resume while the Mac is locked.
- GPU acceleration blocks saving state for Linux guests ("the GPU's state can't
  be saved"), the same device-set constraint as Tart.
- Storage format: not observable from strings; sizes suggest copy-on-write
  disk layers like UTM.

## Framework constraints (Apple Virtualization.framework)

| Fact | Consequence for Silo |
| --- | --- |
| `saveMachineStateTo(url:)` needs a paused VM, macOS 14+, Apple Silicon | Checkpoint a running computer as pause, save, then resume or stop |
| `validateSaveRestoreSupport()` on the configuration throws for unsupported devices | Run it at build time; record "memory checkpoints unavailable" and offer disk-only |
| Restore needs a stopped VM built from the same configuration | Store a config fingerprint per checkpoint; rebuild from the stored config, not the current one |
| After restore the VM is paused | Call `resume` and only then show the display as running |
| State files are encrypted to the Mac and may be rejected after a host OS update | Record host build; on rejection keep the disk, drop the memory state, cold boot with a notice (VMPal model) |
| State and disk must match | Restoring a disk invalidates memory states taken on a different disk (UTM) |
| Audio, USB and some graphics devices block save (Tart, VMPal) | Choose a save-compatible device set for every macOS computer |
| Machine identifier and MAC must be regenerated on fork | A fork gets a new identifier and MAC (Tart `clone` regenerates the MAC) |

## Silo mapping

| Operation | Approach |
| --- | --- |
| Checkpoint, running | validate, pause, save to `<checkpoint>/state.vzvmsave`, clone disk and aux storage (APFS `clonefile`), resume |
| Checkpoint, stopped | clone disk and aux storage only; mark disk-only |
| Restore | stopped VM from the checkpoint's stored config; restore disk and aux atomically; if memory state exists, `restoreMachineStateFrom` then resume; on failure cold boot with a notice and keep the disk |
| Fork | clone the checkpoint into a new computer with a new machine identifier and MAC; memory state kept only if the identity can match, otherwise disk-only |
| Delete | remove the checkpoint directory; clones are independent, so nothing else changes |

## Verified on this Mac

2026-10-10, macOS 26.5 (build 25F71), a signed Swift probe with Silo's exact
configuration (Mac graphics, virtio block, NAT, Mac keyboard and trackpad, USB
pointer, virtio sound to the host, entropy) on an APFS clone of a finished 4 CPU,
8 GiB macOS 26.6.2 computer, booted for 60 seconds:

| Step | Result |
| --- | --- |
| `validateSaveRestoreSupport` | Passes, audio device included |
| `pause` | 0.02 s |
| `saveMachineStateTo` | 2.1 s, 2.95 GB state file (the guest's used memory, not its 8 GiB), mode 0600 as created by the framework |
| `clonefile` of the 64 GiB sparse disk and the auxiliary storage while paused | 0.3 to 1.3 s |
| `stop`, then a new machine from the same configuration | The auxiliary storage is byte-identical after the stop |
| `restoreMachineStateFrom` with the paused-time clones swapped in | 3.5 s, machine left paused |
| `resume` | 0.4 s, still running 10 s later |

A first attempt failed with `VZErrorDomain` code 12, "failed to restore with
error invalid argument": the probe had not pinned the network device's MAC
address, so each configuration got a random one. The restored configuration has to
match the saved one exactly, which is why a memory state can never move to a fork
(new MAC and machine identifier) and why Silo always builds a computer with the MAC
in its record.
