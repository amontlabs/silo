---
"silo-ui": minor
---

A fork's changes can be kept into the computer it came from with `silo fold`, and put back with `silo undo`. A keep first saves a checkpoint of the computer, stops without changing anything when the fork and the computer changed the same thing or when a database change has no migration, replays packages by name and version, merges git, copies files, merges crontabs and launch agents, and runs the recipe's migrate, restart and after steps. Works for Linux and macOS computers.
