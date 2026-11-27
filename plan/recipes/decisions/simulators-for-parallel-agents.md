# How do several agents in main use simulators at once

## The choice

An agent's part, 24 November 2026. Four agents work in main on the same iPhone app, each in its own
worktree. Each wants to build the app and look at it in a simulator. When two use the same
simulator, one installs over the other's app, and one agent's test run kills the other's. When
they share one build folder, the build database is locked. Each booted simulator holds about
3.4 GB of memory.

The options:

- A simulator and a build folder for each agent, with a maximum booted (recommended). A piece for
  Apple development gives each agent its own simulator, cloned from a base one, and its own build
  folder, through the agent programs' documented settings such as hooks and the environment. The
  number booted at once fits main's memory and is shown. It costs a piece Silo ships and keeps up
  with Xcode, and an agent can wait for a simulator.
- Leave it to the agents' own tools. Plugins such as xcfs and Claude Code plugins already do this
  inside one Mac. It costs the case: each user finds and sets up one, they disagree with each other,
  and nothing sizes main's memory for them.
- A computer for each agent. It costs what Apple allows: two macOS computers running on a Mac,
  main included.

Recommended: a simulator and a build folder for each agent. The two-computer limit puts parallel
iOS work inside main, so this is where Silo makes it work.

## Notes

An agent's research, 24 November 2026: `research/2026-11-24-mac-and-ios-developers/README.md`, sections "Who they are"
and "Simulators". Three tools were made in the last months for this one problem, and
their measurements are quoted there. Mac test 25 measures how much memory each booted simulator
takes in a computer and how many fit.

It bears on `sizing-target`: 16 agents in main on a 32 GB Mac cannot each have a booted simulator.
The heavy load for an iOS developer is a few booted simulators shared out by this maximum.

## The record names who touched a simulator, from a second draft (24 November 2026)

An agent's part, 24 November 2026. A second draft asked the same question under this id, in
`computers`, and recommended that Silo not choose and only make the clash visible. Two drafts held
one id, so it is kept here as a note. What it adds to the answer above: the record shows which
process booted, erased or installed into each simulator, so that if agent B still erases the
simulator agent A was testing in, the morning shows it. Two facts it found also stand here: Claude
Code's documentation says that when a prompt names no simulator Claude uses one already running,
even one another session opened; and a reported issue has the simulator tool ignoring the requested
device when two sessions run on one Mac. It is to check on a Mac whether `xcrun simctl` events are
enough for the record to name the process.

## Held per use, kept warm (25 November 2026)

An agent's part, from `research/2026-11-25-checkpoint-retention/`, added on merge because #389 wrote it before #361 had landed. In a simulation of a night of 16 agents, a simulator held for the whole session leaves 10 of 16 agents with none when 6 may be booted. Held per use and kept booted until another agent needs the place, 6 booted keep 95 waits in 100 under 30 s. "For as long as it uses them" could read "while it runs the app; its simulator stays booted until another agent needs the place".
