# Can a computer read what another computer left on disk or in memory

## The choice

An agent's part, 22 November 2026. You drop a fork in which an agent had loaded a dump of a
production database. A week later you make a fresh Linux computer, and an agent in it, with `sudo`,
reads every raw byte of its disk and of the memory it was given, looking for what was there before.

The options:

- Promise it (recommended). It finds only zeros, and a fork finds only what its parent had. It costs
  a little speed, since Silo's own parts clear memory before they use it again, and tests that
  attack from inside a computer, which must pass before Silo says it in public.
- Leave it to macOS and Linux. They already hand a new file and new memory over as zeros. It costs
  what they do not cover: memory that Silo's own parts reuse from one computer to the next, such as
  the part that carries their network traffic, with nothing testing it.

Recommended: promise it. What one computer held is exactly what an agent in another must not read,
and the cost is small.

## Notes

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
