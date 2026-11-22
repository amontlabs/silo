# When the part of Silo that runs one computer finds a bug in itself, what does it do

## For the owner

An agent's part, 22 November 2026. An agent is in the middle of a task in a fork. The part of Silo
that runs that fork, its helper, finds its own state impossible, which means a bug in Silo.

The options:

- Stop that one computer (recommended). The helper stops and the fork stops with it. Your other
  computers keep running. It costs the agent's running work, and at worst what happened in that fork
  since its last checkpoint.
- Keep running, note it. The fork goes on and the error is logged, so no work is lost. It costs
  trust in its rules, which may no longer be applied correctly.

Recommended: stop that one computer. A computer whose limits may be broken is worse than a stopped
one, and checkpoints keep the loss small.
