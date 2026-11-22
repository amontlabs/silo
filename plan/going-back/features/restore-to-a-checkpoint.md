# Restore

## The choice

An agent's part, 22 November 2026. In a fork where one agent works, a migration goes wrong at 15:40.
You restore the fork to 15:38. Silo first saves 15:40 as a checkpoint, then the disk and the agent's
conversation go back to 15:38.

The options:

- Build it (recommended). One step puts a computer back to a checkpoint, main included, while the
  record, the rules and what was spent stay as they are now. It costs the programs that were
  running, which end; in main every agent goes back, not only the one that went wrong.
- Leave it out. You go back only by forking from a checkpoint and making the fork main. It costs
  steps each time, and on a Mac a second macOS computer to run the fork, which Apple allows only
  once beside main.

Recommended: build it. When one agent works alone in a computer, putting that computer back is the
direct answer, and forks stay for when others are at work in it.
