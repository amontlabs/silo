# Space the engine keeps for itself

## The choice

An agent's part, 24 November 2026. Main's disk image, its checkpoints and the engine's own files
are on the same APFS volume. An agent fills main's disk with build output overnight. After a clone,
overwriting a shared block needs new space, so main's writes fail, and then the engine's next write
fails too. The engine stops, and nothing can change until it is back (`when-the-engine-stops`): no
restore and no removing a checkpoint, which is what you need.

The options:

- A reserve only the engine may delete, and checkpoints that stop before the volume is full
  (recommended). It costs a few gigabytes held back on your Mac.
- Rely on `low-disk`, which pauses computers at two levels. It costs the engine's own writes, which
  nothing keeps room for.
- Nothing. It costs a Mac where the first full disk ends every way back.

Recommended: a reserve only the engine may delete. The size of the reserve and the level are to
find out on a Mac.

## Notes

Agent, 24 November 2026. From draft #352 (files 02 and 07, "Main's disk on a full Mac"). Whether an
overwrite on APFS needs new space when no clone shares the block is a Mac test of that draft.

## Where the drafts of 25 November disagree (integration)

An agent's part, 25 November 2026, written while merging every draft into one branch (`research/2026-11-25-integration-2/`).

#378, section 4: on the Mac's APFS, removing a checkpoint may free nothing while a local Time Machine snapshot exists, and the private size APFS reports then reads 0. The level where Silo pauses computers must not count on space freed by removing checkpoints in the same minute.
