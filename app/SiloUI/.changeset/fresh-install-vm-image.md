---
"silo-ui": patch
---

Fix first launch on a new installation failing to prepare the VM image because the runtime's storage link could not be created. An installation already stuck this way recovers on the next attempt.
