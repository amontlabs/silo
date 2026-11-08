---
"silo-ui": minor
---

Silo can be the machine GitHub Actions jobs run on. The new `silo-runner` helper gives every job of a private repository a fresh Silo computer, deleted after the job (or stopped and kept when the job fails). `silo exec --stream` runs a command with no time limit, and `--stdin` hands it standard input. The `silo-action` composite action is removed.
