## Proposed

- Rule: Silo announces a change before it makes one on the user's behalf in their computers or repositories, such as an upgrade pull request.
- One action updates the guest kernel and base image of every existing computer.
- Silo shows which computers still run an old kernel or base image.
- An upgrade to a recipe or to Silo's own files in a computer can come as a migration prompt that an agent runs in a fork, merged like any other change.

## From the old code

The old code had it partly built, about 3,000 lines, and it needs reworking.

Lessons:
- A kill during create or rebuild left saved configuration and runtime state disagreeing; recovery must be designed in, not patched (R10).
- Quit must not wait for a whole rebuild (T15).
- Owned disk size must match exactly at start; layered disks need the whole owned-volumes folder moved.
- Copy the home by tar inside the guest and msb cp, not exec output (10 MB/s).
- Homes over about 45 % of the root disk are refused, because the home travels as a tar file.
- The old computer never runs again after the workspace disk is cloned, so nothing it does can be missed.
- A rebuild is refused when the home is over 45 % of the root disk, because the home is packed inside the computer before it moves.
- Writes to the home in the seconds between packing and stopping are only in <name>-undo.

Where it lived, at the tag legacy-2026-11-16:
- `docs/direction/evidence/T15.txt`
- `docs/direction/evidence/V.txt`
- `docs/direction/evidence/S2.md`
- `app/SiloUI/src-tauri/engine/src/rebuild`
- `app/SiloUI/cli/src/cli.rs#L1743`
- `docs/direction/checks/t15_rebuild_check.sh`
- `app/SiloUI/src-tauri/engine/src/rebuild/mod.rs`
- `app/SiloUI/src-tauri/engine/src/rebuild/swap.rs`
- `app/SiloUI/src-tauri/engine/src/rebuild/journal.rs`
- `docs/ROADMAP.md#L236`
- `docs/direction/OWNER_REVIEW.md#L46`
- `docs/ROADMAP.md#L467`

Calls: `computers.rebuild`, `silo rebuild`, `silo rebuild --force`.
