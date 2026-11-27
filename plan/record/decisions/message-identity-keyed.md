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

## Where the drafts of 25 November disagreed (integration)

An agent's part, 25 November 2026, written while merging every draft into one branch (`research/2026-11-25-integration-2/`).

Two drafts propose a replacement for the taken `store-messages-once`, for different reasons.

- #376 (this entry): name each message by a keyed hash whose key stays on the Mac, so a hash in the record cannot be used to test guesses.
- #386: "a short list of hashes" is not short. On a modelled night the full lists take 49.6 MB, against 5.5 MB when each request names the request it extends and lists only the messages it added.

They do not conflict. Recommended: one replacement that holds both. "Silo stores each retained message once, named by a keyed hash whose key stays on your Mac. A request names the request it extends and lists the messages it added; when the program changed an earlier message, it lists all of them."

## Settled in the consolidation (27 November 2026)

Settled, 27 November 2026, in the consolidation: the answer now holds both, as recommended.
