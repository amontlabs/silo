# When memory or disk runs low

## The choice

An agent's part, 22 November 2026. Your Mac runs short of memory while a fork builds, and later
short of disk while main writes a large file.

The options:

- Build it (recommended). For memory, the computer that holds the most pauses first, main last, and
  you are alerted. For disk, you are alerted early, and every computer pauses before the disk is
  full. Silo removes nothing by itself, and you resume. It costs the agents' time while they are
  paused.
- Leave it out. It costs your Mac: it slows down for everything, and a computer can fill the disk in
  the middle of a write.

Recommended: build it. The meters are facts Silo reads from outside, so it can act on them exactly.
