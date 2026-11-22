# The text of conversations

The plan's notes say none is: the conversation is disposable. Opening an alert on the turn that
led to it needs that turn's text. The question is polarzero's.

A long conversation takes a few megabytes, as a rough estimate (`record-disk-size`).

## The choice

An agent's part, 22 November 2026. You open an alert and want to read the turn that led to it, or a
receipt that shows what you asked word for word. Both need the text of your conversations with
agents to be in the record.

The options:

- Kept, first to go (recommended). The text is kept once on your Mac. When the record is full it is
  removed right after recordings, with a line saying so. It costs a few megabytes for a long
  conversation, as a rough estimate, and your conversations sit in the record.
- Not kept. No conversation text is in the record. It costs the line of a receipt with your request
  and the turn behind an alert. A promise in force on what was sent would have to change.

Recommended: kept, first to go. The receipt, the alert that opens on a turn and that promise all
need the text.

## Found by an audit, 22 November 2026

An agent's part. This asks whether the text of conversations is kept, and its notes say none is.
`store-messages-once` (taken) stores each message by hash, `what-a-receipt-holds` shows what you
asked the agent word for word, and `record-size-limit` removes message text after recordings. If the
answer is no, the receipt cannot show your request. The audit proposes: "Yes. The text of messages
is kept once, by hash, as bulky content. It is the first text to go at the record's size limit,
after recordings."
