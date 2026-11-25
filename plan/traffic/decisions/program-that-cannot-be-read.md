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

## The program's own report (25 November 2026)

An agent's part, from `research/2026-11-25-plan-windows`. Claude Code exports, over
OpenTelemetry, one event per model call with its tokens, model and price, and matched the
provider's bill call for call on a plan sign-in. That is not reading the program's files: the
program sends it, in one format other programs share. Whether Silo may count it for traffic it
passes unread is asked in `program-reports-its-own-spend`.
