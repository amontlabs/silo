# What a fact carries

The record's principles for the record path say the same of both: "A fact or a report is written
once. A correction is a new one."

That facts and reports are written once is already in force, in `what-the-record-holds`.

## A challenge that belongs elsewhere, 25 November 2026

An agent's part. #352 (file 08) found that a fact about a file also needs the stretch of the file it
covers and the version of the source that saw it. It proposes to add them where each kind of line is
defined, in `kinds-of-lines` and `sources`, not in this answer, which stays as it is. Those two
entries are open, so the addition is an edit of them, for polarzero, when the record's format is
written (the open draft on the record format reconciles #370's `formats.md`, #371 and #362).

## What the line holds

An agent's part, 25 November 2026, from [the record's format](../../../research/2026-11-25-record-format/format.md), which
reconciles #370, #371 and #362. Every fact and report starts with the same keys, in this order:
`v`, `seq` (its number in the computer's chain), `prev` (the hash of the line before),
`time` (when the source saw it, UTC, in nanoseconds), `computer`, and `source` with the id of
its count and the line's number in it (`sources-number-their-lines`). Then `is` and `kind`.
A correction is a new line with `corrects`, the number of the line it corrects. The prototype's
verifier refuses a line that breaks this order, and its fuzzer changed a real record 700,000 times
without verify saying a changed record held.
