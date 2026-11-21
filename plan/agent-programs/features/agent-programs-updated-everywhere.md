# Agent programs updated in every computer

From the list of smaller features the owner put on the roadmap on 21 November 2026. The owner's
words on the list are in `silos-own-extensions`.

## An agent's notes

- Silo runs each vendor's own updater inside each computer. It ships no agent program, as
  `what-silo-does-not-build` proposes.
- The checkpoint is the way back when an update breaks a program. Restoring main sends everyone in
  it back (`restore-main`), so it helps most right after the update.
- Bad paths to design: a run is using the program while it is updated. The update works in one
  computer and fails in another. A computer is paused or stopped when the action runs.
- No tool of an extension runs a command in one of your computers, and a task runs in a fork or a
  fresh computer, not in main. So this is part of Silo and not an extension, as things stand.
