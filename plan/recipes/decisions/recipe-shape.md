# The shape of a recipe

## The choice

An agent's part, 22 November 2026. An agent in a Linux computer downloads a tool from its release
page on GitHub and runs it.

The options:

- Ubuntu with the recipe's layer on top (recommended). The tool runs as on any Linux machine, and
  `apt install` works. It costs part of the exact rebuild. The system underneath is fixed by its
  version, not described package by package, and what agents add with `apt` is only on that
  computer.
- A system built wholly from the recipe. Everything in the computer is described and rebuilt
  exactly. It costs the downloads: a program fetched from the internet does not run without a fix,
  and agents fetch such programs all the time, then spend time and tokens getting around the
  failure.

Recommended: Ubuntu with the layer on top. Agents expect a machine that works like the ones they
know, and the recipe's layer still fixes the version of everything it adds.

## Notes

A proposal, built on the research in `nix-under-recipes`.
