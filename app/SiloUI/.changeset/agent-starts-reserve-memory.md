---
"silo-ui": patch
---

When several computers are started by agents at the same moment, Silo now counts the memory of starts it has already accepted, so a burst can no longer pass the free-memory safety floor together. A request for more memory than the device has free above the floor is refused with a plain message saying how much is free.
