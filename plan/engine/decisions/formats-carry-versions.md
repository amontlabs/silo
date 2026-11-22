# How does Silo read what an older version of it wrote

Agent, 22 November 2026, from Guillotine and guillotine-mini (evmts), two Zig codebases by one
maintainer.

- A version cannot be added once data is written without one, so this is settled before the
  first file is written.
- Rule decisions already carry the version of the rule (`rule-decisions-are-recorded`).
- From guillotine-mini: a field that a version introduces is required from that version and refused
  before it; a reader maps an older record onto the current shape before using it; a mark kept for
  a future format is refused today.

## For the owner

An agent's part, 22 November 2026. Two years from now, after many updates of Silo, you open the
record of a run from this month.

The options:

- Read forever, never rewritten (recommended). Any newer Silo reads that record as it was written.
  Silo's own working state may be rebuilt at an update. It costs carrying the old readers forever.
- Converted on update. An update rewrites old records into the new form. It keeps Silo simpler, and
  it costs rewriting the one thing that is meant never to change.
- Recent versions only. Silo reads records from its last few versions. It costs your older records,
  which become unreadable.

Recommended: read forever, never rewritten. A record is worth something only if it can still be
read, untouched, years later.
