# Receipts

## The choice

An agent's part, 28 November 2026. An agent finishes work in a fork and says all tests pass and it
only touched files in the repository.

The options:

- Build it in MVP (recommended). The receipt puts the outside record beside the agent's own summary:
  the model calls and tool calls read at the boundary, the files created, changed or deleted between
  checkpoints, the hosts reached and bytes sent, and any gap, shown in the Swift Mac app (`mac-app`)
  and in `silo`, with copy and export controls so you can hand a full receipt with its relevant
  record lines and file diffs to any agent for analysis. Later features add processes, spend, tasks
  and screen recordings to the same receipt. It costs building the receipt and export formatter in
  Zig in MVP.
- Leave it out until after MVP. You read raw JSON lines from `silo` and inspect diffs by hand. It
  costs the clearest way to compare what an agent claimed with what Silo saw.

Recommended: build it in MVP. The receipt is how you verify agent work at a glance without taking
the agent's word for it.

## Notes

An agent's doubt, from drafting this feature on 21 November 2026.

It is large: its text carries the whole list of `what-a-receipt-holds`, which has no verdict. It could be cut into a first receipt and the rest.

## A first receipt, in the first build

An agent's part, 24 November 2026, from [the comparison of first builds](../../../research/2026-11-24-first-build-and-the-record/README.md). The doubt above, made concrete. A first receipt holds
only what the first build's record has: what you asked, the files that changed in and outside the
repository, the commands with the exit code the agent program sent to its model (a report read from
the traffic), the hosts reached and how much was sent, the conversation's spend, and the agent's
own last words beside them. It says what it does not have yet: processes, and anything a rule
decided. On a real night it catches the two false claims #371 caught, «All tests pass.» against
an exit 1 and «I only changed files in the repo.» against files deleted outside it. Proposed: a
new feature in `views`, split from this one, read from the `silo` command.
