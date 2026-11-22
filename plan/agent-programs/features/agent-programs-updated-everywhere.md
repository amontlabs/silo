# Agent programs updated in every computer

## The choice

An agent's part, 22 November 2026. Claude Code releases a new version. You have main, two forks and
a Linux computer. One action would update Claude Code in all four, after a checkpoint of each.

The options:

- Leave it out (recommended). Each program updates the way its vendor set it up; Claude Code, for
  one, updates itself. Checkpoints, taken on their own, give the way back if an update breaks
  something. It costs a stopped computer staying on the old version until its program next starts.
- Build it. One action updates every computer. It costs Silo running a command in all your computers
  at once, and handling a run that uses the program during the update, or a computer where the
  update fails.

Recommended: leave it out. The vendors' own updaters already do this, and a checkpoint taken on its
own already gives the way back.

## Notes

From the list of smaller features polarzero put on the roadmap on 21 November 2026. polarzero's
words on the list are in `what-ships-with-silo`.

## An agent's notes

- Silo runs each vendor's own updater inside each computer. It ships no agent program, as
  `what-silo-does-not-build` proposes.
- The checkpoint is the way back when an update breaks a program. Restoring main sends everyone in
  it back (`restore-main`), so it helps most right after the update.
- Bad paths to design: a run is using the program while it is updated. The update works in one
  computer and fails in another. A computer is paused or stopped when the action runs.
- No tool of an extension runs a command in one of your computers, and a task runs in a fork or a
  fresh computer, not in main. So this is part of Silo and not an extension, as things stand.
