# Storing what is sent to a model provider

Agreed as a direction. Every number around it is a rough estimate, to find out
(`record-disk-size`).

The trap it avoids: when a provider format resends the conversation history, each request
repeats the whole conversation so far, and raw traffic grows with the square of a conversation's
length.

## Challenged, 25 November 2026

An agent's part. #355 challenge this answer on one case: a plain hash lets anyone holding the record test guesses of a short password Silo kept out of a message. The replacement is `message-identity-keyed`, which names this decision in `replaces`, for polarzero to take or drop. Until then this answer stays in force. The reasons are in `research/2026-11-25-challenged-decisions/README.md`.

## Challenge: the list of hashes grows with the square too

An agent's part, 25 November 2026, from [the record's
format](../../../research/2026-11-25-record-format/README.md), finding 4. "A request is then a short
list of hashes" is not short. In a conversation of 150 turns the last request lists 300 hashes of 71
bytes each, and the requests of the whole conversation list about 22,600 in all. On a modelled night
of three agents for 8 hours, model-call lines take 49.6 MB with a full list on each, and 5.5 MB when
each request names the model-call line whose request it extends and lists only the messages added
since. The messages themselves take 13.1 MB stored once, against 935 MB stored whole with each
request.

In its place: "Silo stores each retained message once by hash. A request names the request it
extends and lists the messages it added; when the program changed an earlier message, it lists all
of them." Claude Code changes earlier messages it resends (#371, finding 1), so some requests carry
the full list; the model assumed 5% of them. How often it happens on a real night is to measure.

## Where the drafts of 25 November disagreed (integration)

An agent's part, 25 November 2026, written while merging every draft into one branch (`research/2026-11-25-integration-2/`).

#376 and #386 each challenge this answer, for different reasons: #376 for a keyed hash, #386 for the size of the list. One replacement that holds both is proposed in the notes of `message-identity-keyed`.

## One store for every computer on the Mac (25 November 2026)

An agent's part, 25 November 2026, from `research/2026-11-25-forked-conversations`. A fork copies its parent's conversations, and its
programs resend them. On seven kinds of fork with Claude Code and Codex, a store kept per computer
held 5.3 and 5.6 times the bytes of one store shared by every computer on the Mac. The store should
be one per Mac, with each computer's record pointing into it; what a computer may read of it stays
its own, as `computer-reads-its-record` says.

## Settled in the consolidation (27 November 2026)

Settled, 27 November 2026, in the consolidation: the replacement is `message-identity-keyed`, whose answer now holds both.
