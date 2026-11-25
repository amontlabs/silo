# The record damaged when the recorder starts

## The choice

An agent's part, 25 November 2026, from [the record's format](../../../research/2026-11-25-record-format/README.md).
Overnight a disk error changes one byte in the middle of today's segment of main's record. In the
morning the recorder starts again and finds a line whose CRC fails, with good lines after it. It
is not a line the recorder was writing when it stopped: that one would be the last thing in the
file, and the recorder cuts it off by itself, keeps its bytes, and writes a fact saying so.

The options:

- Keep recording (recommended). The recorder leaves the damaged segment as it is, starts a new one
  after the last line it could read, and writes a fact naming the damaged segment and the place.
  `silo verify` names the damage until you look at it, and the first screen says it. It costs a
  record that carries on past a damage nobody has looked at yet.
- Stop until you look. The recorder refuses to start. Under `enforcing-and-recording`, every
  computer then stops getting anything that needs a new decision, and the engine refuses calls with
  `record_behind`. It costs every agent's work for one bad byte, possibly hours before you see it.

Recommended: keep recording. The damage stays where it is and is shown, and nothing that happens
after it goes unrecorded. The prototype refuses to start, which shows the damage is found; it does
not yet do the recommended option.
