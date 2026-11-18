## Proposed

- Property: A checkpoint is stored where the agent in the computer cannot reach it.
- Rule: A restore takes a lock on its computer and writes an audit event.
- Risk: A restore to a checkpoint taken during an agent's tool call can repeat an effect outside the computer, so the task restarts from a checkpoint taken before the call.
- Test: An agent destroys a database, or deletes a repository and its history, in a fork; restoring the checkpoint or dropping the fork brings it back, identical to the original.
- Property: A checkpoint, like an image, is identified by a content hash and can be verified against it.
- A past state is read from the base snapshot plus its deltas, with a cache, and never by replaying the whole log.
- Restores of a computer are rate-limited.
- The docs present a checkpoint as a point to restore to and say it is not a backup; the word stays "checkpoint" (`git-words`).
- The docs describe restoring a checkpoint separately from recovery after a crash.

## Open

- Which checkpoints does Silo take by itself: before an agent changes its own workflow or recipe, before a long batch, before a model switch or a compaction, at intervals in a long run?
  - Suggested: Start with intervals in a long run, which the engine can do alone; the other triggers depend on what the agent program lets Silo see.
- Are checkpoint and fork disks left to the VM runtime's snapshots, as in the old code, or stored by Silo as deltas against a base, older ones compressed, or as content-addressed chunks?
  - Suggested: Leave them to the VM runtime for the starter, and measure how much identical data is stored twice before Silo takes on a format of its own.
- Do Linux computers use btrfs inside the guest for rollback, as first intended?
  - Suggested: Not as the way checkpoints are made, because a checkpoint must sit where the agent cannot reach it and work the same for macOS computers.
- Does Silo use Jujutsu as a library to checkpoint the code inside a computer, so that no Git is needed there?
  - Suggested: No, because a checkpoint already holds the code with the rest of the disk, and Jujutsu is a Rust library beside a Zig engine (`engine-in-zig`).
- To find out: What time budget for taking and for resuming a checkpoint can Silo hold, checked against Firecracker's reported speed?

## From the old code

The old code had it built and driven end to end, about 8,000 lines, and it needs reworking.

Lessons:
- An interrupted checkpoint could block the next start until its dependencies were released (R10).
- A computer's agent checkpointing itself always makes a manual checkpoint so retention never removes the user's automatic and step checkpoints.
- Restore first saves a recovery checkpoint and leaves the computer stopped.
- Forking an earlier checkpoint of itself asks the user: it can hold access since taken away.
- Snapshot data and its graph belong to MicroSandbox; Silo keeps only names, intent and references, and removes without --force so upstream guards still apply.
- A snapshot something still needs (a fork not started yet, an import, a restore's recovery point) must be pinned by Silo records.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/src-tauri/silod/src/methods/checkpoints.rs`
- `app/SiloUI/api/src/methods/checkpoints.rs`
- `docs/direction/evidence/F.txt`
- `app/SiloUI/src-tauri/silod/src/methods/checkpoints/tests.rs`
- `app/SiloUI/cli/src/ops.rs`
- `docs/SiloUI-CHECKPOINTS-PLAN.md`
- `docs/SiloUI-POLICY.md#method-by-method`
- `docs/product/JOURNEYS.md#L57`
- `app/SiloUI/src-tauri/engine/src/runtime/checkpoints.rs`
- `app/SiloUI/src-tauri/engine/src/checkpoints/mod.rs`
- `app/SiloUI/src-tauri/engine/src/runtime/checkpoints/native.rs`
- `app/SiloUI/src/features/application/components/checkpoint-panel.tsx`

Calls: `checkpoints.create`, `checkpoints.restore`, `checkpoints.delete`, `checkpoints.abandonRestore`, `checkpoints.usage`, `silo checkpoint`, `silo checkpoints`, `silo restore`.
