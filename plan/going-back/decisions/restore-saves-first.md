# What does a restore do before it goes back

## The choice

An agent's part, 22 November 2026. At 16:00 you restore main to 15:38 to undo a bad migration. Then
you remember that a second agent had finished a fix at 15:55 that you wanted.

The options:

- Save first (recommended). Silo took a checkpoint at 16:00, before the restore, so you fork from it
  and bring the fix back. It costs the disk of that one checkpoint until thinning removes it, and a
  moment before each restore.
- Restore at once. The state at 16:00 is gone, unless a checkpoint happened to be taken just before.
  It costs the work done since the checkpoint you went back to.

Recommended: save first. A restore that cannot be undone makes every restore a gamble, and one more
checkpoint takes only the space of what changes after it.

## A restore that a crash cannot leave half done (24 November 2026)

An agent's part, 24 November 2026, from the prototype in `research/2026-11-24-checkpoint-engine/`
(Linux).

The prototype's restore takes the checkpoint of where you were, builds the computer's folder anew
from clones of the checkpoint's files in `live.new`, and swaps the two folders in one call
(`renameat2` with `RENAME_EXCHANGE` on Linux, `renamex_np` with `RENAME_SWAP` on macOS). A crash at
any step leaves either the computer as it was, with one more checkpoint, or the computer restored;
never a disk from one moment with the auxiliary storage of another, which a macOS computer may
refuse to boot. Two renames in place of the swap failed the crash test at a crash between them.
