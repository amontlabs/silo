# Simulators for parallel agents

## The choice

An agent's part, 24 November 2026. Main is a macOS computer. Two agents work in it on one iOS app.
Each runs the tests in the iOS simulator. Claude Code's documentation, as a search showed it on 24
November 2026, says that when a prompt names
no simulator, Claude uses one that is already running, even one another session opened. A reported
issue has the simulator tool ignoring the requested device when two sessions run on one Mac. So
agent B can erase the simulator agent A is testing in, and A's tests fail for a reason A cannot
see.

A fork does not settle it. Apple allows two macOS computers running on one Mac, so at most one fork
runs beside main (`main-on-macos-first`). The agents that test iOS apps in parallel share one
computer.

The options:

- Silo does not choose; it makes the clash visible (recommended). A recipe for Apple platforms can
  create several simulators with distinct names, such as `iPhone A` and `iPhone B`, and a file the
  agent program reads, such as `AGENTS.md`, can tell each agent to name one. The record shows which process booted,
  erased or installed into each simulator, so the morning shows that agent B erased what agent A
  used. It costs nothing that Silo does not already plan, since the simulator runs as processes
  and files of the computer. It leaves the clash possible.
- Silo gives each conversation its own simulator. Silo creates a simulator when it sees a new
  conversation (`recognising-a-conversation`) and points the agent at it. It costs a change to what
  the agent program sees, which `unmodified-agent-programs` and `what-silo-does-not-build` rule
  out, and no supported way was found to force the device an agent program picks.
- One agent per computer for iOS work. Parallel iOS work runs in main and one fork. It costs every
  third agent, and a fork boots cold from a copy of main's disk.

Recommended: Silo does not choose, and the record makes the clash visible. It keeps Silo out of
the agent's work and still answers the question you have in the morning, which is why a test
failed.

## Why this comes up

Agents that build Mac and iOS software are one of the reasons for `main-on-macos-first`. Linux
computers have no Xcode and no iOS simulator, so these agents can only run on macOS. Developers who
run agents on iOS apps report this clash today. It is to check on a Mac how often it happens, and
whether `xcrun simctl` events are enough for the record to name the process.
