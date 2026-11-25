# How the record stores what is sent to a model provider

## The choice

An agent's part, 25 November 2026, from the challenge of #355 (page 5) to `store-messages-once`, which
polarzero took.

The case: you paste a database password into a chat, `hunter22`. Silo keeps the value out of the
stored text and puts a marker in its place (#355's `credential-found-in-text`). The record still
names the message by a plain hash of its text. Anyone who holds the record and the rest of the
message tries every short password until the hash matches, offline, in minutes.

The options:

- A keyed hash for every message, with a key that stays on your Mac (recommended). It costs nothing
  measurable against a plain hash. A record copied to another Mac, as a team might (#362), cannot
  share stored messages with that Mac's record, since the keys differ.
- A keyed hash only for a message that holds a marker (#355's wording). It costs two ways of naming
  a message, and a reader that has to know which one applies.
- A plain hash, as the earlier answer says. It keeps the case above open.

Recommended: a keyed hash for every message. One way of naming messages, and the guess test fails
for all of them. The direction polarzero took, store each message once, does not change.
