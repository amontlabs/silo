# Where is a checkpoint stored

## For the owner

An agent's part, 22 November 2026. An agent in main runs `sudo rm -rf /`, by mistake or on purpose.
It has full access inside main.

The options:

- Outside the computer, never changed (recommended). The agent can wreck main but cannot touch a
  single checkpoint, so you restore main to the moment before. It costs disk on your Mac:
  checkpoints sit on the same drive as the computer, and cannot move to an external drive without a
  full copy.
- Inside the computer, as snapshots of its own file system. Each computer keeps its own history, and
  an agent with full access can read, change or delete it. It costs the way back exactly when an
  agent goes wrong.

Recommended: outside the computer, never changed. The agent is treated as hostile, so the way back
has to be out of its reach.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **A checkpoint names its place in the record.** Represent a checkpoint as a position in an
  append-only log of changes. A checkpoint of a computer's disk also carries the record position at
  which it was taken, so "what happened since this checkpoint" is a plain query.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/client/state/journal.zig#L5))
