---
"silo-ui": minor
---

Keeping a fork's changes no longer stops when both the fork and main changed the same thing. Silo lists each clash with what changed on each side and the answers that fit: use the fork's version, leave main's, or for a repository bring the fork's commits in as a branch named `silo/<fork name>` without merging anything. You can also leave out items that do not clash. The result now lists what was done with every item, says whether main had changed since the fork, and states what happened to databases (they are never combined). Undo takes a branch back along with everything else.
