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

## Local models (24 November 2026)

An agent's part, 24 November 2026. A model served from your Mac, rather than inside the computer,
does cross the boundary, in plain HTTP. Aider 0.86.2 with an `ollama_chat/` model sent
`POST /api/chat` to port 11434, Ollama's own API, whose reply is a stream of JSON lines rather than
of events. Goose 1.60.2 with Ollama sent Chat Completions to the same port. LM Studio's port is
1234. So Silo can read a model on the Mac without any certificate, with a reader for Chat
Completions and one for Ollama's lines. Whether to add the second is open. A model the agent runs
inside the computer sends nothing across, as this entry says.

## Programs that make another provider one setting away (24 November 2026)

An agent's part, 24 November 2026. Cline (`-P openrouter`), OpenCode (a model in its settings),
Qwen Code (`OPENAI_BASE_URL`) and Aider (`--model`) each switch to another provider with one
change that an agent with full access can make itself. Traffic to a provider Silo does not read is
outside spend and budgets, as this entry says. So under a budget, the rule that denies model
traffic Silo does not read (the research for #351) has to be on by default, or the budget is one
setting away from not counting.
