---
"silo-ui": patch
---

A computer's agent can now read in full only the recipe its computer was built from. For the other saved recipes it sees their names, so it can still pick one, but not their setup lines, which may hold tokens you pasted. Undoing a merge now also takes back the packages and settings files the merge added to the computer's recipe and to your saved recipe, and leaves everything that was already there.
