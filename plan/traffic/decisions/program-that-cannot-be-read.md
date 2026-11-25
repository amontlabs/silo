# What does Silo know of an agent program whose traffic cannot be read

## The choice

An agent's part, 22 November 2026. You try a new agent program in main. It talks to a model provider
whose format Silo cannot read yet. Main has a budget of 50 dollars.

The options:

- What Silo sees, and no more (recommended). The record holds the hosts, the sizes and the timing of
  that traffic. The app says that this program's conversations are not read, and main's budget does
  not count what it spends. It costs turns, tool calls and spend for that program until Silo reads
  its provider.
- Also read the program's own files. Silo reads the conversation the program keeps in the computer
  and shows it, marked as told by the program. It costs a reader for each program, kept up with each
  of its versions, and an agent can rewrite those files.

Recommended: what Silo sees, and no more. A few providers carry nearly all model traffic, so reading
a new provider covers every program that uses it, where a reader of files covers one program.

## A known model host under a budget (24 November 2026)

An agent's part, 24 November 2026, from `research/2026-11-24-spend-budgets`. #351 proposed that a connection to a known model
host Silo cannot count is denied while a budget covers the computer. The prototype does it: a
request to `api.mistral.ai` from a computer with a budget got 400 in OpenAI's shape, which most
such hosts copy, with "Silo: api.mistral.ai is a model host Silo cannot count, and a budget covers
this computer." Without a budget the connection passes.

## A program that refuses Silo's certificate (25 November 2026)

An agent's part, 25 November 2026, from `research/2026-11-25-tls-refusals`. #403 found that Codex
without Silo's authority says "waiting for network" for ever, and that only the side that ends TLS
can see why. #395's boundary already wrote a line for it, but the same `handshake-failed` for ten
causes: a refused certificate, a client that sent nothing, plain HTTP, no common TLS version. The
prototype tells them apart from how far the handshake went and the alert the client sent, and
names the client by its ClientHello, since a program that refuses never sends a request.

Seven real clients were run with and without the authority. Refusals come three ways: an alert
(curl, Python, Go: `unknown_ca` or `bad_certificate`; Codex: `unknown_ca`), a close once the
certificate arrived (Node, Claude Code), or a finished handshake followed by a close with no
request (git on GnuTLS, and a pinned key in curl), which looks like any program giving up and
counts only when it repeats. No run of a trusting program gave a line that reads as a refusal. A
stuck Codex makes about one refusal every 6 s, about 800 bytes of record each with its
`opened` line.
