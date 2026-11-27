# One position gives all of it

## The choice

An agent's part, 27 November 2026. The answer of 22 November listed four things the position
gives: limits, checkpoints, counting spend and a record. polarzero's words of 27 November, quoted
in `one-sentence`, put two others first: comparing what is reported from inside with what was seen
from outside, and holding a request at the boundary. Counting spend in dollars is now the work of
an extension (`spend-and-budgets-ship-as-an-extension`).

One case shows the difference. Claude Code keeps its own log of what it did, and its own rules for
what it may do. Both are files and processes inside the computer, and an agent with `sudo` can
change them. Silo's record and Silo's holds are outside, so the same agent cannot.

The options:

- The record, the check and the hold first (recommended). The answer names five things and says
  why nothing inside can do them. It costs a longer answer.
- The answer of 22 November. It costs the two things polarzero named first, and it still counts
  spend as the core's.

Recommended: the record, the check and the hold first.

## The earlier choice, 22 November 2026

An agent's part, 22 November 2026. A visitor asks why one product sets limits, takes checkpoints,
counts spending and keeps a record, when each exists as a separate tool.

The options:

- One position gives all four (recommended). Silo sits outside the computer, where the agent cannot
  touch it, and from there it can do the four. It costs tying the pitch to a virtual machine, which
  some people hear as heavy.
- Four features. Each is presented by itself. It costs a product that reads as a bundle, and no
  answer to "why not four tools".

Recommended: one position. It is true, it is short, and it explains what an agent program's own
rules cannot do from inside.

## Notes

The design record gives this with no mark, and this comparison:

| | A boundary the agent cannot touch | An account of what happened |
|---|---|---|
| Sandboxes and virtual machines for agents | yes | of the network at most |
| Agent programs | no: their rules live inside, next to the agent | of themselves only |
| Silo | yes | of everything in the computer, under any agent program |

An agent's part, 24 November 2026: the cell for sandboxes was "no". Some of them now log the
network decisions at their proxy, or the model requests with their tokens, so the cell says "of the
network at most". None found records the processes and file changes of the machine.
