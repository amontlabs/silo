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

## For an iOS project

An agent's note, 24 November 2026. A fresh macOS computer for an iOS project starts without Xcode
and without a simulator runtime, and builds from cold. The runtime alone is 9.0 GB for iOS 27.3,
and three agents on new worktrees waited 284 to 387 s for their first builds in the xcfs project's
measurements. A computer cloned from the kept image with Xcode (`xcode-in-a-computer`) avoids the
first part; nothing in this answer avoids the cold build. Whether "fresh" should allow a clone of
a checkpoint of main for such a project is a question for polarzero. Details: `research/2026-11-24-mac-and-ios-developers/README.md`.
