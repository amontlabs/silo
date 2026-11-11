---
"silo-ui": patch
---

If Silo's engine stops while a new computer is still starting, the next start now finishes the creation or removes the half-made computer, instead of refusing every command with "saved computer configuration does not match its managed runtime state". Other computers keep working meanwhile. A checkpoint that was finished just as the engine stopped is now kept, so the first start of a rebuilt computer after such a stop no longer fails with "An interrupted checkpoint could not be removed".
