# Which system main runs first

## The choice

An agent's part, 22 November 2026. You install Silo on your Mac. Main is where your agents will
live, and it is either a macOS computer or a Linux one.

The options:

- macOS first (recommended). Your agents get a Mac of their own: they can build and test Mac and iOS
  apps, use a Mac screen, and what you bring in from your Mac keeps its paths and tools. A Linux
  computer beside main is never a copy of it: it is a fresh computer. It costs a first run that
  downloads macOS from Apple, over 10 GB, and installs it. A macOS computer takes more disk and
  memory than a Linux one. Apple allows two macOS computers running on one Mac, so at most one fork
  of main runs beside it. A fork of main starts from a copy of main's disk, so the programs that
  were running in main are not running in it.
- Linux first. Main is lighter, gives idle memory back, and several forks can run beside it. It
  costs what makes Silo different: a Linux machine for agents is what cloud services already offer,
  and agents that build Mac software, Silo's own app included, cannot do it there.

Recommended: macOS first. A Mac of its own is what people already give their agents, and no cloud
offers it.

## Notes

polarzero said this, 18 November 2026. polarzero expects much more interest this way.

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
- A macOS computer needs a second virtual machine monitor, Apple's Virtualization framework, driven
  from Swift on the engine side. That is decided, in `engine`. Its network goes through Silo's
  boundary, never Apple's NAT, as `engine` puts forward.
- A macOS computer is larger than a Linux one, in disk and in memory.

## What a fork is beside a macOS main

polarzero said "forks are macOS or Linux". A fork is a copy, so a Linux computer beside a macOS
main is a fresh computer, not a copy, and the answer says so. `linux-beside-a-macos-main` says
how the code reaches it.

## What follows from it, put forward

`linux-beside-a-macos-main`, `containers-on-macos`, `record-halves-per-system`,
`macos-ships-facts-first`, `x86-linux-computers`, `neutral-core`.

## Weighed against a Linux main first

An agent's part, 24 November 2026, from [the comparison of first builds](../../../research/2026-11-24-first-build-and-the-record.md). A Linux main on the Mac in the first build was
weighed, because more of the record has already run on Linux (#347, #367, #371) and a Linux
computer can give processes from a probe with a kernel of Silo's own. It is not recommended: it
lands beside free Linux microVM products on the Mac, it gives up Xcode and macOS apps in main, and
the macOS main would still have to be built later, which writes the computer, its checkpoints and
its file comparison twice. It stays the fallback if the Mac kit (#359) shows that macOS 27's
provisioning or gvproxy under a macOS computer does not work: the engine, the boundary and the
recorder run on the Mac outside every computer and carry over unchanged.
