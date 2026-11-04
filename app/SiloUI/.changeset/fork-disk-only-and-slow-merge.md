---
"silo-ui": patch
---

A fork now starts from the checkpoint's disks instead of resuming its memory, and the memory files a restore leaves behind are removed once no checkpoint uses them. Freeing checkpoint disk on a computer with large disks now waits for the merge to finish instead of failing and leaving the next checkpoint to fail.
