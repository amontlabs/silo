# Does every wait at the boundary end at Silo's own limit

## The choice

An agent's part, 22 November 2026. You installed an extension that gives its opinion on each tool
call of main before the call goes on. Its computer hangs while a call from main waits for it, and
the agent program that made the call has no time limit of its own.

The options:

- Silo's own limit, always (recommended). Silo ends the wait itself, at a limit it keeps, whatever
  is stuck: the extension, your app or the agent program. The call is then denied or left to your
  rules, as the setting of that extension says. A hold for your yes ends the same way. If the part
  of Silo that holds the call crashes, the call fails with it and nothing passes. It costs a call
  refused, or passed to the rules unchecked, when a slow extension would have answered a moment
  later. On a crash, it also costs a broken connection, which the agent reads less well than a
  refusal.
- The extension's own limit. Each extension says how long it may take. It costs you control, since
  an extension that is stuck or hostile keeps your agents waiting for as long as it wrote.
- Best effort. The wait ends when the opinion comes, or when the agent program gives up. It costs an
  agent stuck for good behind an extension that hangs and a program that never gives up.

Recommended: Silo's own limit, always. A hold already ends this way, and a limit that depends on the
extension or on the agent program does not end the wait when either of them is stuck.

## Notes

The record's wording, Proposed: "Waiting for a classifier's opinion is not a hold: a hold waits
for you. That wait ends at Silo's limit in the same way, and what then happens to the request is
the default of 14.1, question 17." That default is asked in `extension-that-does-not-answer` in `extensions`.

The property it sets is row 35 of the record's section 10, whole. For a hold, the part in force
is `hold-limit`.

- That no wait at the boundary ends by best effort is polarzero's, said of the limits on a hold and
  on every wait at the boundary.

## How long the agent program waits on its model (24 November 2026)

An agent's part, 24 November 2026. A classifier that judges a tool call in the model's reply holds
that reply, so its wait is bounded by how long the program waits on its model. Measured in
`how-a-hold-ends`: with nothing new on the stream, Claude Code 2.1.331 and Codex 0.172.0 both
wait 300 seconds, then send the request again, and that second request is paid again. Silo's limit
on a classifier's wait has to stay well under 300 seconds, or the program sends the request a second
time while the first one is still held.
