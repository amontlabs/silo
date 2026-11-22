# A Linux computer beside main

## The choice

An agent's part, 22 November 2026. An agent in main wants to run the tests of `~/work/app` on Linux,
with a Postgres in Docker. Silo makes a Linux computer from the repository's recipe and hands it a
commit of main's current work.

The options:

- Build it (recommended). Main's agents get Linux beside them, arm64 on a Mac, from one Ubuntu image
  downloaded once. It costs the memory of a second computer, a recipe that works on Linux, and
  Intel-only programs, which run there only slowly, through an emulator.
- Leave it out. It costs Linux for main's agents on a Mac, and with it containers, which a macOS
  computer cannot run.

Recommended: build it. Most of what agents build runs on Linux servers, and a Linux computer is the
light one to make on a Mac.

## Notes

An agent's doubt, from drafting this feature on 21 November 2026.

It needs `containers-on-macos`, which is open, and its text says nothing about containers.
