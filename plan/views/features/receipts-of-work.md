# Receipts

## The choice

An agent's part, 22 November 2026. codex worked for 1 h 12 in a fork on "fix the flaky login test",
and says all tests pass.

The options:

- Build it (recommended). The receipt shows your words, the commands and their exit codes, the 38
  files changed and the one only in the fork, the hosts, the secret used, $6.40, and a gap of 2
  minutes, with codex's own summary beside it. It costs a large build that rests on most of the
  record: files, commands, model calls and which run wrote what.
- Leave it out. You read the agent's summary and the diff. It costs everything a diff does not show,
  such as packages, hosts, secrets and spend, and any check of the agent's claim.

Recommended: build it. polarzero called the receipt the main thing Silo gives, and it is how an hour of
agent work is judged in a minute.

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
