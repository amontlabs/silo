# Two runs writing in the same checkout

## For the owner

An agent's part, 22 November 2026. Two runs in main both edit `src/app.ts` in the checkout of your
site within the same ten minutes. Silo alerts you and offers to make a fork.

The options:

- Build it (recommended). You learn of the collision when it starts and not from a broken build. It
  costs alerts that can be wrong, since which run wrote a file is an opinion. With several agents in
  one main, two runs in the same checkout is normal, so the alert is useful only for the same files,
  which is still to settle. A fork also copies both runs, and what becomes of them is not decided.
- Leave it out. The map of who is writing what shows the same thing when you look at it. It costs
  the moment: you find out later.

Recommended: build it, in the stage "later", and for the same files only. Silo alerts and offers,
and it moves no run and gives no work to anyone.

## Notes

From the list of smaller features the owner put on the roadmap on 21 November 2026. The owner's
words on the list are in `what-ships-with-silo`.

A checkout is the working copy of a repository in one folder.

## An agent's notes

- Which run wrote a path is an opinion (`which-run-did-it`). So this is a precaution, and it alerts.
  Making a fork is granting, so only you do it: the alert offers it.
- `hot-files`, taken, says Silo ships the map of who writes what and "does not build the part that
  splits work between agents". The offer makes a fork. It moves no run into the fork and gives no
  work to anyone. A feature that moved a run would go against `hot-files`.
- `several-agents-in-main`, taken, makes several agents in one main the normal case. To settle:
  whether the alert is for the same checkout or only for the same paths.
- A fork copies the whole computer, both runs included. What becomes of them is
  `agent-programs-conversation-in-a-fork`.
- An extension that reads the record and alerts could do this. The fork stays yours to make.
