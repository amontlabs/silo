# File changes, seen from outside

To find out, on Linux and on macOS computers. It is the main research question for the record.
What each route costs is part of it.

Two routes:

- **The engine serves the work folders itself.** Every file operation passes through it, live.
  The cost is speed, exactly where builds run. A folder served from outside the computer, or a
  filesystem layer, is measured before it is adopted, and a slowdown of eight times on parallel
  file work is the failure to avoid. (This last sentence is in the plan's notes; it is a
  proposal.)
- **The engine reads the disk at each checkpoint and compares.** Nothing is slowed while the
  computer runs. The result is only as fine as the checkpoints, and the engine has to read the
  computer's filesystem.
