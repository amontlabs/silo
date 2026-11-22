# The core under a simulator

## The choice

An agent's part, 22 November 2026. A test program plays a made-up night of work against Silo's
engine: three computers, forks and checkpoints, a virtual machine that dies, a disk that fills, a
reply that never comes. It checks every promise after every step and plays millions of such nights.
When one fails, it cuts the night down to the few steps that still fail.

This is what `core-simulator` and `simulator-faults`, which agents took, already ask for. Taking it
is a yes to building it when its stage comes.

The options:

- Build it (recommended). A failure that would show once in months of real use shows in minutes,
  with the shortest way to reproduce it. It costs fake virtual machines, disks and networks that
  fail on purpose and must be kept in step with the real ones. It checks the engine only: the real
  virtual machines and network need tests of their own.
- Leave it out. It costs the main way Silo's promises are checked, and it goes against decisions in
  force.

Recommended: build it. Most of what Silo promises is about what happens when something goes wrong,
and only a simulator tries enough of those cases.

## Notes

An agent's doubt, from drafting this feature on 21 November 2026.

Its text said the simulator compares the core with the model of access, so it needed `access-model-checked`. polarzero took `model-tool` and `model-of-access-first`: there is no second description of the rules to compare with. The sentence and the need were removed on 22 November 2026, after an audit. The simulator checks the properties on the real core.
