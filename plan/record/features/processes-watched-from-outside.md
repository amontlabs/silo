# Processes watched from outside

## For the owner

An agent's part, 22 November 2026. Today the list of commands that ran in main is told by a part of
Silo inside main, and an agent with full access can interfere with it. Watching from outside would
make that list something Silo saw itself, which a rule could then read.

The options:

- Leave it out (recommended). The commands stay told from inside. It costs any rule that would act
  on which program is running, and an agent with full access can hide a process from the record.
- Build it. It costs work with no known way on a macOS computer: as far as we know, Apple's
  virtualization gives no view of the programs running inside. On Linux it means reading the
  computer's memory and following every version of its kernel.

Recommended: leave it out. The part within reach, a Linux kernel that the agent cannot change, is
already looked at in `processes-from-inside-linux`.
