## Proposed

- An agent's memory is the files of its computer, which the user can read and diff. The conversation is disposable, and Silo keeps none.
- A computer exists once. Another device or person uses it live or forks it; one computer is never synced into two copies that diverge.
- A named checkpoint never changes once it is saved.
- When an agent left alone degrades main's setup, the fix is a diff of main against an older checkpoint, then a restore.

## Open

- Where does the agent do a task?
  - In main, forking when asked. A user may then never fork.
  - In main, forking on its own.
  - Always in a fork, so main changes only by merge. Every small edit then needs a merge.
- Does a user have one main, or several (one per project, or work and personal)? Several keep one client's secrets away from another's; one lets the agent see everything from one place.
