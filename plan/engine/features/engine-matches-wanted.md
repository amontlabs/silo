# Computers brought to what you asked

## For the owner

An agent's part, 22 November 2026. Main and two forks are running when your Mac restarts overnight.
One of the forks had also died by itself an hour before.

The options:

- Build it (recommended). In the morning Silo shows each computer as it really is, acts until the
  ones you asked to be running are running, and says what it is waiting for when it cannot. It never
  shows a computer as stopped while it may still run. It costs in-between states in the app, such as
  starting, waiting or unknown, and each needs its wording.
- Leave it out. Silo shows what it last saved. It costs you a check of every computer by hand after
  each restart or crash, and main on your Mac cannot be built, since it needs this.

Recommended: build it. A long-lived main has to survive the restarts of a Mac.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **A kill that fails is escalated.** When stopping a process fails, record that it may still be
  running and escalate, instead of only warning. Silo never shows a computer as stopped while its
  virtual machine may still run.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/test_runner.zig.md?plain=1#L459))
