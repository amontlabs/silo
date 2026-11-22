# How speed is measured

## Sources

- [sled performance guide](https://sled.rs/perf.html): measuring distributions, worst cases.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **A speed claim names its measurement.** Every speed figure comes with its workload, machine and
  sample. Profile and record a baseline before optimizing, and benchmark the production settings,
  worst cases included. `how-speed-is-measured` asks for distributions over interleaved runs, and
  `publishing-measurements` for a date and a machine. This adds a baseline before optimizing, and
  benchmarks on the production settings, worst cases included. Guillotine claims "zero branch
  misprediction" with no counter behind it.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/docs/performance/README.md?plain=1#L9))
