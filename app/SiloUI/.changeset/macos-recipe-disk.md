---
"silo-ui": patch
---

A checkpoint of a running macOS computer now has the computer write its disk out before it is copied, whatever kind of checkpoint it is. Before, the last files written, and part of a large extracted archive, could be missing from a computer forked from that checkpoint, and so from every computer forked from a recipe. Recipes built before this are rebuilt on the next use.
