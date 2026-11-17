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
