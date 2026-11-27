# One path from a checkpoint

## The choice

An agent's part, 24 November 2026. In the morning `silo changes 02:40 03:10` shows that Codex deleted
`~/work/app/src`. You run `silo put-back 02:40 ~/work/app/src`, and the folder is back while Claude
Code goes on in `~/work/site`.

The options:

- Build it with checkpoints (recommended). It costs a reader of checkpoints that can copy a path
  out, which file changes from outside already need.
- Wait for forks. It costs a whole restore each time, until then.

Recommended: build it with checkpoints.
