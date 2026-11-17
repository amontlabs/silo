# Silo

Silo gives an agent its own computer, lets it copy that computer for each task, shows you
everything the copy changed, and lets you merge it or drop it.

## The loop

1. **A computer.** Your setup and your agent live in one computer, called main. It is a virtual
   machine on your device. Nothing from your device is shared into it.
2. **A copy per task.** The agent, or you, forks main, or starts a fresh computer from a recipe
   or a checkpoint. The copy is a whole machine, so two tasks never share a port or a database.
3. **Everything that changed.** The diff covers code, packages, settings, files and databases.
4. **Merge or drop.** Merging brings the changes into main and asks you first. Dropping leaves
   nothing behind.

## Status

Silo is being rebuilt in the open, one feature at a time. The plan, and what is being built
now, is on [the Workshop](https://silo.amontlabs.com/workshop). There is nothing to install yet.

The earlier code is kept at the tag `legacy-2026-11-16` (branch `legacy`).

## Licence

Silo is [MIT licensed](LICENSE).
