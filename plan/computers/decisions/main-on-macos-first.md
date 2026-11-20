# Which system main runs first

The owner said this, 18 November 2026. The owner expects much more interest this way.

## Why, as put forward

1. The visible thing people already do is give their agent a Mac of its own.
2. A macOS computer is what Linux sandboxes in the cloud cannot offer.
3. Agents that build Mac or iOS software need macOS. That includes the agents that build Silo's
   own app.
4. Under this design a fork is occasional. The limit of two macOS computers per Mac does not rule
   it out.

## Facts it has to respect

From earlier work on macOS computers. Not yet a decision.

- Apple allows two macOS computers running on one Mac. With main on macOS, one macOS fork can run
  beside it.
- A fork of a macOS computer is a clone of its disk and boots cold. A saved memory state cannot
  move to a new identity under Apple's framework.
- Apple forbids shipping a pre-installed macOS image. Each Mac downloads and installs its own, so
  the first run takes longer than with a Linux image (`first-run-time-macos`).
- A macOS computer needs a second VM engine, Apple's virtualization framework, driven from Swift
  on the engine side. That is decided, in `engine`. Its network goes through Silo's boundary,
  never Apple's NAT, as `engine` puts forward.
- A macOS computer is larger than a Linux one, in disk and in memory.

## A flaw to settle with it

A fork is a copy, so a Linux computer beside a macOS main is a fresh computer, not a copy. "Forks
are macOS or Linux" then holds only for a Linux main. `linux-beside-a-macos-main` says how a Linux
computer is made beside a macOS main.

## What follows from it, put forward

`linux-beside-a-macos-main`, `containers-on-macos`, `record-halves-per-system`,
`macos-ships-facts-first`, `x86-linux-computers`, `neutral-core`.
