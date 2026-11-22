# Hostile tests on the real thing

## The choice

An agent's part, 22 November 2026. An agent with root in a real Linux computer tries to reach your
Mac, to read the cloud metadata address 169.254.169.254, to get past a rule that denies paste.rs,
and to stop what Silo sees from outside. Each attempt tests one promise as it is worded for you, and
Silo states a promise in public only when its tests pass.

This is what `checks-on-the-real-thing`, which you took, already says. Taking it is a yes to
building it when its stage comes.

The options:

- Build it (recommended). It costs real computers on a Mac and on a Linux host at each release, and
  tests to write for each new promise and each new way out of a computer.
- Leave it out. It costs every promise about the edge of a computer, since the simulator checks only
  the engine, never the real virtual machine or network. It also goes against a decision you took.

Recommended: build it. Silo treats the agent as hostile, so its promises have to be tried by an
agent that is.
