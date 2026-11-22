# An extension that reads and alerts

## For the owner

An agent's part, 22 November 2026. Disk watch wakes every minute and reads /work on main. When /work
passes 40 GB it alerts you, and the alert opens on the file lines of the build that wrote 12 GB of
node_modules at 14:10.

The options:

- Build it (recommended). You add checks Silo does not ship, written by you or by anyone. It costs a
  computer that runs for each such extension, and a line in the record for each read and each alert.
- Leave it out. Silo alerts you only on what Silo itself knows to look for. It costs every check
  particular to your work: a disk limit, spend by project, a run that writes where it should not.

Recommended: build it. Watching a lot of work your own way is the first thing extensions are for,
and this one can only read and tell you.
