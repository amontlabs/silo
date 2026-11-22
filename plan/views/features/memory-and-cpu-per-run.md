# Memory and CPU of each run

## The choice

An agent's part, 22 November 2026. Your Mac has 16 GB. Main runs low on memory, Silo pauses it and
alerts you. Three runs were alive in main.

The options:

- Build it (recommended). Now shows each run's memory and processor, and the alert says "claude #2
  uses 9 GB", so you stop that run and resume main. It costs figures that come from inside the
  computer, so they are shown as not exact, and an agent could make them wrong.
- Leave it out. The alert says that main is low on memory. It costs a terminal in main and a command
  such as `top` to find which run to stop.

Recommended: build it, when its stage comes. The record is planned to sample each run's memory and
processor anyway, so it costs little and saves you the hunt.

## Notes

From the list of smaller features polarzero put on the roadmap on 21 November 2026. polarzero's
words on the list are in `what-ships-with-silo`.

## An agent's notes

- The meters of a whole computer are facts, seen from outside. The meters of one process come from
  inside the computer, so they are reports. The sum for a run also rests on which process belongs to
  which run. The app shows the figures of a run as not exact.
- The rule of `computers-low-memory-or-disk` still rests on the fact. The name of the heaviest run
  is added to its alert.
- An extension that reads the record and alerts could do this, once the record holds the meters of
  each process.
