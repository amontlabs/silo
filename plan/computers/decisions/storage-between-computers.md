# Can a computer read what another computer left on disk or in memory

Agent, 22 November 2026, from Guillotine and guillotine-mini (evmts), two Zig codebases by one
maintainer.

- `nothing-shared-from-your-device` covers your Mac. This covers one computer and another: disk
  blocks freed by a dropped computer, and buffers in a helper.
- A fork shares blocks with its parent on purpose. That is sharing, not reuse, and stays as
  `work-on-top-of-libkrun` describes.
- A wipe cannot be guaranteed on an SSD, and the bin keeps dropped computers, so the promise is
  about what a computer reads, not about erasing.
- Guillotine zeroes memory before code can read it, and clears buffers that held secrets when they
  are released.
