---
"silo-ui": patch
---

After Silo's engine stops unexpectedly (a crash, a force quit or an update) while computers keep running, creating, stopping and quitting no longer fail for several minutes with "The previous computer command is still finishing". The running computers no longer keep Silo's computer-command lock; a new engine takes it at once, and only waits if a command the old engine started is itself still running. This covers the `msb` command itself, not a helper it detaches on purpose (creating or restoring a computer leaves the computer's VM running by design). If Silo cannot record a command it starts, it stops that command and says so instead of running it unrecorded.
