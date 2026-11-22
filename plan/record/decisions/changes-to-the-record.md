# Can anything change what the record already holds

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **An append-only log exposes no edit.** An append-only log's type has no operation that changes an
  entry. The record's Zig type offers append and read for its lines, and the check that reads the
  source refuses writing a line any other way. Refusals and losses are new lines, appended like any
  other.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/client/state/journal.zig#L14))
