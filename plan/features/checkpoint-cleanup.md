## Proposed, not confirmed by the owner

### For whoever builds it

- Detail: A prune command keeps what replaying history needs and drops the detail of transcripts.
- To find out: How much disk is used for how much kept history (space amplification)?

## Notes

- The old engine never pruned: chained snapshots needed forcing. Prune on drop and on checkpoint removal, keep a retention count for checkpoints taken before merges, and check the disk really comes back.

## From the old code

The old code had it partly built, about 700 lines, and it needs reworking.

Lessons:
- Each MicroSandbox snapshot holds its whole layer closure, so deleting old snapshots frees nothing until the disk's layers are compacted (msb modify --compact).
- Export a capture without --with-parents: it writes a layer twice and snapshot load refuses it.
- The merge of layers and the removal are one unit: a failed merge removes nothing.
- Deleting snapshots frees nothing until layers are merged; never remove the newest, manual or depended-on ones.
- A fresh snapshot group per export plus import created duplicate parents and broke later exports.

Where it lived, at the tag legacy-2026-11-16:
- `app/SiloUI/src-tauri/engine/src/runtime/checkpoints/reclaim.rs`
- `app/SiloUI/src-tauri/engine/src/checkpoints/retention.rs`
- `docs/SiloUI-CHECKPOINTS-PLAN.md#L321`
- `docs/v1/upstream/EXPLAINED.md`
- `docs/research/silo-snapshot-lineage-groups-2026-09-26.md`
