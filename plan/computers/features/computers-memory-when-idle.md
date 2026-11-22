# Memory given back when idle

## For the owner

An agent's part, 22 November 2026. On a Mac with 32 GB, main and two forks each grew to 10 GB during
a build. At night the forks' agents finish and the forks sit idle.

The options:

- Build it (recommended). The idle forks give memory back to your Mac, how much is measured and
  shown, and Silo shows how many computers can run on your Mac. It costs measuring at each release.
  For macOS computers, whether Apple's framework gives memory back at all is to find out, so this
  may first hold only for Linux computers.
- Leave it out. A computer keeps the most memory it ever used until you stop it. It costs computers:
  fewer fit on your Mac, and you stop idle ones by hand.

Recommended: build it. Memory decides how many computers a Mac holds, and you want memory to be
where Silo does better.
