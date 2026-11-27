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

## A power cut looks like damage

An agent's part, 25 November 2026, from [the record's head off the Mac](../../../research/2026-11-25-record-head-off-the-mac/README.md).
A power cut is not a process killed: the batch being written can reach the disk in part, with zeros
where a sector did not arrive, or a later sector without an earlier one. A simulation of the
writer through 4,100 power cuts per model, with every flush `F_FULLFSYNC`, lost no acknowledged line,
but the start took 119 of those states for damage, since the bad frame was not the last thing in
the file. With the recommended option, each such power cut would leave a damaged segment that
`silo verify` and the first screen show until you look.

A rule tells the two apart: in the last segment, a bad frame or a line that does not follow, after
the line the head on disk names, is a write that was never acknowledged. The start cuts it, keeps
the bytes and writes a fact, as it already does for a last frame cut short. Before the head, it is
damage, and this entry's options apply. Under that rule every one of the 4,100 states passed, and
#386's crash and tamper tests gave the same results. It asks that the head be written as often as
it is sent off the Mac (`record-head-off-the-mac`), so that the lines after it are only the last
minute's.

## Where the drafts disagreed, second pass (integration)

An agent's part, 25 November 2026, written while merging #390 and #392 to #396 onto #391 (`research/2026-11-25-integration-2/`).

#386 recommends that the recorder keeps recording in a new segment when it finds damage at start. #394 found that a power cut leaves tails that look like damage, so that option alone would flag every power cut. Recommended: #386's option with #394's after-head rule, which tells a cut tail from damage.

## Settled in the consolidation (27 November 2026)

Settled, 27 November 2026, in the consolidation: the answer now has #394's after-head rule.
