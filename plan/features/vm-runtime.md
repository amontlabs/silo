## Proposed, not confirmed by the owner

### For the owner to settle

- Does the VM runtime link into the engine binary, or run as a separate process behind a driver?
  - Recommendation: A separate process, so an engine crash or update need not stop every computer; the loop has to be safe to leave running.

### For whoever builds it

- Detail: The first driver may run the runtime's commands.
- Risk: The VM runtime could be wound down, relicensed or drop a feature Silo needs, so its source stays vendored and buildable, and Silo keeps the means to patch or fork it.
- Rule: Each patch Silo carries on the VM runtime has an upstream issue or pull request aimed at removing it.
- Risk: The VM runtime moves fast and is not stable, so its commits and its replies to issues are watched.
- Docs: For each driver, the docs say whether Silo depends on the runtime and waits for what it lacks, or replaces it and follows every change, and why.
- Test: Run a build and an I/O-heavy parallel test suite in a computer and on a plain Linux machine, and compare the times; an 8x slowdown is the failure to avoid.
- Rule: A shared folder or a filesystem layer between host and guest is measured the same way before it is adopted, because it can serialise I/O.
- Test: Stress the runtime with many computers on one Mac, and report upstream what breaks.
- To find out: Build a proof of concept against Apple's virtualization framework to settle which process hosts a VM.
- To find out: How does MicroSandbox compare with stock Firecracker on Linux hosts for snapshots, fork with memory, suspend and resume, and start time? Record the gaps, and check the choice against the microvm-2026 overview.
- To find out: Does the VM runtime support memory ballooning or hotplug, so that idle computers give memory back to the Mac?

## From the old code

The old code had it partly built, about 16,600 lines, and it needs reworking.

Lessons:
- Each MicroSandbox upgrade found regressions (missing runtime_instance_id, restore with spare CPU or memory, relay disconnect during a bulk write); keep upgrade checks.
- msb needs the library-validation exception to load libkrunfw without an Apple Team ID.
- A nested source checkout needs its own Git repository, or patches silently skip paths.
- Upstream restore always starts the guest and create always boots; stopped create and fork needed patches.
- Snapshot capture folders were never deleted upstream, so deleting snapshots freed no disk.
- Compaction upstream kept the old layers, using more disk instead of less.
- Upstream 0.7.14 made blocked connections complete with a 403; Silo refuses them so nothing is ever sent to an unlisted address.
- The VM is the wall, not the guest account: the silo working account has passwordless sudo.

Where it lived, at the tag legacy-2026-11-16:
- `docs/SiloUI-RUNTIME-PACKAGING.md`
- `app/SiloUI/scripts/prepare-microsandbox-runtime.mjs`
- `app/SiloUI/scripts/microsandbox-runtime.mjs`
- `app/SiloUI/src-tauri/vendor`
- `app/SiloUI/patches`
- `app/SiloUI/runtime-inputs.json`
- `docs/SiloUI-RUNTIME-BACKUP-FINDINGS.md`
- `docs/v1/upstream/EXPLAINED.md`
- `docs/direction/MSB-0.7.15-MIGRATION.md`
- `docs/THREAT-MODEL.md#L70`
- `SECURITY.md`
- `docs/evals/results/2026-11-03-0dae8805.md`

Calls: `npm run runtime:prepare`.
