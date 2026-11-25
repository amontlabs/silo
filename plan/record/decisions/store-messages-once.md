# Storing what is sent to a model provider

Agreed as a direction. Every number around it is a rough estimate, to find out
(`record-disk-size`).

The trap it avoids: when a provider format resends the conversation history, each request
repeats the whole conversation so far, and raw traffic grows with the square of a conversation's
length.

## One store for every computer on the Mac (25 November 2026)

An agent's part, 25 November 2026, from `research/2026-11-25-forked-conversations`. A fork copies its parent's conversations, and its
programs resend them. On seven kinds of fork with Claude Code and Codex, a store kept per computer
held 5.3 and 5.6 times the bytes of one store shared by every computer on the Mac. The store should
be one per Mac, with each computer's record pointing into it; what a computer may read of it stays
its own, as `computer-reads-its-record` says.
