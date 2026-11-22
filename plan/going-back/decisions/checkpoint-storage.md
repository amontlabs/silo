# Where is a checkpoint stored

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **A checkpoint names its place in the record.** Represent a checkpoint as a position in an
  append-only log of changes. A checkpoint of a computer's disk also carries the record position at
  which it was taken, so "what happened since this checkpoint" is a plain query.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/client/state/journal.zig#L5))
