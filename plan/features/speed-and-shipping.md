## Proposed

- Fork, start, diff, merge and an idle computer's memory each have a budget, written before their code as a p99 and a worst case.
- A task's result is usable first: its diff can be read at once. Checks and reviews run after it and can be stopped.
- A risky feature ships only when it works on every setup Silo supports. Until then Silo ships without it and says it is coming.
- Before 1.0, a changed interface replaces the old one at once, with an automatic migration. Two versions never run side by side.
- Once the starter works, the agents that build Silo run in Silo computers, and what breaks for them is fixed before other work.

## Open

- Are forking, diffing, merging and opening the app at least as fast as creating a git worktree or starting a container? A fork boots a whole virtual machine, so matching a container may need memory checkpoints early.
- What does the first release need?
  - Rough, with a demo. It reaches users sooner.
  - Only features that work end to end, the rest cut and listed. Fewer features to support.
- Does Silo ship only what the people and agents building it use themselves? Team features would then wait.
- Does a view of a browser running inside a computer move into the starter? People ask for it first; ports already cover development servers.
- Until the first release, is migration code written for older saved data? Before the starter there is no data worth keeping; once someone lives in main, there is.
