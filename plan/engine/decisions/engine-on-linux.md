# Does the engine run on Linux

Decided on 16 November 2026.

It is one of the reasons put forward for libkrun in `linux-vm-engine`: libkrun runs on a Mac and on
a Linux host.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **Fixed at build, chosen at run only when needed.** Specialize at build time what is fixed for a
  build. Choose at run time only where one process must handle several cases. What differs between
  macOS and Linux hosts is chosen at build time. What differs between computers is a row.
  ([source](https://github.com/evmts/guillotine/blob/dbdf61bd7902c726cdfb4cf80d60ed0e02da5663/docs/dev/HARDFORKS.md?plain=1#L357))
