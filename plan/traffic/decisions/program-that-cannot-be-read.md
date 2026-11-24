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
