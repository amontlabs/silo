# A checkpoint that leaves paths out

## The choice

An agent's part, 22 November 2026. A rule leaves `node_modules` and `target` out of main's
checkpoints to save disk. You restore main to yesterday's checkpoint, which holds the code but not
those two folders.

The options:

- It says so, and the folders come back empty (recommended). The checkpoint shows that it left the
  two folders out, on the Timeline and before you restore. After the restore they are empty, Silo
  names them, and `npm install` or a build fills them again. It costs the time of that rebuild.
- It says so, and the folders keep what they hold now. Nothing needs rebuilding at once. It costs a
  project that looks built when it is not: today's build output sits beside yesterday's code, and a
  build tool that compares dates can skip work it should redo.

Recommended: it says so, and the folders come back empty. A restore should give back one moment, not
a mix of two, and the rebuild that follows is the one you would run after a fresh clone.

## Notes

The record marks this sentence Proposed (l.2254–2255, l.1108–1109).
