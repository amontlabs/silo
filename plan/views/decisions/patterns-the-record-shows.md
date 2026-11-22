# Which repeats can the record show, when file reads are not recorded

## For the owner

An agent's part, 22 November 2026. An agent reads the same 4,000-line file twenty times in one run.
You asked for repeated reads to be noticed, and you also decided that file reads are not recorded.

The options:

- Show what Silo can see (recommended). Silo shows repeated calls, repeated writes, and the same
  content given to the model again. It catches the twenty reads when they cost you tokens. It costs
  seeing the repeated reads that never reach the model.
- Record file reads too. Every read of a file is recorded, so repeats can be listed. It costs a
  flooded record and a slower computer, because a single build reads everything.

Recommended: show what Silo can see. What costs you is the content going to the model again, and
Silo already sees that.

This would replace `patterns-in-the-record`, whose wording is yours. The audit's finding is in the
notes of `file-reads`.
