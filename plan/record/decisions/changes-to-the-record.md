# Can anything change what the record already holds

## The choice

An agent's part, 22 November 2026. You dropped ten forks last spring and emptied the bin. Their
record is still on your Mac, with the conversations you had in them.

The options:

- Only you, once the computer is gone (recommended). Nothing an agent does, no restore and no drop
  can change or remove a line. You can remove the whole record of a computer that has left the bin,
  and a line says you did. It costs the sentence "the record is never removed", which becomes "never
  by a computer".
- Nobody, ever. The record of every computer you ever had stays on your Mac. It costs disk that only
  grows, and the only way to remove a conversation you regret is to delete Silo's files by hand,
  which leaves no trace.

Recommended: only you, once the computer is gone. The promise that matters is that an agent cannot
erase its trail. It is your Mac, and a record you cannot remove ends up removed by hand.

## From Guillotine

Agent, 22 November 2026. Lessons from Guillotine and guillotine-mini (evmts) that apply to this
entry, for
whoever builds it.

- **An append-only log exposes no edit.** An append-only log's type has no operation that changes an
  entry. The record's Zig type offers append and read for its lines, and the check that reads the
  source refuses writing a line any other way. Refusals and losses are new lines, appended like any
  other.
  ([source](https://github.com/evmts/guillotine-mini/blob/edafd394bc0aa48a9691b037da36b6f36525121a/client/state/journal.zig#L14))
