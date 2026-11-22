# The engine

The core piece. Everything else only talks to it.

## For

Given by `engine-language`.

## Why it is so

- **One language and one design for the core** (`engine-language`, `engine-design`), on a Mac and
  on Linux (`engine-on-linux`).
- **macOS computers are driven from Swift**, on the engine side and never in the app
  (`macos-vm-driver`).
- **The core does not wait for a classifier** (`core-never-waits`).
- How the engine is cut into parts, and which processes Silo runs, are not settled yet:
  `engine-shape`, `processes`. Neither is the virtual machine monitor that runs Linux computers
  (`linux-vm-engine`).

## One example

A precaution makes a tool call wait at the boundary for a classifier's opinion. What waits is that
one request, not the core (`core-never-waits`). Where that wait is handled is still to decide
(`synchronous-precaution-path`).
