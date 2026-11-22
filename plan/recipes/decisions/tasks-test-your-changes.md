# Testing your changes

## The choice

An agent's part, 22 November 2026. You change the sync code of your Mac app in main, without
committing, and ask Silo to test it. Silo makes a fresh macOS computer and a fresh Linux computer
from the repository's recipes and runs `cargo test` in each. macOS passes, and Linux fails on one
test.

The options:

- Fresh computers from the recipes (recommended). A pass means the change works on a clean machine
  of each system. It costs waiting for two computers to be made at each test, and how long that
  takes is still to measure. The macOS one takes one of the two places for macOS computers on your
  Mac.
- A fork of main. The tests run in a copy of main as it is. It costs the clean machine, since a pass
  may rest on something an agent installed in main by hand. A fork of main is also macOS only, so
  Linux needs a fresh computer anyway.

Recommended: fresh computers. Before you push, you want to know whether the change works on a clean
machine of each system, and only a computer made from the recipe tells you.

## Notes

polarzero's example, 18 November 2026. How it would work is put forward in
`task-gets-one-commit`, `engine-runs-task-checks` and `task-macos-limit`; the results come back
as receipts (`what-a-receipt-is`).
