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

## A count of calls as the backstop

Agent, 25 November 2026, from `research/2026-11-25-techniques-from-two-tools/`. An open-source
gateway's last bound counts an event it causes itself, not dollars a reply reports, so no gap in
reading usage can undercount it. For Silo, a limit on the number of model calls a computer makes
holds where money cannot be counted: plan traffic, Chat Completions streams without
`include_usage`, a host Silo cannot read. The same gateway refuses batch routes
(`/v1/messages/batches`) rather than count them. #374 did not build batches or `background: true`
responses; until they are counted, refusing them in the provider's shape while a budget is set is
the simplest answer.
