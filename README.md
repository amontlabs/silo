# Silo

Agents work in a computer of their own. Silo records everything they do there, shows you what
matters, and takes you back to any moment.

## The shape

1. **A computer.** Your agents work in one long-lived virtual machine on your device, called main.
2. **The record.** Silo writes down every process, file change, connection and model call in it, seen from outside.
3. **Going back.** Silo takes checkpoints on its own, and you restore one or continue from it in a fork.
4. **Forks for the exception.** A fork is a copy of a computer for a risky try, then dropped or made main.

## Status

Silo is being rebuilt in the open, one feature at a time. The plan, and what is being built
now, is on [the Workshop](https://silo.amontlabs.com/workshop). There is nothing to install yet.

The earlier code is kept at the tag `legacy-2026-11-16` (branch `legacy`). The earlier plan is
kept at the tag `plan-2026-11-18`.

## Licence

Silo is [MIT licensed](LICENSE).
