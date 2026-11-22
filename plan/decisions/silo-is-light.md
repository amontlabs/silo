# Is "What Silo adds costs as little as it can" a principle of Silo

Agent, 22 November 2026, from Guillotine and guillotine-mini (evmts), two Zig codebases by one
maintainer.

The owner, 21 November 2026: "My wish being that anything silo is as lightweight as possible and
barely demands anything (excluding stuff like vms of course which is not silo but
libkrun/microsandbox/third party)."

- `core-is-not-the-slow-part` stays: it is about the core, which decides rarely. This principle is
  about everything else Silo runs: the helper, the recorder, the probe, the engine and the app.
- The targets are `own-cost-targets`, in the part `checking`.
