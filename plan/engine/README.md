# The engine

The core piece. It decides, drivers act, and everything else only talks to it.

## For

To be the one place where Silo decides. The app, the command line and every tool only talk to it
(`engine-language`).

## Why it is so

- **One language and one design for the core** (`engine-language`, `engine-design`), on a Mac and
  on Linux (`engine-on-linux`).
- **macOS computers are driven from Swift**, on the engine side and never in the app
  (`macos-vm-driver`).
- **The core does not wait for a classifier** (`core-never-waits`).
- How the engine is cut into parts, and which processes Silo runs, are not settled yet:
  `engine-shape`, `processes`. Neither is the VM engine for Linux computers (`linux-vm-engine`).

## One example

A precaution makes a tool call wait at the boundary for a classifier's opinion. What waits is that
one request, not the core (`core-never-waits`). Which process keeps the limit of that wait is
still to decide (`synchronous-precaution-path`).
