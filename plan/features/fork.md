## Proposed

- What a fork is
  - Property: A fork's life ends in exactly one outcome, merged or dropped, and the outcome is reported.
  - Rule: A fork reports to its parent, and forks have no way to message their siblings.
  - An agent working in a fork can fork that fork the same way, to explore a side path.
  - Finished forks are dropped in one step, so dead forks do not pile up.
- Storage
  - Property: A fork shares its parent's disk at block level and stores only what changes afterwards, so a first write to a large file does not copy the whole file.
  - Property: A computer is paused or forked only with its whole virtual machine paused, so nothing is caught half done.
  - Property: What one fork writes to a build or package cache never appears in another fork or in its parent.
  - Test: A computer running a memory-mapped database is forked and benchmarked, using Reth's database library, which is reported to fail on OverlayFS; the database works in the fork.
  - Where a computer's storage would copy whole files on write, Silo detects a database on it and warns.
  - Risk: fuse-overlayfs was rewritten in Rust shortly before May 2026, so its maturity is checked before Silo uses it.
- Agents in a fork
  - Property: An agent session's durable state (session ids, resume files) survives a fork, a checkpoint restore and an engine restart.
  - Test: A fork is made from a computer with a live agent session; the resume flag of each supported agent program works inside the fork.
  - Test: Two agents that would collide in one checkout, one pushing and one switching branch, run in two forks; neither affects the other.
  - Test: An agent loop without permission prompts runs in a fork for hours, killed and restarted at random; work continues from disk, and dropping the fork leaves nothing behind.
- Speed
  - Test: The first build in a fresh fork is timed against the same build in main on sample Rust, Zig and TypeScript projects; the fork's build is incremental.
  - Test: Start, stop and fork times of a Linux and a macOS computer are measured on each device and published per device.
  - The limits of the fork budget (time, memory, disk) are configurable per device.
  - Test: A computer holding a 6.5 GB repository (3.9 GB of tests, a 1.5 GB git folder, 1 GB of build output) is forked; time and disk use stay within budget.
- Other
  - Dropping a fork can first bring chosen files back to its parent, such as its notes, and discards everything else.
  - A fork can carry labels (text, number or yes-or-no values) that every event of the fork carries, so forks can be filtered by them.
  - Risk: A fork starts with no secrets and read-only GitHub but still carries every file of its source, including other projects' data held in main.

## Open

- Which fork time does Silo commit to: 3 s to a shell as the old target says, or under 1 s as JJHub claims (800 ms on bare metal)?
  - Suggested: 3 s to a shell first: the old app ranged from 1.2 s to 21 s by what was counted, and a user left another tool at 10 s.
- To find out: Measure disk I/O during a fork, a build and a bulk delete, early, on a Mac's internal disk and on a Linux host with a standard NVMe drive.
- To find out: Measure fork time and memory with many forks open, to say whether one fork per subagent is practical.

## Notes

- Old targets: a Linux fork of a running computer ready for a shell in 3 s or less, using 1.5 GB or less, with disk fully given back after 50 fork-and-drop cycles on 16 GB and 32 GB Macs. Measured on the old app: 1.24 to 1.57 s for the first fork, 0.29 to 0.42 s for later ones; macOS forks are disk clones.

## From the old code

The old code had it partly built, about 4,000 lines, and it needs reworking.

Lessons:
- A fork must drop the source's Silo-tools token and get its own endpoint, or it answers as its parent (T5b S3).
- A fork may get only what the caller already has; forking an earlier checkpoint of itself asks, since it can hold what the user since took away.
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
