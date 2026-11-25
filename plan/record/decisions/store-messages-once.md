# Storing what is sent to a model provider

Agreed as a direction. Every number around it is a rough estimate, to find out
(`record-disk-size`).

The trap it avoids: when a provider format resends the conversation history, each request
repeats the whole conversation so far, and raw traffic grows with the square of a conversation's
length.

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
