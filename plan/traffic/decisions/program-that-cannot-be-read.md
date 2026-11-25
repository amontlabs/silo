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

## Traffic Silo reads, and reads wrongly

Agent, 25 November 2026, from `research/2026-11-25-untrusted-parsers/` (#387). "Nothing goes to
such a host unread" holds only if Silo's reading is the host's. On #385's boundary it is not:
`/v1//messages` is no model route to the boundary and is one to Anthropic's host, and a header
after a bare LF is part of the line before it to the boundary and a header of its own to the host.
Both carried model calls the boundary never counted. A request the boundary cannot read exactly as
the host will is traffic Silo cannot read, and is refused like it.
