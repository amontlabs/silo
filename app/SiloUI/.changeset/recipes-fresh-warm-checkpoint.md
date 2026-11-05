---
"silo-ui": minor
---

A fork of main now starts from its warm checkpoint only if it is under five minutes old (`warm_max_age` in `.silo/silo.yaml`) and says which checkpoint it used. Recipe builds no longer collide, keep the previous build until the new one succeeds, and run their services under the service's own name.
