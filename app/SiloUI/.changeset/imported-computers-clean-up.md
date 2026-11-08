---
"silo-ui": patch
---

Dropping an imported computer now frees all of its disk, and Silo clears the data of earlier imported computers that were dropped but left it behind. An export's free-space check counts only what that export writes, so it no longer refuses a small export because of other computers' checkpoints. The Silo certificate authority every computer has is no longer named as a login when you share a computer, and the files D-Bus rewrites at every login no longer show up as changes when you keep a computer into another.
