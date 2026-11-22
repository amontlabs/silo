# A computer made from a recipe

## The choice

An agent's part, 22 November 2026. You make main from a recipe that names macOS, Claude Code, Node
22 and computer use, and main comes up with all of it. A Linux computer for a test is made from its
own recipe the same way, with the same versions every time.

The options:

- Build it (recommended). Everything a computer holds beyond its system comes from a text you can
  read, with exact versions, and Silo keeps that text with the computer. It costs disk for what Nix
  stores, reported at tens of gigabytes per machine, and the time of a recipe's first build. Both
  are still to measure.
- Leave it out. Every computer starts bare, and you or an agent set it up inside, then fork it. It
  costs every fresh computer, since a task or a Linux computer beside main starts with nothing in
  it, and nothing can make main again if it is lost.

Recommended: build it. Tasks that start ready, Linux computers beside main and the list of what is
only on a computer all stand on it.

## Notes

An agent's doubt, from drafting this feature on 21 November 2026.

It needs `nix-store-shared`, which is open.
