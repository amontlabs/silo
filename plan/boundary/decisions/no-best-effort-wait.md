# May a wait at the boundary end by best effort

## For the owner

An agent's part, 22 November 2026. An extension gives its opinion on each tool call of main before
the call goes on. Its computer hangs while a call waits for it, and the agent program that made the
call has no time limit of its own.

The options:

- Silo's own limit, always (recommended). Silo ends the wait at a limit it keeps itself, whatever is
  stuck: the extension, your app or the agent program. The call is then denied or left to your
  rules, as that extension's setting says. A hold for your yes ends the same way. If the part of
  Silo that holds the call crashes, the call fails with it and nothing passes. It costs a call
  refused when the opinion would have come a moment later. On a crash, it also costs a broken
  connection, which the agent reads less well than a refusal.
- Best effort. The wait ends when the opinion comes, or when the agent program gives up. It costs an
  agent stuck for good behind an extension that hangs and a program that never gives up.

Recommended: Silo's own limit, always. A limit that depends on the extension or on the agent program
does not end the wait when either of them is stuck.

## Notes

- The owner said this, of the limits on a hold and on every wait at the boundary.
- "Here" means those waits; the sentence is copied as the record has it.
