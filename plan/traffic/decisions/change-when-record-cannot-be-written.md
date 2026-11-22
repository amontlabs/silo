# What happens to a change of traffic when the record cannot be written

## For the owner

An agent's part, 22 November 2026. Main runs a build that writes 40,000 files, and the record falls
behind until the room Silo keeps for its own decisions is full. Claude Code in main gets a reply
that asks for `git push --force origin main`, which a rule denies.

The options:

- The reply waits (recommended). The reply stays at the boundary until the record has room, then the
  denial is made and the reply goes on to Claude Code. The push never runs, and every change Silo
  made is in the record. It costs Claude Code a wait. If Claude Code gives up first, it gets an
  error and asks the model again, and the reply it never received still counts in spend and in your
  budgets, since the provider charged for it.
- The reply fails at once. Claude Code gets an error as soon as the room is full, and asks the model
  again. It costs a second reply paid each time, even when the record would have caught up a second
  later.

Recommended: the reply waits. Waiting gives the record a chance to catch up, and when it does not in
time, the result is the same error and the same spend as failing at once.

## Notes

To find out. It bears on `no-silent-change-to-traffic`, and on the record's principle that enforcing does not depend on recording (`record`).
