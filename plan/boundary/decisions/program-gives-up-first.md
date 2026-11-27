# What does Silo see when an agent program gives up on a hold

## The choice

An agent's part, 22 November 2026. A rule holds a push from main to github.com, and an ask waits in
your Silo app. Thirty seconds in, the agent program gives up on the command and closes its
connection.

The options:

- The hold ends at once (recommended). The ask closes in your app. The record says the connection
  closed while the push was held, and not why, since a crash or a kill closes it the same way. A
  program that gives up without closing anything is not seen, and its hold ends at the limit. It
  costs an ask that can vanish just as you were about to answer it.
- The hold runs to its limit. The ask stays in your app until then. It costs your attention, since
  you can say yes to a push that no longer exists, and that yes does nothing.

Recommended: the hold ends at once. An ask in your app should always be one your answer can still
change.

## Notes

The record marks this line and the next "(Owner; the mechanism Agreed)". The content map reads only
`hold-limit` as in force, so this line waits for polarzero.

## How each program gives up on a held command (24 November 2026)

An agent's part, 24 November 2026. Measured with the rig of `hold-limit-length`. In every case the
program or the tool closed the held connection, and the held host saw the close the moment it
happened. No case gave up without closing.

- Claude Code stopped a held `curl` at 720 seconds: 120 in the foreground, then the 10 minutes a
  background command may run. It killed the command, which closed the connection, and told the
  model "stopped after reaching its background time limit".
- Codex killed a held `curl` when its turn ended, after 10 or 30 seconds in these runs, because the
  fake model ended the turn instead of waiting on the command.
- pip closed each try at 15 seconds, then sent the same request again. To Silo that is a close
  followed by a new request, which this answer ends and the next one starts again. That is the
  case `hold-limit-length` proposes to join to the open ask.
