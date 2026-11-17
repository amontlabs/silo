## Notes

- Old targets: a Linux fork of a running computer ready for a shell in 3 s or less, using 1.5 GB or less, with disk fully given back after 50 fork-and-drop cycles on 16 GB and 32 GB Macs. Measured on the old app: 1.24 to 1.57 s for the first fork, 0.29 to 0.42 s for later ones; macOS forks are disk clones.

## From the old code

The old code had it partly built, about 4,000 lines, and it needs reworking.

Lessons:
- A fork must drop the source's Silo-tools token and get its own endpoint, or it answers as its parent (T5b S3).
- A fork may get only what the caller already has; forking an earlier checkpoint of itself asks, since it can hold what the owner since took away.
- A fork always cold-boots the disks, never memory, so nothing of the source keeps running.
- The engine does not copy published ports; the CLI republishes them, each on its own device port.
- Early plan said forks stay stopped until an explicit Start; the CLI later starts them by default.
- Upstream restore always starts the guest, so a stopped fork is a Silo record pointing at an immutable snapshot until Start.
- The pending restore is consumed only after a durable successful start; a failed first start must not cold-boot or double-start.
- Other tools have placeholder secrets, allowlists and microVMs; only the whole-machine loop is distinctive, so it is what comes first.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/api/src/methods/checkpoints.rs`
- `app/SiloUI/src-tauri/silod/src/methods/checkpoints.rs`
- `docs/product/JOURNEYS.md#L38`
- `docs/direction/evidence/T5b.txt`
- `docs/SiloUI-POLICY.md`
- `app/SiloUI/cli/src/ops.rs`
- `docs/SiloUI-RECIPES.md#silo-up`
- `docs/direction/evidence/T5a.txt`
- `docs/product/JOURNEYS.md`
- `app/SiloUI/src/features/application/components/fork-popover.tsx`
- `app/SiloUI/src/features/application/components/fork-tree.tsx`
- `app/SiloUI/src/features/application/components/fork-tree-layout.ts`

Calls: `checkpoints.fork`, `silo fork`, `computers.tree`, `silo diff`, `silo merge`, `silo drop`.
