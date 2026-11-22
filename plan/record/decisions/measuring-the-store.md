# Designing the store

polarzero said this.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Track the running total, never rescan.** Keep the current size and cost as you go, charge only
  the difference, and recompute only what changed. Disk use of the record and of checkpoints is kept
  as a running total.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/docs/dev/GAS-METERING.md?plain=1#L90))
