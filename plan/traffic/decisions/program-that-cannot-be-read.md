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

## A stream that will end with no count (25 November 2026)

An agent's part, 25 November 2026, from `research/2026-11-25-spend-budget-edges`. A Chat
Completions stream that does not set `stream_options.include_usage` ends with no count, on a host
Silo reads. Its reasoning streams as nothing, so the visible text missed 93% of its cost in the run
($0.010 counted for $0.157 billed). Counting by time at the model's speed came to $0.161, but only
because the stand-in's speed was known exactly; a real model's speed varies.

This is the same case as the sentence #364 and #373 propose for this entry, traffic Silo cannot
count while a budget covers the computer, inside a host Silo reads. The options under a budget:

- Refuse such a stream (recommended). Built: it gets a 400 in OpenAI's shape, "a budget covers
  this computer, and this stream does not ask for its usage, so Silo could not count it. Set
  stream_options.include_usage to true." Streams that ask for usage, and calls not streamed, pass.
  It costs a program that cannot be told to ask for usage.
- Let it pass, settled on an estimate by time, and marked estimated in the app. It costs a count
  that can be several times off either way.
- Add `include_usage` to the request. It changes the request, which `what-silo-changes-in-traffic`
  rules out, and adds a chunk the program did not ask for.

Not checked: which programs leave `include_usage` unset. Aider, Cline and OpenCode, which #369
found can be switched to any OpenAI-compatible host, should be captured before this is ruled.
