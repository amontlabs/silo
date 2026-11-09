---
"silo-ui": minor
---

A container computer has checkpoints, forks (`silo fork -n N` included), quiet forks, diff, keep and undo, as a Linux VM does. Its checkpoints save the disks only, also while it runs. A container keeps running when the engine restarts, and its disk size is held: past it, the computer's files turn read-only until you free space and start it again.
