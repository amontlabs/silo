# How a fork is made on each system

## For the owner

An agent's part, 22 November 2026. Main runs a dev server and a test watcher. At 14:02 you fork main
to try Bun. The fork gets main's disk as it was at 14:02, the agents' conversations included.

The options:

- A cold boot from a checkpoint (recommended). The fork starts from a checkpoint taken at 14:02 and
  boots like a computer turned on. The dev server and the watcher are not running in it until an
  agent or you starts them. It costs the wait of a boot, longer for a macOS computer than for a
  Linux one, and the programs to start again. If the copy fails, as on a full disk, no fork is made
  and Silo says why.
- Forks that keep the running programs, and no fork of a running computer until then. It costs forks
  of Linux computers until Silo can save a Linux computer's memory, and forks of a running macOS
  computer for good, since Apple's framework cannot move a saved memory to a new identity.

Recommended: a cold boot. A fork is mostly for trying something risky on the same files, and that
needs the disk, not the running programs.

## Notes

The answer is worded from the table of what can be seen per system, which comes from the plan's
notes:

| | Linux computer | macOS computer |
|---|---|---|
| Fork | Shares blocks. Under two seconds in earlier measurements. | A clone of the disk and a cold boot. One other macOS computer may run. |

