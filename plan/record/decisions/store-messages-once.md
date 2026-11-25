# Storing what is sent to a model provider

Agreed as a direction. Every number around it is a rough estimate, to find out
(`record-disk-size`).

The trap it avoids: when a provider format resends the conversation history, each request
repeats the whole conversation so far, and raw traffic grows with the square of a conversation's
length.

## Challenged, 25 November 2026

An agent's part. #355 challenge this answer on one case: a plain hash lets anyone holding the record test guesses of a short password Silo kept out of a message. The replacement is `message-identity-keyed`, which names this decision in `replaces`, for polarzero to take or drop. Until then this answer stays in force. The reasons are in `research/2026-11-25-challenged-decisions/README.md`.
