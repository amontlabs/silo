# What Silo does when memory runs low

## For the owner

An agent's part, 22 November 2026. Your Mac has 32 GB. Main holds 14 GB, a fork running a large
build 10 GB, and a Linux computer 4 GB. You open Xcode and a few browser tabs, and macOS reports its
memory pressure as critical.

The options:

- Pause the largest first, main last (recommended). The fork with the build pauses and you get an
  alert. If the pressure is still critical a minute later, the Linux computer pauses too. You resume
  when memory is back. It costs that fork's work, which waits for you. A paused computer frees its
  memory only as macOS moves it out, so the Mac may stay slow for a while.
- Pause every computer at once. It costs all agent work, main's included, even when one fork caused
  the shortage.
- Alert only. It costs your Mac: everything slows down, your own apps included, until you act, and
  macOS may ask you to quit apps.

Recommended: pause the largest first, main last. It frees the most memory for the least work
stopped, and main, where your agents live, keeps going longest.

## Notes

For memory, the owner's first thought is pause and alert. A first thought, not a yes.
