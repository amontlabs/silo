# Where an x86 Linux computer runs

## For the owner

An agent's part, 22 November 2026. An agent in a Linux computer beside main needs a tool that exists
only for Intel processors, or a container image built only for them.

The options:

- arm64 on a Mac, Intel on a Linux server (recommended). On your Mac, Linux computers run arm64 at
  full speed, and an Intel-only program runs only through an emulator the agent installs, several
  times slower. A real Intel Linux computer runs on an Intel or AMD server that runs Silo. It costs
  the rare Intel-only tool, which is slow on the Mac.
- Linux computers on Apple's own virtual machines, with Rosetta. Intel programs run at nearly full
  speed on the Mac, as in Docker Desktop and OrbStack. It costs what the engine put forward for
  Linux computers gives: memory given back when idle, and Silo's own fixes to it.

Recommended: arm64 on a Mac. Almost every tool agents use is built for arm64 today, and memory given
back matters on every Mac.

## Notes

It rests on a decided line: the engine also runs on Linux (`engine-on-linux`, in `engine`).
