# Is "What Silo adds costs as little as it can" a principle of Silo

## The choice

An agent's part, 22 November 2026. Main runs a large build that changes 40,000 files, and Silo
records every change while it runs.

The options:

- A principle (recommended). Every addition says what it adds in memory, processor, disk and
  waiting, with a target, before it is built. It costs slower additions, and some are redesigned or
  refused for their cost.
- Measure when it is slow. Features are built first and tuned when someone complains. It costs
  weight that builds up unseen and is expensive to remove later.

Recommended: a principle. You asked on 21 November that anything of Silo's be as light as possible,
and a virtual machine is already heavy enough.

## Notes

Agent, 22 November 2026, from Guillotine and guillotine-mini (evmts), two Zig codebases by one
maintainer.

The owner, 21 November 2026: "My wish being that anything silo is as lightweight as possible and
barely demands anything (excluding stuff like vms of course which is not silo but
libkrun/microsandbox/third party)."

- `core-is-not-the-slow-part` stays: it is about the core, which decides rarely. This principle is
  about everything else Silo runs: the helper, the recorder, the probe, the engine and the app.
- The targets are `own-cost-targets`, in the part `checking`.
