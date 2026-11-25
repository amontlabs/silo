# Can main be a Linux computer

## The choice

An agent's part, 22 November 2026. A developer who builds a Linux server app installs Silo on a Mac
and wants main to be Linux: lighter, giving idle memory back, and with more than one fork running
beside it.

The options:

- Your choice (recommended). On a Mac you choose macOS or Linux when you make main. On a Linux
  server main is Linux. It costs two kinds of main to build and test, and what assumes a macOS main,
  such as bringing in your agent programs from your Mac with the same paths, does not apply to a
  Linux main.
- macOS only, on a Mac. It costs that developer a heavier main than needed, with at most one fork
  running beside it.

Recommended: your choice. It is what polarzero said they wanted, and its cost comes after the work on a
macOS main, in a later stage.

## polarzero's words (21 November 2026)

> our purpose is just to start with mac but we want mac and linux possible as main computer really
> it should just be user choice.

This is a lean, not a verdict. The answer is written from these words.

## An agent's notes

- There are two cases. A Linux main can run on a Mac. A Linux main can run on a Linux machine
  (`linux-server-host`, and `engine-on-linux`, taken).
- macOS runs only on Apple hardware. On a Linux machine main is a Linux computer, and the user has
  no choice there. `one-kind-of-computer` allows a difference the platform forces.
- No taken decision conflicts with it. `who-it-is-for` says who is first: polarzero, on a Mac.
  `where-it-runs` says the user's own machine comes first. `main-and-its-agents` says there is one
  main and not which system it runs. `mac-agent-not-the-main-path` is about where the agent runs.
- `main-on-macos-first`, which has no verdict, says "first". This decision says what comes after.
  Its notes already say that "forks are macOS or Linux" holds only for a Linux main.
- To state when a Linux main is designed: `agent-programs-moving-in` says what you bring in keeps
  the same system, paths and tools, which is true only of a macOS main.
  `linux-never-copies-macos-main` has a mirror: a macOS computer beside a Linux main cannot be a
  copy of it either.
- Placed in the stage "Later", before `linux-server-host`, so that it comes after the work on the
  Mac.

## As a fallback for the first build

An agent's part, 24 November 2026, from [the comparison of first builds](../../../research/2026-11-24-first-build-and-the-record/README.md). If the first build's macOS main fails on the Mac
kit's tests, this entry is the one that moves forward: a Linux main on the Mac, with the same
engine, boundary and recorder. The comparison counts 94 open decisions for that first build,
against 88 for a macOS main with the record of model traffic.
