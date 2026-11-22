# How does Silo read what an older version of it wrote

Agent, 22 November 2026, from Guillotine and guillotine-mini (evmts), two Zig codebases by one
maintainer.

- A version cannot be added once data is written without one, so this is settled before the
  first file is written.
- Rule decisions already carry the version of the rule (`rule-decisions-are-recorded`).
- From guillotine-mini: a field that a version introduces is required from that version and refused
  before it; a reader maps an older record onto the current shape before using it; a mark kept for
  a future format is refused today.
