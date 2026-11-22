# What checks the real drivers and the real boundary

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Bisect a crash the simulator cannot shrink.** Confirm the exact reproduction, remove code in
  steps while it still compiles, and confirm by toggling only the last line both ways. The simulator
  shrinks failures in the core by itself. Drivers and live checks need this by hand.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/.claude/commands/crash-bisection.md?plain=1#L77))
