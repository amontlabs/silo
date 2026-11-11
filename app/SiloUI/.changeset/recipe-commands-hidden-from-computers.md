---
"silo-ui": patch
---

A computer's agent no longer sees the command lines of a recipe. Reading a recipe from inside a computer (including the recipe the computer was built from, or one it got by forking a recipe's build) now leaves out the setup commands, each service's start command and the migrate and after steps of the merge hooks, and says they are hidden. Package lists, files, service names and the rest stay visible, and you still see everything. Anything a setup command writes ends up on every computer built from the recipe, so keep tokens in secrets, never in setup lines.
