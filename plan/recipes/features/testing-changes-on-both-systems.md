# Testing your changes on macOS and Linux

## The choice

An agent's part, 22 November 2026. An agent in main changes how your site builds its pages. Before
it pushes, Silo tests the change in a fresh macOS computer and a fresh Linux computer, and shows the
two receipts side by side: macOS passed, and Linux failed on one test.

The options:

- Build it (recommended). You learn before pushing that a change breaks one system, from a run no
  agent touched. It costs two computers made at each test, the wait for them, and one of the two
  places for macOS computers on your Mac. It also waits for Linux computers beside main.
- Leave it out. You push, and your CI tells you. It costs time, since the failure comes after the
  push, when the agent that made the change may have moved on.

Recommended: build it. It is the use you gave for tasks, and it is where a fresh computer and a
receipt help most.
