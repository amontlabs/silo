# What does a restore do before it goes back

## For the owner

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
