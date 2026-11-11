---
"silo-ui": patch
---

After Silo's engine stops unexpectedly (a crash, a force quit or an update) while computers keep running, creating, stopping and quitting no longer fail for several minutes with "The previous computer command is still finishing". The running computers no longer keep Silo's computer-command lock; a new engine takes it at once, and only waits if a command the old engine started is itself still running.
