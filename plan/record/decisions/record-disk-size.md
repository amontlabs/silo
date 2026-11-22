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
