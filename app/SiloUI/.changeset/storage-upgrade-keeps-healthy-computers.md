---
"silo-ui": patch
---

Moving your computers to the new storage format no longer stops computers that are already in it: only computers still in the old format are stopped, so a running computer keeps its memory and state even when another computer needs attention. Computers that were imported or forked but have not started yet are no longer flagged as needing attention, and Start works on them. After an update, the computers an update saved resume in the same launch instead of failing while the storage upgrade runs. The backup taken before the storage upgrade is now flushed to disk before it is trusted, so a power cut cannot leave an incomplete copy that Silo later relies on.
