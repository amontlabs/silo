# The record's size on disk

## The choice

An agent's part, 22 November 2026. Main runs agents all day: builds, tests, three long
conversations, and one agent that drives the screen for an hour. Rough estimates say the lines of
one busy computer take tens of megabytes a day, and a screen recording far more.

The options:

- Measure first (recommended). Several days of real work in main, and one run made to flood the
  record, give the bytes per day of each kind of line, with message text and recordings counted
  apart. The default size limit, and the share at which Silo warns you, are set from those numbers
  and come to you then. It costs a record with no default limit until the measures exist.
- A default from the estimates now. The limit is set today from the rough figures. It costs a guess
  on your disk: recordings swing the figures most, so the limit can remove recordings and message
  text too early, or let the record fill the disk.

Recommended: measure first. The default limit decides what gets removed from your record, so it
should rest on what real work writes.

## Notes

To find out, per kind of fact and report.

Rough estimates for one busy computer per day: tens of megabytes for process, file and connection
lines once merged and compressed; a few megabytes per long conversation; recordings are the heavy
item and are kept only while an agent drives the screen.

## Carried over 30 nights (26 November 2026)

An agent's part, 26 November 2026, from `research/2026-11-25-disk-budget/`. With #386's heavy night per agent-hour (lines
0.29 MB once compressed, message text 0.55 MB), 30 nights of agents working 17 hours a day hold
0.4 GiB of lines and 0.8 GiB of text for 3 agents, 1.1 and 2.1 GiB for 8. Recordings of computer
use were measured on Linux: 0.2 GiB an hour encoded by quality, 0.75 GiB at a fixed 6 Mbit/s, so
an hour a day of computer use is 6 to 23 GiB a month, most of the record. With the full list of
hashes on every request, lines reach 8.2 GiB for 8 agents in 30 nights, about 100 GiB a year
that no limit may remove.
