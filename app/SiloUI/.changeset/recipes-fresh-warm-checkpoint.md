---
"silo-ui": minor
---

A fork of main now starts from its warm checkpoint only if the engine can tell main has not changed since it was taken (the engine tracks a per-computer generation that commands, file writes, keep, restore and similar operations move) and it is under a minute old (`warm_max_age` in `.silo/silo.yaml`; changes made over a plain `ssh` session are not seen), and says which checkpoint it used. Recipe builds no longer collide, keep the previous build until the new one succeeds, and run their services under the service's own name.
