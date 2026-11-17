## Notes

- The fork takes main's name, SSH alias, ports and access; the old main is kept as a checkpoint.
- macOS: re-apply main's identity after the swap. Sign-ins made in the fork do not carry over.

## From the old code

The old code had it built, about 3,200 lines, and it needs reworking.

Lessons:
- The fork must be as it was when the agent asked; a moved change generation refuses and asks again.
- From inside a computer it always asks, since it restarts the agent's own computer.
- Tell sandboxes apart by the identity they carry (silo.machine-id), never by name, because a crash can leave names half moved.
- Every step must be idempotent with a revert, so engine start can roll forward or back.
- Lineage after a replace is read from the journal, not rewritten into the stores.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/keep.rs`
- `app/SiloUI/src-tauri/silod/src/methods/checkpoints/keep.rs`
- `docs/product/JOURNEYS.md#L47`
- `docs/SiloUI-CHANGES.md#replacing-main-with-a-fork-silo-replace-main-fork`
- `app/SiloUI/cli/src/cli.rs`
- `docs/SiloUI-CHANGES.md#L309`
- `app/SiloUI/src-tauri/engine/src/changes/promote/mod.rs`
- `app/SiloUI/src-tauri/engine/src/changes/promote/linux.rs`
- `app/SiloUI/src-tauri/engine/src/changes/promote/journal.rs`
- `app/SiloUI/src-tauri/engine/src/changes/promote/lineage.rs`

Calls: `changes.promote`, `silo replace-main`.
