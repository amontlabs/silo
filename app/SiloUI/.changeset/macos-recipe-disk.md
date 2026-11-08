---
"silo-ui": patch
---

A macOS recipe build now writes the computer's disk out before it saves the build. Before, the last files a setup command wrote, and part of a large extracted archive, could be missing from every computer forked from the recipe. Recipes built before this are rebuilt on the next use.
