---
"silo-ui": minor
---

A container computer has checkpoints, forks (`silo fork -n N` included), quiet forks, diff, keep and undo, as a Linux VM does. Its checkpoints save the disks only, also while it runs. A container keeps running when the engine restarts. A container computer shares the host's disk, so its size is reported, not a bound: when the host has less than 5 GiB free, Silo refuses to start, checkpoint or fork a container computer, and stops the running ones whose files are over their size, largest first, with the reason in their status.
