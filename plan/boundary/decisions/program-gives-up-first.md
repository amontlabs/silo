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
`hold-limit` as in force, so this line waits for the owner.
