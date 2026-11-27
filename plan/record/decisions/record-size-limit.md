# The size limit

## The choice

An agent's part, 22 November 2026. You set the record's limit at 20 GB. It holds 15 GB of
recordings, 4 GB of message text and 1 GB of lines, and it is full.

The options:

- Bulky content goes, lines stay (recommended). The oldest recordings are removed, then the oldest
  message text. You still see that a turn happened, what it cost and which files changed, without
  its text. It costs old recordings and old conversation text.
- The oldest of everything goes. The record keeps the most recent 20 GB. It costs the trail, since
  an agent can push out what it did by making noise, and old totals of spend disappear.

Recommended: bulky content goes, lines stay. Lines are small and are what everything else is
computed from.

## Notes

What Silo does when the limit is still reached after that is the open decision
`record-full-after-bulk`. The record names no check for the last sentence: `simulator` is put
forward here.

## Against the rest of the disk (26 November 2026)

An agent's part, 26 November 2026, from `research/2026-11-25-disk-budget/`. Removing the record's content before checkpoints
buys almost no way back: 30 days of message text for 3 agents is 0.8 GiB, under half an hour of
checkpoints at 2 GiB written an hour. So the record's content goes by this limit only, and a
limit on everything Silo keeps counts the record but never removes it (`low-disk`'s notes). A
20 GB limit holds about 8 days of 3 hours of computer use a day recorded at a fixed bitrate, and
30 days encoded by quality, which is 3.5 times smaller on the same screen.
