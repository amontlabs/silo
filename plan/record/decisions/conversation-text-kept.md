# The text of conversations

The plan's notes say none is: the conversation is disposable. Opening an alert on the turn that
led to it needs that turn's text. The question is the owner's.

A long conversation takes a few megabytes, as a rough estimate (`record-disk-size`).

## Found by an audit, 22 November 2026

An agent's part. This asks whether the text of conversations is kept, and its notes say none is.
`store-messages-once` (taken) stores each message by hash, `what-a-receipt-holds` shows what you
asked the agent word for word, and `record-size-limit` removes message text after recordings. If the
answer is no, the receipt cannot show your request. The audit proposes: "Yes. The text of messages
is kept once, by hash, as bulky content. It is the first text to go at the record's size limit,
after recordings."
