# Checkpoints taken on their own

## The choice

An agent's part, 22 November 2026. Three agents work in main all afternoon. At 15:40 one of them
deletes the database it was migrating. Silo took checkpoints on its own every few minutes and at
each ask, so there is one from 15:38, with the database and the agents' conversations as they were.

The options:

- Build it (recommended). Checkpoints are taken while agents work, kept out of their reach and never
  changed. It costs disk on your Mac for what changes after each checkpoint, which thinning keeps
  down. A checkpoint holds the disk only, so the programs that were running start again after a
  restore.
- Leave it out. Going back relies on Git and on copies you make yourself. It costs everything Git
  does not hold: the database, installed packages, settings, and work not yet committed.

Recommended: build it. Restore, forks from a moment and putting back one run all start from these
checkpoints, and a checkpoint nobody has to remember is the only kind that exists before the
mistake.
