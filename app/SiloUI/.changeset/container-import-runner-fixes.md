---
"silo-ui": patch
---

An imported container computer now starts with its network blocked, and keeps it, like an imported virtual machine. Container exports no longer carry the exporting host's SELinux labels. The runner checks again before each job that the repository is still private, and refuses the job when it is not or the check fails. A second Ctrl-C ends a slow runner shutdown at once. Stopping a paused container computer works cleanly instead of ending in a force kill, and a second checkpoint of a running container computer now says a checkpoint is already in progress.
